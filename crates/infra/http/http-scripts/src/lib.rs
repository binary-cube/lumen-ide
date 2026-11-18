//! A request's JavaScript run by QuickJS (`rquickjs`): `http_requests::script`'s port. Each
//! script runs in an engine of its own, made for it and let go after (nothing is kept, no
//! script sees another's but through `client.global`), with a memory limit and a time limit
//! (a loop that never ends is stopped). The HTTP client API is plain JavaScript over the data
//! given (`prelude.js`): the script reaches no disk, no network, nothing of the app.

use std::time::{Duration, Instant};

mod jwt_keys;

use http_requests::script::{
    Phase, ScriptEngine, ScriptRequest, ScriptResponse, ScriptRun, TestResult,
};
use serde_json::{Value, json};

/// The API a script is given.
const PRELUDE: &str = include_str!("prelude.js");
/// The most memory a script's engine takes.
const MEMORY: usize = 32 * 1024 * 1024;
/// The longest a script runs.
const LONGEST: Duration = Duration::from_secs(5);

/// The engine (it holds nothing between scripts).
#[derive(Debug, Default)]
pub struct QuickJs {
    /// Variables of the environment a script may read (`request.environment.get`).
    environment: std::collections::BTreeMap<String, String>,
    /// The file's own variables (`client.variables.file`).
    file: std::collections::BTreeMap<String, String>,
}

impl QuickJs {
    pub fn new() -> Self {
        Self::default()
    }

    /// The same, the environment's variables readable by its scripts.
    pub fn with_environment(environment: std::collections::BTreeMap<String, String>) -> Self {
        Self { environment, file: Default::default() }
    }

    /// The same, the file's own variables readable too.
    pub fn with_file(mut self, file: std::collections::BTreeMap<String, String>) -> Self {
        self.file = file;
        self
    }
}

fn headers_json(headers: &[http_requests::Header]) -> Value {
    Value::Array(headers.iter().map(|h| json!({"name": h.name, "value": h.value})).collect())
}

impl ScriptEngine for QuickJs {
    fn run(
        &self,
        source: &str,
        phase: Phase,
        request: &ScriptRequest,
        response: Option<&ScriptResponse>,
        run: &mut ScriptRun,
    ) {
        let response = match (phase, response) {
            // Its body once, as text (the prelude parses a JSON one).
            (Phase::After, Some(r)) => json!({
                "status": r.status,
                "headers": headers_json(&r.headers),
                "body": r.body,
                "content_type": r.content_type,
                "charset": r.charset,
            }),
            _ => Value::Null,
        };
        let input = json!({
            "globals": run.globals,
            "environment": self.environment,
            "file": self.file,
            "iteration": run.iteration,
            "request": {
                "method": request.method,
                "url": request.url,
                "headers": headers_json(&request.headers),
                "body": request.body,
            },
            "response": response,
        });
        // The script in a function of its own: its names don't meet the API's.
        let program = format!(
            "const __in = {input};\n{PRELUDE}\n\
             (async function (client, request, response) {{\n{source}\n}})(client, request, response)\n\
               .catch((e) => {{ if (!(e && e.__exit)) __out.error = e && e.message ? e.message : String(e); }});"
        );
        match evaluate(&program) {
            Ok(out) => read_back(&out, run),
            Err(why) => run.error = Some(why),
        }
    }
}

/// `program` run in an engine made for it: its last value (a JSON text), or why it stopped.
fn evaluate(program: &str) -> Result<String, String> {
    let runtime = rquickjs::Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(MEMORY);
    runtime.set_max_stack_size(512 * 1024);
    let started = Instant::now();
    runtime.set_interrupt_handler(Some(Box::new(move || started.elapsed() > LONGEST)));
    let context = rquickjs::Context::full(&runtime).map_err(|e| e.to_string())?;
    // What the prelude can't do in JavaScript, given natively: hashes, HMACs, waiting — no
    // more (nothing reaches the disk, the network, the app).
    context.with(|ctx| natives(&ctx, started)).map_err(|e| e.to_string())?;
    let said = |ctx: &rquickjs::Ctx<'_>, e: rquickjs::Error| match e {
        rquickjs::Error::Exception => {
            let thrown = ctx.catch();
            let text = thrown
                .as_exception()
                .map(|ex| ex.message().unwrap_or_default())
                .or_else(|| thrown.as_string().and_then(|s| s.to_string().ok()))
                .unwrap_or_else(|| "the script threw".to_owned());
            if started.elapsed() > LONGEST {
                format!("stopped after {} s", LONGEST.as_secs())
            } else {
                text
            }
        }
        other => other.to_string(),
    };
    context.with(|ctx| ctx.eval::<(), _>(program).map_err(|e| said(&ctx, e)))?;
    // What it left waiting (a promise's `then`, an `await`, a timer) done before its results
    // are read, the timers in the order they're due.
    for _ in 0..1000 {
        while runtime.is_job_pending() {
            if started.elapsed() > LONGEST || runtime.execute_pending_job().is_err() {
                break;
            }
        }
        let more =
            context.with(|ctx| ctx.eval::<bool, _>("__runTimer()").map_err(|e| said(&ctx, e)))?;
        if !more || started.elapsed() > LONGEST {
            break;
        }
    }
    context.with(|ctx| ctx.eval::<String, _>("JSON.stringify(__out)").map_err(|e| said(&ctx, e)))
}

/// The native functions the prelude uses: `__digest(algorithm, hex)`, `__hmac(algorithm,
/// keyHex, dataHex)` (hexadecimal in and out), `__jwtSign` / `__jwtVerify` (a key's DER as
/// hexadecimal, [`jwt_keys`]), `__sleep(ms)` (no longer than the time left).
fn natives(ctx: &rquickjs::Ctx<'_>, started: Instant) -> rquickjs::Result<()> {
    use http_requests::hash::{Algorithm, digest, from_hex, hex, hmac};
    let globals = ctx.globals();
    globals.set(
        "__digest",
        rquickjs::Function::new(ctx.clone(), |algorithm: String, data: String| -> String {
            match (Algorithm::named(&algorithm), from_hex(&data)) {
                (Some(a), Some(bytes)) => hex(&digest(a, &bytes)),
                _ => String::new(),
            }
        })?,
    )?;
    globals.set(
        "__hmac",
        rquickjs::Function::new(
            ctx.clone(),
            |algorithm: String, key: String, data: String| -> String {
                match (Algorithm::named(&algorithm), from_hex(&key), from_hex(&data)) {
                    (Some(a), Some(key), Some(bytes)) => hex(&hmac(a, &key, &bytes)),
                    _ => String::new(),
                }
            },
        )?,
    )?;
    // JWT signed with keys: `ok:<hex>` or `error:<why>` (the prelude throws it).
    globals.set(
        "__jwtSign",
        rquickjs::Function::new(
            ctx.clone(),
            |algorithm: String, key: String, data: String| -> String {
                let (Some(key), Some(data)) = (from_hex(&key), from_hex(&data)) else {
                    return "error:not hexadecimal".to_owned();
                };
                match jwt_keys::sign(&algorithm, &key, &data) {
                    Ok(signed) => format!("ok:{}", hex(&signed)),
                    Err(why) => format!("error:{why}"),
                }
            },
        )?,
    )?;
    globals.set(
        "__jwtVerify",
        rquickjs::Function::new(
            ctx.clone(),
            |algorithm: String, key: String, data: String, sig: String| -> String {
                let (Some(key), Some(data), Some(sig)) =
                    (from_hex(&key), from_hex(&data), from_hex(&sig))
                else {
                    return "error:not hexadecimal".to_owned();
                };
                match jwt_keys::verify(&algorithm, &key, &data, &sig) {
                    Ok(valid) => format!("ok:{valid}"),
                    Err(why) => format!("error:{why}"),
                }
            },
        )?,
    )?;
    globals.set(
        "__sleep",
        rquickjs::Function::new(ctx.clone(), move |ms: f64| {
            let left = LONGEST.saturating_sub(started.elapsed());
            let wait = Duration::from_millis(ms.max(0.0) as u64).min(left);
            // The script's own worker waits (never the UI's).
            std::thread::sleep(wait);
        })?,
    )?;
    Ok(())
}

/// What the script left (`__out`) into `run`.
fn read_back(out: &str, run: &mut ScriptRun) {
    let Ok(value) = serde_json::from_str::<Value>(out) else {
        run.error = Some("the script's results couldn't be read".to_owned());
        return;
    };
    let strings = |key: &str| -> std::collections::BTreeMap<String, String> {
        value
            .get(key)
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    run.globals = strings("globals");
    // `client.global.headers`: set (a value) or cleared (null).
    if let Some(headers) = value.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            run.headers.insert(name.clone(), value.as_str().map(str::to_owned));
        }
    }
    // It threw, after what it logged and tested (kept).
    if let Some(why) = value.get("error").and_then(Value::as_str) {
        run.error = Some(why.to_owned());
    }
    run.variables.extend(strings("variables"));
    if let Some(logs) = value.get("logs").and_then(Value::as_array) {
        run.logs.extend(logs.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if let Some(tests) = value.get("tests").and_then(Value::as_array) {
        run.tests.extend(tests.iter().map(|t| TestResult {
            name: t.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
            failure: t.get("failure").and_then(Value::as_str).map(str::to_owned),
        }));
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
