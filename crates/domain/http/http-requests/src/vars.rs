//! `{{name}}` resolved in a request's text: from the places that hold variables, first one
//! first ([`Layers`]: the request's own, the file's `@name = …`, what scripts and chained
//! responses set, the environment chosen), a value holding `{{…}}` itself resolved in turn;
//! or one of the dynamic ones (`{{$uuid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`,
//! `{{$randomInt}}`, `{{$random.uuid}}`, `{{$random.integer(1, 10)}}`,
//! `{{$random.alphanumeric(8)}}`, `{{$random.alphabetic(8)}}`, `{{$random.hexadecimal(8)}}`,
//! `{{$random.float(0, 1)}}`, `{{$random.email}}`, `{{$env.NAME}}`), made by a [`Dynamic`] the caller gives (the clock and
//! the dice are the module's: the domain reads neither).
//! A name none has is said ([`Unresolved`]), never sent empty.

/// A variable's value, and whether it's the user's own (a secret: not shown, not logged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub secret: bool,
}

impl Value {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), secret: false }
    }
}

/// A place holding variables.
pub trait Lookup {
    fn lookup(&self, name: &str) -> Option<Value>;
}

/// An environment of [`crate::env::Environments`] as a place of variables.
#[derive(Debug, Clone, Copy)]
pub struct Environment<'a> {
    pub environments: &'a crate::env::Environments,
    pub chosen: Option<&'a str>,
}

impl Lookup for Environment<'_> {
    fn lookup(&self, name: &str) -> Option<Value> {
        let variable = self.environments.resolve(self.chosen, name)?;
        Some(Value { text: variable.value.clone(), secret: variable.source.is_secret() })
    }
}

/// A file's `@name = value` variables (the last one written wins).
#[derive(Debug, Clone, Copy)]
pub struct FileVariables<'a>(pub &'a [crate::model::FileVariable]);

impl Lookup for FileVariables<'_> {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.0.iter().rev().find(|v| v.name == name).map(|v| Value::plain(v.value.clone()))
    }
}

/// Variables by name (a script's, a test's).
impl Lookup for std::collections::BTreeMap<String, Value> {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.get(name).cloned()
    }
}

/// Where variables are looked for, in order.
#[derive(Default)]
pub struct Layers<'a> {
    layers: Vec<&'a dyn Lookup>,
}

impl std::fmt::Debug for Layers<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layers").field("layers", &self.layers.len()).finish()
    }
}

impl<'a> Layers<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Looked in after the ones before.
    #[must_use]
    pub fn then(mut self, layer: &'a dyn Lookup) -> Self {
        self.layers.push(layer);
        self
    }
}

impl Lookup for Layers<'_> {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.layers.iter().find_map(|layer| layer.lookup(name))
    }
}

/// What the dynamic variables are made from.
pub trait Dynamic {
    /// Seconds since the Unix epoch.
    fn now(&self) -> u64;
    /// A random UUID (v4), as text.
    fn uuid(&mut self) -> String;
    /// A random number.
    fn random(&mut self) -> u64;
    /// The system's environment variable `name` (`{{$env.NAME}}`; `None`: none, or not
    /// told).
    fn env(&self, name: &str) -> Option<String> {
        let _ = name;
        None
    }
    /// `{{$projectRoot}}`: the project's folder (`None`: a file alone).
    fn project_root(&self) -> Option<String> {
        None
    }
    /// `{{$historyFolder}}`: where the responses are kept.
    fn history_folder(&self) -> Option<String> {
        None
    }
}

/// A text resolved: its variables in place, and whether a secret went into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub text: String,
    pub secret: bool,
}

/// The names nothing had (each once, in the order met).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved(pub Vec<String>);

/// The secrets that went into a text: whether any did, and their values as they went in
/// (what a view hides wherever they show: the URL, a header, the body).
#[derive(Debug, Default)]
struct Secrets {
    any: bool,
    values: Vec<String>,
}

/// How deep values holding `{{…}}` are followed (a loop stops there).
const DEPTH: usize = 8;

/// `text` with its `{{name}}` resolved from `lookup` (and `dynamic`).
pub fn resolve(
    text: &str,
    lookup: &dyn Lookup,
    dynamic: &mut dyn Dynamic,
) -> Result<Resolved, Unresolved> {
    let mut missing = Vec::new();
    let mut secrets = Secrets::default();
    let text = expand(text, lookup, dynamic, &mut missing, &mut secrets, 0);
    if missing.is_empty() {
        Ok(Resolved { text, secret: secrets.any })
    } else {
        Err(Unresolved(missing))
    }
}

/// The `{{…}}` names written in `text` (where each starts, its name), for completion and
/// hover.
pub fn references(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{").map(|at| from + at) {
        let Some(close) = text[open + 2..].find("}}").map(|at| open + 2 + at) else { break };
        out.push((open, text[open + 2..close].trim()));
        from = close + 2;
    }
    out
}

fn expand(
    text: &str,
    lookup: &dyn Lookup,
    dynamic: &mut dyn Dynamic,
    missing: &mut Vec<String>,
    secrets: &mut Secrets,
    depth: usize,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}").map(|at| open + 2 + at) else { break };
        out.push_str(&rest[..open]);
        let name = rest[open + 2..close].trim();
        match value_of(name, lookup, dynamic) {
            Some(value) if depth < DEPTH => {
                let text = expand(&value.text, lookup, dynamic, missing, secrets, depth + 1);
                if value.secret {
                    secrets.any = true;
                    if !text.is_empty() && !secrets.values.contains(&text) {
                        secrets.values.push(text.clone());
                    }
                }
                out.push_str(&text);
            }
            Some(value) => {
                secrets.any |= value.secret;
                out.push_str(&value.text);
            }
            None => {
                if !missing.iter().any(|m| m == name) {
                    missing.push(name.to_owned());
                }
                out.push_str(&rest[open..close + 2]);
            }
        }
        rest = &rest[close + 2..];
    }
    out.push_str(rest);
    out
}

fn value_of(name: &str, lookup: &dyn Lookup, dynamic: &mut dyn Dynamic) -> Option<Value> {
    match name.strip_prefix('$') {
        // A dynamic one (`$uuid`), else a JSONPath over the environment (`$.clients..id`).
        Some(dynamic_name) => {
            dynamic_value(dynamic_name, dynamic).map(Value::plain).or_else(|| lookup.lookup(name))
        }
        None => lookup.lookup(name),
    }
}

/// A dynamic variable's value (`None`: no such one).
fn dynamic_value(name: &str, dynamic: &mut dyn Dynamic) -> Option<String> {
    let (call, args) = match name.split_once('(') {
        Some((call, args)) => (call.trim(), args.trim_end_matches(')').trim()),
        None => (name.trim(), ""),
    };
    let numbers: Vec<i64> = args.split(',').filter_map(|a| a.trim().parse().ok()).collect();
    match call {
        "uuid" | "random.uuid" => Some(dynamic.uuid()),
        "timestamp" => Some(dynamic.now().to_string()),
        "isoTimestamp" => Some(iso_timestamp(dynamic.now())),
        "randomInt" => Some((dynamic.random() % 1000).to_string()),
        "random.integer" => {
            let (low, high) = match numbers.as_slice() {
                [low, high] if high > low => (*low, *high),
                _ => (0, 1000),
            };
            // In `i128`: the whole of `i64` is a span too.
            let span = (i128::from(high) - i128::from(low)) as u128;
            let pick = i128::from(low) + (u128::from(dynamic.random()) % span) as i128;
            Some(pick.to_string())
        }
        "random.alphanumeric" => Some(random_text(dynamic, &numbers, ALPHA_NUMERIC)),
        "random.alphabetic" => Some(random_text(dynamic, &numbers, &ALPHA_NUMERIC[..52])),
        "random.hexadecimal" => Some(random_text(dynamic, &numbers, b"0123456789abcdef")),
        "random.float" => {
            let floats: Vec<f64> = args.split(',').filter_map(|a| a.trim().parse().ok()).collect();
            let (low, high) = match floats.as_slice() {
                [low, high] if high > low => (*low, *high),
                _ => (0.0, 1000.0),
            };
            let unit = (dynamic.random() % 1_000_000) as f64 / 1_000_000.0;
            Some(format!("{:.4}", low + (high - low) * unit))
        }
        "random.email" => {
            let user = random_text(dynamic, &[8], &ALPHA_NUMERIC[26..52]);
            Some(format!("{user}@example.com"))
        }
        "projectRoot" => dynamic.project_root(),
        "historyFolder" => dynamic.history_folder(),
        // Faker, seeded: `random.locale.name.fullName("fr", 12)`.
        other if other.starts_with("random.locale.") => {
            let (class, method) = other["random.locale.".len()..].split_once('.')?;
            let seed = args.split(',').nth(1).and_then(|s| s.trim().parse::<u64>().ok());
            let mut next = crate::faker::seeded(seed.unwrap_or_else(|| dynamic.random()));
            crate::faker::fake(class, method, &mut next)
        }
        // Faker: `random.name.fullName`, `random.address.city`…
        other if other.starts_with("random.") => {
            let (class, method) = other["random.".len()..].split_once('.')?;
            crate::faker::fake(class, method, &mut || dynamic.random())
        }
        other => other.strip_prefix("env.").and_then(|name| dynamic.env(name)),
    }
}

/// A request ready to send: its variables resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<crate::model::Header>,
    /// Its body's pieces, resolved (a file's path too: read by who sends it).
    pub body: Vec<crate::model::BodyPart>,
    /// A secret went into it (the history keeps it hidden).
    pub secret: bool,
    /// The secrets' values as they went in (hidden wherever they're shown).
    pub secrets: Vec<String>,
}

impl ResolvedRequest {
    /// `text` (a part of it, as shown) with every secret that went into it hidden.
    pub fn masked(&self, text: &str) -> String {
        let mut out = text.to_owned();
        // A very short one would hide every such letter of the text: those stay (they're
        // hardly secret, and the headers that carry credentials are hidden whole anyway).
        for secret in self.secrets.iter().filter(|s| s.chars().count() >= 3) {
            out = out.replace(secret.as_str(), "••••••");
        }
        out
    }
}

/// `request` resolved from `lookup` (every name nothing has, together).
pub fn resolve_request(
    request: &crate::model::Request,
    lookup: &dyn Lookup,
    dynamic: &mut dyn Dynamic,
) -> Result<ResolvedRequest, Unresolved> {
    use crate::model::{BodyPart, Header};
    let mut missing = Vec::new();
    let mut secrets = Secrets::default();
    let mut one = |text: &str, dynamic: &mut dyn Dynamic| {
        expand(text, lookup, dynamic, &mut missing, &mut secrets, 0)
    };
    let url = one(&request.url, dynamic);
    let headers = request
        .headers
        .iter()
        .map(|h| Header { name: one(&h.name, dynamic), value: one(&h.value, dynamic) })
        .collect();
    let body = request
        .body
        .iter()
        .map(|part| match part {
            BodyPart::Text(text) => BodyPart::Text(one(text, dynamic)),
            BodyPart::File(path) => BodyPart::File(one(path, dynamic)),
        })
        .collect();
    if !missing.is_empty() {
        return Err(Unresolved(missing));
    }
    Ok(ResolvedRequest {
        method: request.method.clone(),
        url,
        headers,
        body,
        secret: secrets.any,
        secrets: secrets.values,
    })
}

/// Letters (upper case, lower case), digits, `_`: what random texts are made of.
const ALPHA_NUMERIC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_";

/// A random text of `numbers[0]` characters (8 when not given) from `chars`.
fn random_text(dynamic: &mut dyn Dynamic, numbers: &[i64], chars: &[u8]) -> String {
    let length = numbers.first().map_or(8, |n| (*n).clamp(1, 1024) as usize);
    (0..length).map(|_| chars[(dynamic.random() % chars.len() as u64) as usize] as char).collect()
}

/// `seconds` since the epoch as an ISO 8601 UTC time (`2026-11-11T12:00:00Z`).
pub fn iso_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
#[path = "../tests/unit/vars.rs"]
mod tests;
