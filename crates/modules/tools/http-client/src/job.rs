//! A request run: started on the UI thread (cheap: the file's text read, the run noted),
//! done on the pool `http-run` — the environments read, the variables resolved (the
//! request's file's, the chosen environment's; a name missing ends it, nothing sent), the
//! body's files read as sent, the request sent with the project's cookies, the response's
//! body saved for the editor (`<project dir>/http/responses/<time>.<status>.<extension>`,
//! the newest [`KEPT`] kept), the cookies written back.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use http_requests::cookies::CookieJar;
use http_requests::script::{Phase, ScriptEngine, ScriptRequest, ScriptResponse, ScriptRun};
use http_requests::transport::{
    HttpTransport, IncomingBody, Outgoing, OutgoingPart, SendOptions, TransportError,
};
use http_requests::vars::{Dynamic, Environment, FileVariables, Layers, resolve_request};
use http_requests::{BodyPart, Request};
use kernel::work::Cancel;

use crate::runs::{Outcome, Run, RunState, Runs};

/// The pool runs are done on.
pub const THREAD: &str = "http-run";

/// The responses kept for the editor.
const KEPT: usize = 50;

/// The clock and the dice of the dynamic variables.
#[derive(Debug, Default)]
pub struct System {
    /// The project's folder (`{{$projectRoot}}`; `None`: a file alone).
    pub root: Option<PathBuf>,
    /// Where its responses are kept (`{{$historyFolder}}`).
    pub history: Option<PathBuf>,
}

impl Dynamic for System {
    #[allow(clippy::disallowed_methods)] // The module's clock, for `{{$timestamp}}`.
    fn now(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }

    fn uuid(&mut self) -> String {
        kernel::id::uuid_text(kernel::id::new_uuid())
    }

    fn random(&mut self) -> u64 {
        kernel::id::new_uuid() as u64
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn project_root(&self) -> Option<String> {
        self.root.as_ref().map(|p| p.to_string_lossy().into_owned())
    }

    fn history_folder(&self) -> Option<String> {
        self.history.as_ref().map(|p| p.to_string_lossy().into_owned())
    }
}

/// The transport every run goes through (its agents shared).
fn transport() -> Arc<http_transport::UreqTransport> {
    static TRANSPORT: std::sync::OnceLock<Arc<http_transport::UreqTransport>> =
        std::sync::OnceLock::new();
    Arc::clone(TRANSPORT.get_or_init(http_transport::UreqTransport::new))
}

/// The folder made (if it isn't) and kept out of git whatever else ignores it: its own
/// `.gitignore` ignores everything in it (sessions, responses — never committed).
pub(crate) fn ready_folder(folder: &Path) -> bool {
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let ignore = folder.join(".gitignore");
    if !ignore.exists() {
        let _ = files::write_atomic(&ignore, b"*\n");
    }
    true
}

/// The project's folder of the module's own files (`<root>/<project dir>/http`), or the
/// temporary folder's (a file opened alone).
pub(crate) fn own_folder(runs: &Runs) -> PathBuf {
    match runs.root() {
        Some(root) => root.join(kernel::brand::PROJECT_DIR).join("http"),
        None => std::env::temp_dir().join("http-requests"),
    }
}

/// Runs the request of `text` (the file `file` as in the editor) at byte `offset`, after
/// the requests whose responses it uses and that haven't run yet (theirs first); `false`:
/// no request there. A loop, or a request it names that the file hasn't: said, nothing run.
pub fn run_at(runs: &Arc<Runs>, file: &Path, text: &str, offset: usize) -> bool {
    let parsed = http_requests::parse(text);
    let Some(at) = parsed.index_at(offset) else { return false };
    let fresh = |name: &str| runs.latest(file, name).is_some();
    match http_requests::chain::order(&parsed, at, &fresh) {
        Ok(order) => {
            let chain = order.into_iter().map(|i| parsed.requests[i].clone()).collect();
            start(runs, file, text, chain, &parsed.variables);
        }
        Err(problem) => refuse(runs, file, text, &parsed.requests[at], &problem),
    }
    true
}

/// A chain that can't run (a loop, a request it names that the file hasn't): said as the
/// run of `request` that failed.
fn refuse(
    runs: &Runs,
    file: &Path,
    text: &str,
    request: &Request,
    problem: &http_requests::chain::ChainError,
) {
    let why = match problem {
        http_requests::chain::ChainError::Unknown(name) => {
            i18n::tr!("http.unknown_request", name = name.as_str())
        }
        http_requests::chain::ChainError::Loop(names) => {
            i18n::tr!("http.loop", names = names.join(" → ").as_str())
        }
    };
    let id = runs.begin(run_of(file, text, request));
    runs.end(id, RunState::Failed(why));
}

/// Runs every request of `text` in the order its chain allows (each after the ones it uses):
/// a graph's Play. A loop (or a name the file hasn't): said on the first request it holds,
/// nothing run.
pub fn run_graph(runs: &Arc<Runs>, file: &Path, text: &str) -> bool {
    let parsed = http_requests::parse(text);
    match http_requests::chain::order_all(&parsed) {
        Ok(order) => {
            let chain = order.into_iter().map(|i| parsed.requests[i].clone()).collect();
            start(runs, file, text, chain, &parsed.variables);
            true
        }
        Err(problem) => {
            let at = match &problem {
                http_requests::chain::ChainError::Loop(names) => {
                    names.first().and_then(|n| parsed.named(n))
                }
                http_requests::chain::ChainError::Unknown(_) => None,
            };
            let Some(request) = at.or(parsed.requests.first()) else { return false };
            refuse(runs, file, text, request, &problem);
            true
        }
    }
}

/// Runs every request of `text`, one after the other (in the order written).
pub fn run_all(runs: &Arc<Runs>, file: &Path, text: &str) -> bool {
    let parsed = http_requests::parse(text);
    if parsed.requests.is_empty() {
        return false;
    }
    start(runs, file, text, parsed.requests, &parsed.variables);
    true
}

/// A run of `request` about to begin.
fn run_of(file: &Path, text: &str, request: &Request) -> Run {
    let line = request.line_range.clone();
    Run {
        id: kernel::id::new_uuid(),
        number: 0,
        file: file.to_path_buf(),
        label: request.label(),
        method: request.method.clone(),
        hash: ui_components::editor_toolbar::text_hash(text.get(line.clone()).unwrap_or_default()),
        line,
        line_number: request.line,
        state: RunState::Running,
        started: Instant::now(),
        cancel: Cancel::new(),
    }
}

/// `chain` run in order, in one job (each sees the responses before it); one failing, the
/// rest isn't run.
fn start(
    runs: &Arc<Runs>,
    file: &Path,
    text: &str,
    chain: Vec<Request>,
    variables: &[http_requests::FileVariable],
) {
    let steps: Vec<(u128, Cancel, Request)> = chain
        .into_iter()
        .map(|request| {
            let run = run_of(file, text, &request);
            let cancel = run.cancel.clone();
            (runs.begin(run), cancel, request)
        })
        .collect();
    let environment = runs.environment(file);
    let (runs, file, variables, text) =
        (Arc::clone(runs), file.to_path_buf(), variables.to_vec(), text.to_owned());
    kernel::work::run(THREAD, move || {
        // Read once for the whole chain (every request, every element of a collection).
        let environments = http_environments::of(&file, runs.root().as_deref());
        let mut failed: Option<String> = None;
        for (id, cancel, request) in steps {
            // A variable holding a collection: one request for each of its elements, each
            // its own run.
            let elements = match &failed {
                Some(_) => None,
                None => collection(&environments, &request, &variables, environment.as_deref()),
            };
            let rounds: Vec<Iteration> = match elements {
                Some((name, values)) => values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| Iteration { index, value: Some((name.clone(), value)) })
                    .collect(),
                None => vec![Iteration::default()],
            };
            for (n, round) in rounds.iter().enumerate() {
                let id = if n == 0 { id } else { runs.begin(run_of(&file, &text, &request)) };
                let outcome = match &failed {
                    Some(label) => {
                        RunState::Failed(i18n::tr!("http.not_run", name = label.as_str()))
                    }
                    None => {
                        let step = Step {
                            runs: &runs,
                            environments: &environments,
                            file: &file,
                            request: &request,
                            variables: &variables,
                            environment: environment.as_deref(),
                            cancel: &cancel,
                        };
                        execute(step, round, &|piece: &[u8]| runs.append_live(id, piece))
                    }
                };
                let ok = matches!(&outcome, RunState::Done(o) if o.response.status < 400);
                if !ok && failed.is_none() {
                    failed = Some(request.label());
                }
                runs.end(id, outcome);
            }
        }
    });
}

/// The OAuth tokens `request` uses (`{{$auth.token("id")}}`, `{{$auth.idToken("id")}}`),
/// had and put in `own` (secrets) under the names it writes them by.
fn oauth_tokens(
    Step { runs, request, variables, environments, environment, cancel, .. }: Step<'_>,
    own: &mut std::collections::BTreeMap<String, http_requests::vars::Value>,
) -> Result<(), String> {
    let mut texts = vec![request.url.clone()];
    texts.extend(request.headers.iter().map(|h| h.value.clone()));
    texts.extend(request.body.iter().filter_map(|p| match p {
        BodyPart::Text(t) => Some(t.clone()),
        BodyPart::File(_) => None,
    }));
    let used: Vec<(String, bool)> =
        texts.iter().flat_map(|t| http_requests::oauth::uses(t)).collect();
    if used.is_empty() {
        return Ok(());
    }
    let chosen = Environment { environments, chosen: environment };
    let file_variables = FileVariables(variables);
    let lookup = Layers::new().then(&chosen).then(&file_variables);
    let text = |t: &str| {
        http_requests::vars::resolve(t, &lookup, &mut System::default())
            .map_or_else(|_| t.to_owned(), |r| r.text)
    };
    let transport = transport();
    let open = |url: &str| runs.ask_browser(url);
    let tell = |text: &str| runs.tell(text);
    let ctx = crate::auth::Context {
        transport: transport.as_ref(),
        now: System::default().now(),
        open: &open,
        tell: &tell,
        cancel,
    };
    for (id, id_token) in used {
        let failed =
            |why: String| i18n::tr!("http.auth_failed", id = id.as_str(), why = why.as_str());
        let config = environments
            .security(environment)
            .and_then(|security| http_requests::oauth::config(security, &id, &text))
            .ok_or_else(|| failed(i18n::t("http.auth_not_configured")))?;
        let token = crate::auth::token(&runs.tokens, environment, &id, &config, id_token, &ctx)
            .map_err(failed)?;
        let call = if id_token { "idToken" } else { "token" };
        for quote in ['"', '\''] {
            own.insert(
                format!("$auth.{call}({quote}{id}{quote})"),
                http_requests::vars::Value { text: token.clone(), secret: true },
            );
        }
    }
    Ok(())
}

/// Which element of a collection a run is for (`request.iteration()`), and the variable
/// standing for it.
#[derive(Debug, Default)]
struct Iteration {
    index: usize,
    value: Option<(String, String)>,
}

/// The most requests one collection's elements make (the rest aren't sent).
const MOST_ROUNDS: usize = 1_000;

/// The first variable `request` uses whose value is a collection (a JSON array: an
/// environment's list, `{{users[*].name}}`), and its first [`MOST_ROUNDS`] elements as text.
fn collection(
    environments: &http_requests::env::Environments,
    request: &Request,
    variables: &[http_requests::FileVariable],
    environment: Option<&str>,
) -> Option<(String, Vec<String>)> {
    let chosen = Environment { environments, chosen: environment };
    let file_variables = FileVariables(variables);
    let lookup = Layers::new().then(&chosen).then(&file_variables);
    let mut texts: Vec<&str> = vec![&request.url];
    texts.extend(request.headers.iter().map(|h| h.value.as_str()));
    texts.extend(request.body.iter().filter_map(|p| match p {
        BodyPart::Text(t) => Some(t.as_str()),
        BodyPart::File(_) => None,
    }));
    for text in texts {
        for (_, name) in http_requests::vars::references(text) {
            if name.starts_with('$') || name.contains(".response.") {
                continue;
            }
            let Some(value) = http_requests::vars::Lookup::lookup(&lookup, name) else { continue };
            let Ok(serde_json::Value::Array(items)) = serde_json::from_str(value.text.trim())
            else {
                continue;
            };
            if items.is_empty() {
                continue;
            }
            return Some((
                name.to_owned(),
                items.iter().take(MOST_ROUNDS).map(http_requests::jsonpath::as_text).collect(),
            ));
        }
    }
    None
}

/// The responses of the file's requests, as variables: `name.response.body.<path>` (JSONPath;
/// `*` the whole body), `name.response.headers.<Name>`, `name.response.status` — from the
/// newest response of the request named so.
struct Responses<'a> {
    runs: &'a Runs,
    file: &'a Path,
}

impl http_requests::vars::Lookup for Responses<'_> {
    fn lookup(&self, name: &str) -> Option<http_requests::vars::Value> {
        use http_requests::chain::{ResponsePart, ResponseRef};
        let reference = ResponseRef::parse(name)?;
        let outcome = self.runs.latest(self.file, &reference.request)?;
        let response = &outcome.response;
        let text = match reference.part {
            ResponsePart::Status => response.status.to_string(),
            ResponsePart::Header(header) => response.header(&header)?.to_owned(),
            ResponsePart::Body(path) if path == "*" => {
                String::from_utf8_lossy(&crate::runs::body_bytes(&response.body)).into_owned()
            }
            // XPath over an XML body (`….body./a/b`, `….body.//token`).
            ResponsePart::Body(path) if path.starts_with('/') => {
                let bytes = crate::runs::body_bytes(&response.body);
                let found =
                    http_requests::xpath::select(&String::from_utf8_lossy(&bytes), &path).ok()?;
                match found.as_slice() {
                    [] => return None,
                    [one] => one.clone(),
                    many => serde_json::Value::from(many.to_vec()).to_string(),
                }
            }
            ResponsePart::Body(path) => {
                // Read once on its run's worker; a larger body read now (this is a worker too).
                let json = match &outcome.json {
                    Some(json) => Arc::clone(json),
                    None => Arc::new(
                        serde_json::from_slice(&crate::runs::body_bytes(&response.body)).ok()?,
                    ),
                };
                let found = http_requests::jsonpath::select(&json, &path).ok()?;
                http_requests::jsonpath::as_text(found.first()?)
            }
        };
        Some(http_requests::vars::Value { text, secret: outcome.request.secret })
    }
}

/// What one request of a chain is run with: the runs it's noted in, the environments read
/// for the chain, its file and its file's variables, the environment chosen, its cancel.
#[derive(Clone, Copy)]
struct Step<'a> {
    runs: &'a Runs,
    environments: &'a http_requests::env::Environments,
    file: &'a Path,
    request: &'a Request,
    variables: &'a [http_requests::FileVariable],
    environment: Option<&'a str>,
    cancel: &'a Cancel,
}

/// `step` sent once, for `round` (an element of a collection, or none), its body's pieces
/// told to `live` as they come.
fn execute(step: Step<'_>, round: &Iteration, live: &dyn Fn(&[u8])) -> RunState {
    let Step { runs, environments, file, request, variables, environment, cancel } = step;
    let (file_variables, chosen) =
        (FileVariables(variables), Environment { environments, chosen: environment });
    let responses = Responses { runs, file };
    // Its script before it (`< {% … %}`): this request's variables, the globals it keeps.
    let engine = || {
        http_scripts::QuickJs::with_environment(
            environments
                .variables(environment)
                .into_iter()
                .map(|(name, v)| (name.to_owned(), v.value.clone()))
                .collect(),
        )
        .with_file(variables.iter().map(|v| (v.name.clone(), v.value.clone())).collect())
    };
    let mut script: Option<ScriptRun> = None;
    if let Some(pre) = &request.pre_script {
        let before = runs.globals();
        let mut run =
            ScriptRun { globals: before.clone(), iteration: round.index, ..ScriptRun::default() };
        match script_source(file, pre) {
            Ok(source) => {
                engine().run(&source, Phase::Before, &script_request(request), None, &mut run)
            }
            Err(why) => run.error = Some(why),
        }
        runs.apply_globals(&before, &run.globals);
        runs.apply_headers(&run.headers);
        // A script that fails stops its request.
        if let Some(why) = &run.error {
            return RunState::Failed(i18n::tr!("http.script_failed", why = why.as_str()));
        }
        script = Some(run);
    }
    let plain = |map: &std::collections::BTreeMap<String, String>| {
        map.iter()
            .map(|(k, v)| (k.clone(), http_requests::vars::Value::plain(v.clone())))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let mut own = script.as_ref().map(|s| plain(&s.variables)).unwrap_or_default();
    // `{{$auth.token("id")}}`: the configuration's token had first (on this worker).
    if let Err(why) = oauth_tokens(step, &mut own) {
        return RunState::Failed(why);
    }
    // This round's element of a collection, over everything else.
    if let Some((name, value)) = &round.value {
        own.insert(name.clone(), http_requests::vars::Value::plain(value.clone()));
    }
    let globals = plain(&runs.globals());
    // Looked up in order: what its script set, the responses, the scripts' globals, the
    // environment's, then the file's own (`@name = …`).
    let layers = Layers::new()
        .then(&own)
        .then(&responses)
        .then(&globals)
        .then(&chosen)
        .then(&file_variables);
    // Made ready to send (Basic credentials encoded, GraphQL as a POST).
    let mut system =
        System { root: runs.root(), history: Some(own_folder(runs).join("responses")) };
    let resolved = match resolve_request(request, &layers, &mut system) {
        Ok(resolved) => {
            http_requests::prepare::prepare_with(resolved, !request.options.no_auto_encoding)
        }
        Err(missing) => {
            return RunState::Failed(i18n::tr!(
                "http.missing",
                names = missing.0.join(", ").as_str()
            ));
        }
    };
    let dir = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let body = resolved
        .body
        .iter()
        .map(|part| match part {
            BodyPart::Text(text) => OutgoingPart::Bytes(text.clone().into_bytes()),
            BodyPart::File(path) => OutgoingPart::File(dir.join(path)),
        })
        .collect();
    // The scripts' global headers, after the request's own.
    let mut headers = resolved.headers.clone();
    for (name, value) in runs.global_headers() {
        if !headers.iter().any(|h| h.name.eq_ignore_ascii_case(&name)) {
            headers.push(http_requests::Header { name, value });
        }
    }
    let outgoing =
        Outgoing { method: resolved.method.clone(), url: resolved.url.clone(), headers, body };
    let ssl = environments.ssl(environment);
    let folder = own_folder(runs);
    // Made before anything goes in it (a large body is read into it as it comes).
    ready_folder(&folder);
    let mut options = SendOptions {
        follow_redirects: !request.options.no_redirect,
        timeout: request.options.timeout.or(SendOptions::default().timeout),
        connect_timeout: request
            .options
            .connection_timeout
            .or(SendOptions::default().connect_timeout),
        spill_dir: Some(folder.join("bodies")),
        // The environment's `SSLConfiguration`: the server's certificate checked or not (the
        // client's below).
        verify_tls: ssl.verify_host,
        // `HTTP/2`, `HTTP/2 (Prior Knowledge)` on the request line.
        http2: http_requests::transport::Http2::written(request.version.as_deref()),
        ..SendOptions::default()
    };
    // The client's certificate, read here (on the run's worker) beside the environment's
    // file; asked for but not read: not sent without it (the server
    // would answer as to anyone, or refuse with a reason less clear).
    if let Some(cert) = &ssl.client_certificate {
        let dir = http_environments::folder_for(file, runs.root().as_deref());
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"));
        let certificate = read(cert).and_then(|chain| {
            let key = match &ssl.client_key {
                Some(key) => read(key)?,
                None => chain.clone(),
            };
            Ok(http_requests::transport::ClientCertificate { chain, key })
        });
        match certificate {
            Ok(certificate) => options.client_certificate = Some(certificate),
            Err(why) => return RunState::Failed(i18n::tr!("http.client_cert", why = why.as_str())),
        }
    }
    let use_cookies = !request.options.no_cookie_jar;
    let before = use_cookies.then(|| cookies(runs, &folder));
    let mut jar = before.clone();
    // A WebSocket: its messages sent, what comes back told as it comes, until it closes or
    // is stopped.
    if resolved.method.eq_ignore_ascii_case("WEBSOCKET") {
        return websocket_run(resolved, file, &options, live, cancel);
    }
    // Its body told as it comes (a stream of events shown while it lasts).
    // A profile recording counts the requests sent: how many, never what or where.
    kernel::probe::count("http-client", "requests", 1);
    let sent = {
        // Its time on this worker (in the flame graph's ways down, never the interface's).
        let _timed = kernel::probe::time("http-client", "request");
        transport().send_streaming(&outgoing, &options, jar.as_mut(), cancel, live)
    };
    // What went and came, once it went (a refused connection sent nothing); the body as
    // read (a compressed one counts as unpacked: an estimate of the traffic).
    if let Ok(response) = &sent {
        runs.traffic.sent(outgoing_bytes(&outgoing));
        runs.traffic.received(incoming_bytes(response));
    }
    if let (Some(before), Some(after)) = (before, jar) {
        keep_cookies(runs, &folder, &before, &after);
    }
    match sent {
        Ok(mut response) => {
            let (saved, kept) = if request.options.no_log {
                // Not saved, but a large body is still read into a file (to show): only the
                // newest of those kept. One held in memory past [`NOT_LOGGED_KEPT`] gets a
                // private copy beside them, which the memory budget can leave it in.
                let kept = match &response.body {
                    IncomingBody::Memory(bytes) if bytes.len() > NOT_LOGGED_KEPT => {
                        keep_private(&folder.join("bodies"), bytes)
                    }
                    _ => None,
                };
                if matches!(response.body, IncomingBody::File { .. }) || kept.is_some() {
                    keep_newest(&folder.join("bodies"));
                }
                (None, kept)
            } else {
                (save(&folder, &mut response), None)
            };
            // `>> file` / `>>! file`: its body written there too (in the project only).
            let mut wrote = None;
            if let Some(save_to) = &request.save_to
                && let Ok(path) = http_requests::vars::resolve(&save_to.path, &layers, &mut system)
            {
                let dir = file.parent().unwrap_or(Path::new("."));
                let within = runs.root().unwrap_or_else(|| dir.to_path_buf());
                wrote = Some(match confined(&dir.join(path.text.trim()), &within) {
                    Some(to) => write_response_to(&response, &to, save_to.replace),
                    None => Err(i18n::tr!("http.save_outside", path = path.text.trim())),
                });
            }
            let json = Outcome::json_of(&response);
            // Its script after it (`> {% … %}`, `> ./handler.js`): its tests, its logs.
            if let Some(post) = &request.post_script {
                let mut run = script.take().unwrap_or_default();
                let before = runs.globals();
                run.globals = before.clone();
                run.iteration = round.index;
                run.headers.clear();
                match script_source(file, post) {
                    Ok(source) => engine().run(
                        &source,
                        Phase::After,
                        &script_request(request),
                        Some(&script_response(&response)),
                        &mut run,
                    ),
                    Err(why) => run.error = Some(why),
                }
                runs.apply_globals(&before, &run.globals);
                runs.apply_headers(&run.headers);
                script = Some(run);
            }
            // `>>`'s failure told with what its scripts tell.
            if let Some(Err(why)) = wrote {
                script.get_or_insert_with(Default::default).logs.push(why);
            }
            RunState::Done(Arc::new(Outcome {
                request: resolved,
                response,
                saved,
                kept,
                json,
                script,
            }))
        }
        Err(TransportError::Cancelled) => RunState::Failed(i18n::t("http.cancelled")),
        Err(e) => RunState::Failed(e.to_string()),
    }
}

/// How much a request sends: its line, its headers, its body (a file part by its size).
fn outgoing_bytes(outgoing: &http_requests::transport::Outgoing) -> u64 {
    let head = outgoing.method.len() + outgoing.url.len() + 12;
    let headers: usize = outgoing.headers.iter().map(|h| h.name.len() + h.value.len() + 4).sum();
    let body: u64 = outgoing
        .body
        .iter()
        .map(|part| match part {
            http_requests::transport::OutgoingPart::Bytes(b) => b.len() as u64,
            http_requests::transport::OutgoingPart::File(path) => {
                std::fs::metadata(path).map_or(0, |m| m.len())
            }
        })
        .sum();
    (head + headers) as u64 + body
}

/// How much a response brought: its status line, its headers, its body.
fn incoming_bytes(response: &http_requests::transport::Incoming) -> u64 {
    let headers: usize = response.headers.iter().map(|h| h.name.len() + h.value.len() + 4).sum();
    (response.reason.len() + 16 + headers) as u64 + response.body.len()
}

/// A WebSocket request run: its session's transcript as its response (`→` sent, `←`
/// received).
fn websocket_run(
    resolved: http_requests::vars::ResolvedRequest,
    file: &Path,
    options: &SendOptions,
    live: &dyn Fn(&[u8]),
    cancel: &Cancel,
) -> RunState {
    let dir = file.parent().unwrap_or(Path::new("."));
    let body: String = resolved
        .body
        .iter()
        .map(|p| match p {
            BodyPart::Text(t) => t.clone(),
            BodyPart::File(path) => std::fs::read_to_string(dir.join(path)).unwrap_or_default(),
        })
        .collect();
    let messages = http_requests::transport::ws_messages(&body);
    let line =
        |received: bool, text: &str| format!("{} {text}\n", if received { "←" } else { "→" });
    let started = Instant::now();
    let event = |received: bool, text: &str| live(line(received, text).as_bytes());
    let (url, headers) = (&resolved.url, &resolved.headers);
    kernel::probe::count("http-client", "websockets", 1);
    match transport().websocket(url, headers, &messages, options, cancel, &event) {
        Ok(transcript) => {
            let text: String = transcript.iter().map(|(r, t)| line(*r, t)).collect();
            let response = http_requests::transport::Incoming {
                status: 101,
                reason: "Switching Protocols".into(),
                version: "WebSocket".into(),
                headers: Vec::new(),
                body: IncomingBody::Memory(text.into_bytes()),
                url: resolved.url.clone(),
                hops: Vec::new(),
                timings: http_requests::transport::Timings {
                    total: started.elapsed(),
                    ..Default::default()
                },
            };
            RunState::Done(Arc::new(Outcome {
                request: resolved,
                response,
                saved: None,
                kept: None,
                json: None,
                script: None,
            }))
        }
        Err(TransportError::Cancelled) => RunState::Failed(i18n::t("http.cancelled")),
        Err(e) => RunState::Failed(e.to_string()),
    }
}

/// `response`'s body written to `path` (`replace`: over what's there; else beside it, under
/// a free name: `out-1.json`, `out-2.json`…, as the format's `>>`).
fn write_response_to(
    response: &http_requests::transport::Incoming,
    path: &Path,
    replace: bool,
) -> Result<(), String> {
    let path = if replace || !path.exists() {
        path.to_path_buf()
    } else {
        let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        (1..10_000)
            .map(|n| path.with_file_name(format!("{stem}-{n}{ext}")))
            .find(|p| !p.exists())
            .unwrap_or_else(|| path.to_path_buf())
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match &response.body {
        IncomingBody::Memory(bytes) => files::write_atomic(&path, bytes),
        IncomingBody::File { path: from, .. } => std::fs::copy(from, &path).map(|_| ()),
    }
    .map_err(|e| format!("{}: {e}", path.display()))
}

/// `path` with its `.` and `..` worked out, if it stays inside `within` (else `None`: a
/// `>>` never writes outside the project). A link inside pointing out isn't followed here:
/// the file's own folder is checked once it exists.
pub(crate) fn confined(path: &Path, within: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    if !out.starts_with(within) {
        return None;
    }
    // Its folder, as far as it exists, followed through links: still inside.
    let real_within = within.canonicalize().unwrap_or_else(|_| within.to_path_buf());
    let existing = out.ancestors().find(|a| a.exists())?;
    let real = existing.canonicalize().ok()?;
    real.starts_with(&real_within).then_some(out)
}

/// A script's text: written in the file, or the file it names (beside the request's).
fn script_source(file: &Path, script: &http_requests::Script) -> Result<String, String> {
    match script {
        http_requests::Script::Inline(source) => Ok(source.clone()),
        http_requests::Script::File(path) => {
            let dir = file.parent().unwrap_or(Path::new("."));
            std::fs::read_to_string(dir.join(path)).map_err(|e| format!("{path}: {e}"))
        }
    }
}

/// The request as its scripts see it: as written.
fn script_request(request: &Request) -> ScriptRequest {
    let body = request
        .body
        .iter()
        .map(|part| match part {
            BodyPart::Text(text) => text.clone(),
            BodyPart::File(path) => format!("< {path}"),
        })
        .collect();
    ScriptRequest {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request.headers.clone(),
        body,
    }
}

/// The most of a body a script is given.
const SCRIPT_BODY: usize = 8 * 1024 * 1024;

/// The response as its scripts see it (its body as text, up to [`SCRIPT_BODY`]).
fn script_response(response: &http_requests::transport::Incoming) -> ScriptResponse {
    let body = match &response.body {
        IncomingBody::Memory(bytes) => {
            String::from_utf8_lossy(&bytes[..bytes.len().min(SCRIPT_BODY)]).into_owned()
        }
        // A large one in its file: its start read.
        IncomingBody::File { path, .. } => {
            use std::io::Read;
            let mut head = Vec::new();
            if let Ok(file) = std::fs::File::open(path) {
                let _ = file.take(SCRIPT_BODY as u64).read_to_end(&mut head);
            }
            String::from_utf8_lossy(&head).into_owned()
        }
    };
    let charset = response.header("content-type").and_then(|t| {
        t.split(';').find_map(|p| {
            let (key, value) = p.trim().split_once('=')?;
            key.trim().eq_ignore_ascii_case("charset").then(|| value.trim_matches('"').to_owned())
        })
    });
    ScriptResponse {
        status: response.status,
        headers: response.headers.clone(),
        body,
        content_type: response.media_type(),
        charset,
    }
}

/// What a run changed in the project's cookies applied to the ones kept (another run may
/// have changed them meanwhile: its changes stay), written to their file only if they changed
/// — under the lock, so two runs' writes don't cross.
fn keep_cookies(runs: &Runs, folder: &Path, before: &CookieJar, after: &CookieJar) {
    let mut kept = runs.cookies.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let jar = kept.get_or_insert_with(|| before.clone());
    if jar.apply_changes(before, after) {
        // Readable by the user only: cookies are sessions.
        let _ = files::write_private(&folder.join("cookies"), jar.to_text().as_bytes());
    }
}

/// The project's cookies: as kept in memory, else read from its file.
fn cookies(runs: &Runs, folder: &Path) -> CookieJar {
    let kept = runs.cookies.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    kept.unwrap_or_else(|| {
        let text = std::fs::read_to_string(folder.join("cookies")).unwrap_or_default();
        CookieJar::from_text(&text, System::default().now())
    })
}

/// The response's body written for the editor (its extension its type's), the oldest past
/// [`KEPT`] removed. `None`: it couldn't be.
fn save(folder: &Path, response: &mut http_requests::transport::Incoming) -> Option<PathBuf> {
    let dir = folder.join("responses");
    std::fs::create_dir_all(&dir).ok()?;
    let extension = crate::format::extension(response.media_type().as_deref());
    let stamp = http_requests::vars::iso_timestamp(System::default().now()).replace([':', '-'], "");
    // A few characters of a fresh id after the time: runs in the same second keep theirs.
    let unique = format!("{:08x}", kernel::id::new_uuid() as u32);
    let path = dir.join(format!(
        "{}.{unique}.{}.{extension}",
        stamp.trim_end_matches('Z'),
        response.status
    ));
    match &mut response.body {
        IncomingBody::Memory(bytes) => files::write_atomic(&path, bytes).ok()?,
        // Too large to hold: the file it was read into moved there (and the response says so).
        IncomingBody::File { path: spilled, .. } => {
            std::fs::rename(&*spilled, &path).ok()?;
            spilled.clone_from(&path);
        }
    }
    keep_newest(&dir);
    Some(path)
}

/// The largest `# @no-log` body held in memory only (a larger one gets a private copy).
const NOT_LOGGED_KEPT: usize = 1024 * 1024;

/// A body's private copy in `dir` (the module's own folder, never the history).
fn keep_private(dir: &Path, bytes: &[u8]) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("{}.body", kernel::id::uuid_text(kernel::id::new_uuid())));
    files::write_atomic(&path, bytes).ok()?;
    Some(path)
}

/// Only the newest [`KEPT`] files of `dir` kept (by when they were written).
pub(crate) fn keep_newest(dir: &Path) {
    let mut kept: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH), e.path()))
                .collect()
        })
        .unwrap_or_default();
    kept.sort();
    let extra = kept.len().saturating_sub(KEPT);
    for (_, old) in &kept[..extra] {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
#[path = "../tests/unit/job.rs"]
mod tests;
