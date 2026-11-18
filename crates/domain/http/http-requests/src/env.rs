//! A request's environments, kept beside the `.http` files: a JSON object
//! with one object per environment (its variables) and `$shared` (every environment's):
//!
//! - `http-client.env.json`: the public one, committed (hosts, ports);
//! - `http-client.private.env.json`: the user's (tokens, passwords), never committed;
//! - `http-client.local.env.json`: the user's too, for this machine (ours: what `.local`
//!   means elsewhere in a project).
//!
//! A variable is the environment's own before `$shared`'s; the private file's before the
//! public's, the local one's before both. An object's members are variables by their path
//! (`{"db": {"host": "x"}}`: `db.host`), an array's by their index (`hosts[0]`); the
//! environment's own settings (`Security`) aren't variables.

use std::collections::BTreeMap;

/// The public file's name.
pub const PUBLIC: &str = "http-client.env.json";
/// The user's own file's name (never committed).
pub const PRIVATE: &str = "http-client.private.env.json";
/// This machine's file's name (never committed).
pub const LOCAL: &str = "http-client.local.env.json";
/// What every environment holds.
pub const SHARED: &str = "$shared";

/// Where a variable comes from (a private one is shown hidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Public,
    Private,
    Local,
}

impl Source {
    /// The user's own (its values aren't shown, logged nor kept in a shared history).
    pub fn is_secret(self) -> bool {
        !matches!(self, Self::Public)
    }

    /// The file it's kept in.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Public => PUBLIC,
            Self::Private => PRIVATE,
            Self::Local => LOCAL,
        }
    }
}

/// `name` is one of the user's own files (never committed).
pub fn is_private_file(name: &str) -> bool {
    name == PRIVATE || name == LOCAL
}

/// A variable of an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub value: String,
    pub source: Source,
}

/// What an environment may hold that isn't a variable (its SSL and authentication
/// settings).
const RESERVED: &[&str] = &["Security", "SSLConfiguration"];

/// `value` named `name` into `set`: text, a number, a truth as they are; an object's members
/// as `name.member` (as deep as they go); an array as `name[0]`, `name[1]`…
fn flatten(
    name: &str,
    value: serde_json::Value,
    source: Source,
    set: &mut BTreeMap<String, Variable>,
) {
    let text = match value {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => return,
        serde_json::Value::Object(members) => {
            for (member, value) in members {
                flatten(&format!("{name}.{member}"), value, source, set);
            }
            return;
        }
        serde_json::Value::Array(items) => {
            for (i, value) in items.into_iter().enumerate() {
                flatten(&format!("{name}[{i}]"), value, source, set);
            }
            return;
        }
    };
    set.insert(name.to_owned(), Variable { value: text, source });
}

/// An environment's values merged, as its variables' JSONPaths read them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Root {
    value: serde_json::Value,
    secret: BTreeMap<String, bool>,
}

/// An environment's TLS settings (`SSLConfiguration`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ssl {
    /// The server's certificate checked (`verifyHostCertificate`, on unless said off).
    pub verify_host: bool,
    /// The client's certificate and its key (paths beside the environment's file; the key
    /// may be in the certificate's file).
    pub client_certificate: Option<String>,
    pub client_key: Option<String>,
}

impl Default for Ssl {
    fn default() -> Self {
        Self { verify_host: true, client_certificate: None, client_key: None }
    }
}

/// Every environment of a folder's files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environments {
    /// Environment (or [`SHARED`]) → variable → its value, from every file (the strongest
    /// source kept).
    sets: BTreeMap<String, BTreeMap<String, Variable>>,
    /// Environment (or [`SHARED`]) → its values as written (JSON), each with its source: what
    /// a variable's JSONPath reads.
    values: BTreeMap<String, BTreeMap<String, (serde_json::Value, Source)>>,
    /// Per environment (`None`: none chosen), its values over `$shared`'s as one JSON object
    /// and whether each is a secret: what a JSONPath reads, made once when the files are read
    /// (not for each variable looked up).
    roots: BTreeMap<Option<String>, Root>,
    /// Environment (or [`SHARED`]) → its `SSLConfiguration` and `Security` (as written).
    settings: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// What couldn't be read: the file and why.
    pub problems: Vec<(Source, String)>,
}

impl Environments {
    /// The files' texts read (`None`: no such file), the stronger source after.
    pub fn read(files: &[(Source, Option<&str>)]) -> Self {
        let mut out = Self::default();
        let mut files: Vec<&(Source, Option<&str>)> = files.iter().collect();
        files.sort_by_key(|(source, _)| *source);
        for (source, text) in files {
            let Some(text) = text else { continue };
            if let Err(why) = out.add(*source, text) {
                out.problems.push((*source, why));
            }
        }
        let environments: Vec<Option<String>> =
            std::iter::once(None).chain(out.names().map(|n| Some(n.to_owned()))).collect();
        for environment in environments {
            let root = out.merged(environment.as_deref());
            out.roots.insert(environment, root);
        }
        out
    }

    /// `environment`'s values over `$shared`'s, as one object.
    fn merged(&self, environment: Option<&str>) -> Root {
        let mut value = serde_json::Map::new();
        let mut secret = BTreeMap::new();
        for set in [self.values.get(SHARED), environment.and_then(|e| self.values.get(e))]
            .into_iter()
            .flatten()
        {
            for (key, (v, source)) in set {
                value.insert(key.clone(), v.clone());
                secret.insert(key.clone(), source.is_secret());
            }
        }
        Root { value: serde_json::Value::Object(value), secret }
    }

    fn add(&mut self, source: Source, text: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let serde_json::Value::Object(environments) = value else {
            return Err("not an object of environments".to_owned());
        };
        for (environment, variables) in environments {
            let serde_json::Value::Object(variables) = variables else { continue };
            let set = self.sets.entry(environment.clone()).or_default();
            let values = self.values.entry(environment.clone()).or_default();
            for (name, value) in variables {
                // The environment's own settings, not variables.
                if RESERVED.contains(&name.as_str()) {
                    self.settings.entry(environment.clone()).or_default().insert(name, value);
                    continue;
                }
                values.insert(name.clone(), (value.clone(), source));
                flatten(&name, value, source, set);
            }
        }
        Ok(())
    }

    /// `environment`'s TLS settings (its own over `$shared`'s).
    pub fn ssl(&self, environment: Option<&str>) -> Ssl {
        let config = environment
            .and_then(|e| self.settings.get(e)?.get("SSLConfiguration"))
            .or_else(|| self.settings.get(SHARED)?.get("SSLConfiguration"));
        let Some(config) = config else { return Ssl::default() };
        let text = |key: &str| config.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        Ssl {
            verify_host: config
                .get("verifyHostCertificate")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            client_certificate: text("clientCertificate"),
            client_key: text("clientCertificateKey"),
        }
    }

    /// `environment`'s `Security` settings, as written (its own over `$shared`'s).
    pub fn security(&self, environment: Option<&str>) -> Option<&serde_json::Value> {
        environment
            .and_then(|e| self.settings.get(e)?.get("Security"))
            .or_else(|| self.settings.get(SHARED)?.get("Security"))
    }

    /// The environments' names, in order (not [`SHARED`]).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sets.keys().map(String::as_str).filter(|n| *n != SHARED)
    }

    /// `name`'s variable in `environment` (`None`: none chosen — `$shared` only).
    pub fn variable(&self, environment: Option<&str>, name: &str) -> Option<&Variable> {
        environment
            .and_then(|e| self.sets.get(e)?.get(name))
            .or_else(|| self.sets.get(SHARED)?.get(name))
    }

    /// Variable `name` in `environment`: a JSONPath expression over its
    /// values (a name it holds as it is, failing that: a key written with dots) —
    /// `client.host.url`, `client.['host.url']`, `users[0].name`, `$.clients..id` (`$`: the
    /// whole environment). Several values selected: their JSON array.
    pub fn resolve(&self, environment: Option<&str>, name: &str) -> Option<Variable> {
        self.by_path(environment, name).or_else(|| self.variable(environment, name).cloned())
    }

    /// [`Self::resolve`]'s JSONPath reading.
    fn by_path(&self, environment: Option<&str>, name: &str) -> Option<Variable> {
        // The environment's values over `$shared`'s (an unknown one: `$shared`'s only).
        let root =
            self.roots.get(&environment.map(str::to_owned)).or_else(|| self.roots.get(&None))?;
        let (base, path) = if let Some(rest) = name.strip_prefix('$') {
            (None, format!("${rest}"))
        } else {
            let end = name.find(['.', '[']).unwrap_or(name.len());
            let rest = name[end..].replacen(".[", "[", 1);
            (Some(&name[..end]), format!("${rest}"))
        };
        let (value, secret) = match base {
            Some(base) => (root.value.get(base)?, root.secret.get(base).copied().unwrap_or(false)),
            None => (&root.value, root.secret.values().any(|s| *s)),
        };
        let found = crate::jsonpath::select(value, &path).ok()?;
        let text = match found.as_slice() {
            [] => return None,
            [one] => crate::jsonpath::as_text(one),
            many => {
                serde_json::Value::Array(many.iter().map(|v| (*v).clone()).collect()).to_string()
            }
        };
        let source = if secret { Source::Private } else { Source::Public };
        Some(Variable { value: text, source })
    }

    /// Every variable of `environment` (`$shared`'s after its own), by name.
    pub fn variables(&self, environment: Option<&str>) -> BTreeMap<&str, &Variable> {
        let mut out = BTreeMap::new();
        for set in [self.sets.get(SHARED), environment.and_then(|e| self.sets.get(e))]
            .into_iter()
            .flatten()
        {
            for (name, variable) in set {
                out.insert(name.as_str(), variable);
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../tests/unit/env.rs"]
mod tests;
