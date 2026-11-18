//! A server described as data: a language module's `language-servers.toml` (or the user's,
//! in the config folder), one `[[language-servers]]` table per server —
//!
//! ```toml
//! [[language-servers]]
//! name = "vtsls"                      # shown, and what it's known by
//! languages = ["typescript", "tsx"]   # the languages (their ids) it serves
//! bundle = "vtsls"                    # its folder in the app's `lsp/servers/` (`${server}`)
//! command = "${node}"                 # relative to `lsp/`, absolute, or a program in PATH
//! args = ["${server}/node_modules/@vtsls/language-server/bin/vtsls.js", "--stdio"]
//! env = { NODE_ENV = "production" }
//! root-markers = ["tsconfig.json", "package.json"]
//! file-patterns = ["**/*.{ts,tsx}", "**/tsconfig.json"]   # disk changes it hears
//! language-ids = { tsx = "typescriptreact" }               # LSP's ids, when not ours
//! extension-language-ids = { jsx = "javascriptreact" }
//! file-language-ids = { "tsconfig*.json" = "jsonc" }       # by the file's name (a glob)
//! fallback = false                     # true: only when the one before can't start
//! built-in = ["deprecated", "rename"]  # what the built-in engine keeps answering
//! contents = { "https://json.schemastore.org/" = "${server}/schemas/schemastore" }
//! completion-kinds = { property = "attribute" }  # LSP's kind (by name) shown as ours
//! on-initialized = ["yaml/registerContentRequest"]  # notifications sent once it's ready
//! default-engine = "built-in"          # what answers its languages by default (`server`)
//! transport = "tcp"                    # stdio (default), or a local port (`${port}`)
//! secret-options = { licenceKey = "intelephense-licence" }  # options from the keychain
//! show-messages = true                 # its `window/showMessage` as notifications
//! [language-servers.install]           # not bundled: installed by the user (see below)
//! npm = "intelephense"
//! version = "1.18.5"
//! integrity = "sha512-…"
//! [language-servers.initialization-options]
//! [language-servers.settings]          # `workspace/configuration`, by section
//! [language-servers.os.windows]        # what differs on an OS (`windows`, `macos`, `linux`):
//! default-engine = "built-in"          #   its default engine, transport, arguments
//! ```
//!
//! In `command`, `args`, `env`, the options and the settings, `${node}` is the app's own
//! Node (its program), `${php}` the app's own PHP (its program), `${server}` the server's bundled folder (`${server-uri}` the same as a
//! `file://` URI: a bundled schema given by URL), `${lsp}` the app's `lsp/`,
//! `${root}` the project's root, `${data}` the server's own folder in the app's data folder
//! (for every project: what it keeps across them — intelephense's licence activation),
//! `${cache}` the server's own folder in the project's data
//! folder (`<root>/<project dir>/lsp/<name>`, made when it starts: what it writes — its
//! log, its generated definitions — never in the app's read-only bundle nor in the
//! project's own files) and `${max-memory}` the memory limit chosen (MB; a value that is
//! only it becomes a number).
//!
//! With `transport = "tcp"` the server listens on a local port the client chose (free when it
//! starts, `127.0.0.1`), named by `${port}` in its arguments, and the client connects to it:
//! for servers that can't speak over pipes on an OS (PHP on Windows can't `select` on them).
//!
//! `default-engine` is what answers the server's languages until the user chooses (a server
//! that doesn't work well on an OS stays the built-in engine's there, still selectable).
//!
//! A program named without an extension is found with `.exe` on Windows
//! (`${server}/bin/clangd` is `clangd.exe` there).
//!
//! `contents` answers what the server asks the client for by URL (`vscode/content`: the
//! JSON server's schemas, which it is told never to download): a URL starting with a key is
//! the file named by the rest of it in the folder the key names; any other is refused.
//!
//! `install` describes a server that isn't bundled (its licence forbids it: intelephense) and
//! that the user installs from the settings: npm's package `npm` at `version`, its tarball
//! checked against `integrity` (the registry's SHA-512) — or found where it's installed
//! already (`program` in PATH, npm's global folder). `${server}` is then the package's
//! folder; until it's installed, the server isn't offered and doesn't start.
//!
//! `secret-options` names initialization options given from the app's secrets (a licence
//! key in the keychain, by its id): read on the server's thread when it starts, never in a
//! settings file, never in its log.
//!
//! `on-initialized` names notifications (without parameters) sent once the server is
//! initialized: what a server waits for before it uses something of the client's
//! (yaml-language-server asks for its schemas with `vscode/content` only after
//! `yaml/registerContentRequest`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

/// A server's description.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ServerSpec {
    pub name: String,
    pub languages: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default, deserialize_with = "json")]
    pub initialization_options: Value,
    #[serde(default, deserialize_with = "json")]
    pub settings: Value,
    /// Where it's told what the user excluded from the project.
    #[serde(default)]
    pub excluded_paths: Option<ExcludedPaths>,
    #[serde(default)]
    pub file_patterns: Vec<String>,
    #[serde(default)]
    pub language_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub extension_language_ids: BTreeMap<String, String>,
    /// LSP's ids by the file's name (a glob on the name, before the extension's).
    #[serde(default)]
    pub file_language_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub fallback: bool,
    /// What the built-in engine keeps answering where it's chosen (`diagnostics`: it reports
    /// none of its own; `deprecated`: it doesn't flag deprecations; `rename`: the built-in
    /// engine finds more uses; `completion`: its suggestions join the server's) — see
    /// `code_intelligence::servers::Kept`.
    #[serde(default)]
    pub built_in: Vec<String>,
    /// What it may ask for by URL (`vscode/content`): a URL prefix, the folder it's read from.
    #[serde(default)]
    pub contents: BTreeMap<String, String>,
    /// LSP's completion kinds (by name: `property`, `value`…) shown as ours (`attribute`,
    /// `tag`…) — see [`crate::convert::CompletionKinds`].
    #[serde(default)]
    pub completion_kinds: BTreeMap<String, String>,
    /// Notifications (without parameters) sent once it's initialized.
    #[serde(default)]
    pub on_initialized: Vec<String>,
    #[serde(default)]
    pub bundle: Option<String>,
    /// What answers its languages by default (`server`, `built-in`, `both`; none: `server`).
    #[serde(default)]
    pub default_engine: Option<String>,
    /// How the client talks to it.
    #[serde(default)]
    pub transport: Transport,
    /// What differs on an OS (`windows`, `macos`, `linux`): applied when it's read.
    #[serde(default)]
    pub os: BTreeMap<String, OsSpec>,
    /// Not bundled: how the user installs it (and finds it installed).
    #[serde(default)]
    pub install: Option<Install>,
    /// Initialization options given from the app's secrets: the option's key, the secret's id.
    #[serde(default)]
    pub secret_options: BTreeMap<String, String>,
    /// Its `window/showMessage` shown as notifications (its licence asks that none be hidden).
    #[serde(default)]
    pub show_messages: bool,
    /// Where it was described: a module's id, or `user` (the user's own file). Not read.
    #[serde(skip)]
    pub origin: String,
}

/// How the client talks to a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// Its stdin and stdout.
    #[default]
    Stdio,
    /// A local TCP port it listens on (`${port}` in its arguments), the client connecting.
    Tcp,
}

/// What differs in a server's description on an OS.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OsSpec {
    #[serde(default)]
    pub default_engine: Option<String>,
    #[serde(default)]
    pub transport: Option<Transport>,
    /// Its arguments there (instead of the description's).
    #[serde(default)]
    pub args: Option<Vec<String>>,
}

/// How a server that isn't bundled is installed: an npm package whose published files are the
/// whole server (no dependencies to install, nothing run), at a pinned version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Install {
    /// The package's name on registry.npmjs.org.
    pub npm: String,
    pub version: String,
    /// The tarball's `integrity` as the registry gives it (`sha512-<base64>`).
    pub integrity: String,
    /// Its program's name in PATH, where it may be installed already (npm's global `bin`).
    #[serde(default)]
    pub program: Option<String>,
    /// A file the package must have (relative to its folder): what runs.
    pub entry: String,
    /// Where its licence is read (shown before it's installed).
    #[serde(default)]
    pub licence_url: Option<String>,
    /// Its size, as the registry says it (bytes, shown before it's downloaded).
    #[serde(default)]
    pub size: Option<u64>,
}

/// The OSes a description may name (`std::env::consts::OS`'s).
pub const OSES: [&str; 3] = ["windows", "macos", "linux"];

/// A TOML value as JSON (what's sent to the server).
fn json<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
    let value = toml::Value::deserialize(d)?;
    serde_json::to_value(value).map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(rename = "language-servers", default)]
    servers: Vec<ServerSpec>,
}

/// The servers a TOML text describes (`origin`: whose it is), as they are on the OS this
/// runs on.
pub fn parse(text: &str, origin: &str) -> Result<Vec<ServerSpec>, String> {
    parse_for(text, origin, std::env::consts::OS)
}

/// The servers a TOML text describes, as they are on `os` (`windows`, `macos`, `linux`).
pub fn parse_for(text: &str, origin: &str, os: &str) -> Result<Vec<ServerSpec>, String> {
    let file: File = toml::from_str(text).map_err(|e| e.message().to_owned())?;
    let mut servers = file.servers;
    for server in &mut servers {
        if server.name.trim().is_empty() || server.command.trim().is_empty() {
            return Err("a server needs a name and a command".into());
        }
        if server.languages.is_empty() {
            return Err(format!("{}: no languages", server.name));
        }
        let kept = code_intelligence::servers::Kept::NAMES;
        if let Some(unknown) = server.built_in.iter().find(|k| !kept.contains(&k.as_str())) {
            return Err(format!(
                "{}: built-in `{unknown}` isn't one of {}",
                server.name,
                kept.join(", ")
            ));
        }
        crate::convert::CompletionKinds::parse(&server.completion_kinds)
            .map_err(|e| format!("{}: {e}", server.name))?;
        if let Some(unknown) = server.os.keys().find(|o| !OSES.contains(&o.as_str())) {
            return Err(format!(
                "{}: os `{unknown}` isn't one of {}",
                server.name,
                OSES.join(", ")
            ));
        }
        if let Some(install) = &server.install
            && (!install.integrity.starts_with("sha512-") || install.entry.trim().is_empty())
        {
            return Err(format!("{}: install needs a sha512 integrity and an entry", server.name));
        }
        if let Some(own) = server.os.get(os).cloned() {
            server.default_engine = own.default_engine.or(server.default_engine.take());
            server.transport = own.transport.unwrap_or(server.transport);
            if let Some(args) = own.args {
                server.args = args;
            }
        }
        let engines = server.os.values().filter_map(|o| o.default_engine.as_deref());
        if let Some(bad) = server
            .default_engine
            .as_deref()
            .into_iter()
            .chain(engines)
            .find(|e| code_intelligence::servers::Engine::from_id(e).is_none())
        {
            return Err(format!(
                "{}: default-engine `{bad}` isn't server, built-in or both",
                server.name
            ));
        }
        server.origin = origin.to_owned();
    }
    Ok(servers)
}

impl ServerSpec {
    /// The id LSP knows file `path`'s language by (ours: `language`).
    pub fn language_id(&self, language: &str, path: &Path) -> String {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        self.file_language_ids
            .iter()
            .find(|(pattern, _)| glob(pattern, name))
            .map(|(_, id)| id)
            .or_else(|| self.extension_language_ids.get(extension))
            .or_else(|| self.language_ids.get(language))
            .cloned()
            .unwrap_or_else(|| language.to_owned())
    }

    /// What answers its languages until the user chooses.
    pub fn default_engine(&self) -> code_intelligence::servers::Engine {
        self.default_engine
            .as_deref()
            .and_then(code_intelligence::servers::Engine::from_id)
            .unwrap_or_default()
    }

    /// Whether the server hears about a change of `relative` (a path relative to its root,
    /// `/`-separated) on disk.
    pub fn watches(&self, relative: &str) -> bool {
        self.file_patterns.iter().any(|p| glob(p, relative))
    }
}

/// What the placeholders stand for.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// The app's `lsp/` folder, if it has one.
    pub lsp: Option<PathBuf>,
    /// The bundled Node program.
    pub node: Option<PathBuf>,
    /// The bundled PHP program.
    pub php: Option<PathBuf>,
    /// The server's bundled folder.
    pub server: Option<PathBuf>,
    pub root: PathBuf,
    /// The server's own folder for what it writes (made when it starts).
    pub cache: Option<PathBuf>,
    /// The server's own folder in the app's data folder, for every project (made when it
    /// starts).
    pub data: Option<PathBuf>,
    /// MB.
    pub max_memory: u64,
}

/// Server `name`'s own folder in the data folder of the project at `root`.
pub fn cache_dir(root: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let safe = if safe.trim_matches('.').is_empty() { "server".to_owned() } else { safe };
    root.join(kernel::brand::PROJECT_DIR).join("lsp").join(safe)
}

/// What a server's description can't be started without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// The app has no `lsp/` folder (a build without the servers).
    Bundle,
    /// Its bundled Node isn't there.
    Node,
    /// Its bundled PHP isn't there.
    Php,
    /// Its bundled folder isn't there.
    Server(String),
    /// It isn't bundled and the user hasn't installed it.
    NotInstalled(String),
    /// Its program isn't found (not in the bundle, not absolute, not in PATH).
    Program(String),
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bundle => f.write_str("the app's language servers aren't installed"),
            Self::Node => f.write_str("the bundled Node is missing"),
            Self::Php => f.write_str("the bundled PHP is missing"),
            Self::Server(name) => write!(f, "the bundled server {name} is missing"),
            Self::NotInstalled(name) => write!(f, "{name} isn't installed"),
            Self::Program(name) => write!(f, "{name} wasn't found"),
        }
    }
}

/// A server ready to be started: its program, arguments and environment, the placeholders
/// replaced, and its options and settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub initialization_options: Value,
    pub settings: Value,
    /// What it may ask for by URL: a URL prefix and its folder (see [`ServerSpec::contents`]).
    pub contents: Vec<(String, PathBuf)>,
    /// How its completion kinds are shown.
    pub completion_kinds: crate::convert::CompletionKinds,
    /// Its own folder, if its description names it (`${cache}`): made before it starts.
    pub cache: Option<PathBuf>,
    /// Its folder in the app's data folder, if its description names it (`${data}`): made
    /// before it starts.
    pub data: Option<PathBuf>,
    /// Initialization options given from the app's secrets: the option's key, the secret's id.
    pub secret_options: Vec<(String, String)>,
    /// Its `window/showMessage` shown as notifications.
    pub show_messages: bool,
    /// Notifications sent once it's initialized (its description's `on-initialized`).
    pub on_initialized: Vec<String>,
    /// How the client talks to it (with `tcp`, `${port}` is still in `args`: chosen at each
    /// start).
    pub transport: Transport,
}

/// How a server is told what to leave out of the project: a list, in its configuration, of
/// `pattern` with `{path}` replaced by each path excluded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExcludedPaths {
    /// The list's place in its settings (`intelephense.files.exclude`, by dots).
    #[serde(default)]
    pub setting: Option<String>,
    /// The list's key in its initialization options (whole, dots and all).
    #[serde(default)]
    pub option: Option<String>,
    pub pattern: String,
}

impl Launch {
    /// This launch, `spec`'s server told to leave `excluded` out (added to what its
    /// description lists).
    pub fn excluding(&self, spec: &ServerSpec, excluded: &[String]) -> Self {
        let mut launch = self.clone();
        let Some(paths) = spec.excluded_paths.as_ref().filter(|_| !excluded.is_empty()) else {
            return launch;
        };
        let patterns = excluded.iter().map(|p| Value::from(paths.pattern.replace("{path}", p)));
        if let Some(key) = &paths.option
            && let Some(options) = launch.initialization_options.as_object_mut()
        {
            let list = options.entry(key.clone()).or_insert_with(|| Value::Array(Vec::new()));
            if let Some(list) = list.as_array_mut() {
                list.extend(patterns.clone());
            }
        }
        if let Some(place) = &paths.setting {
            let mut at = &mut launch.settings;
            for part in place.split('.') {
                if at.is_null() {
                    *at = Value::Object(serde_json::Map::new());
                }
                let Some(next) = at.as_object_mut().map(|o| o.entry(part).or_insert(Value::Null))
                else {
                    return launch;
                };
                at = next;
            }
            if at.is_null() {
                *at = Value::Array(Vec::new());
            }
            if let Some(list) = at.as_array_mut() {
                list.extend(patterns);
            }
        }
        launch
    }
}

impl ServerSpec {
    /// Whether its program or arguments name placeholder `name` (`node`: it runs on Node).
    pub fn uses(&self, name: &str) -> bool {
        let needle = format!("${{{name}}}");
        self.command.contains(&needle) || self.args.iter().any(|a| a.contains(&needle))
    }

    /// How to start it in `places`, or what's missing for it.
    pub fn launch(&self, places: &Places) -> Result<Launch, Missing> {
        let uses = |name: &str| self.uses(name);
        let uses_anywhere = |name: &str| {
            let needle = format!("${{{name}}}");
            uses(name)
                || self.env.values().any(|v| v.contains(&needle))
                || self.initialization_options.to_string().contains(&needle)
                || self.settings.to_string().contains(&needle)
        };
        // An installed server's folder isn't the bundle's.
        let bundled_server = uses("server") && self.install.is_none();
        if (uses("node") || uses("php") || bundled_server || uses("lsp")) && places.lsp.is_none() {
            return Err(Missing::Bundle);
        }
        if uses("node") && places.node.as_deref().is_none_or(|n| !n.is_file()) {
            return Err(Missing::Node);
        }
        if uses("php") && places.php.as_deref().is_none_or(|n| !n.is_file()) {
            return Err(Missing::Php);
        }
        if uses("server")
            && self.install.is_some()
            && places.server.as_deref().is_none_or(|s| !s.is_dir())
        {
            return Err(Missing::NotInstalled(self.name.clone()));
        }
        if uses("server") && places.server.as_deref().is_none_or(|s| !s.is_dir()) {
            return Err(Missing::Server(self.bundle.clone().unwrap_or_else(|| self.name.clone())));
        }
        let command = substitute(&self.command, places);
        let program = program(&command, places).ok_or_else(|| Missing::Program(command.clone()))?;
        Ok(Launch {
            program: program.into_os_string(),
            args: self.args.iter().map(|a| substitute(a, places).into()).collect(),
            env: self.env.iter().map(|(k, v)| (k.into(), substitute(v, places).into())).collect(),
            initialization_options: substitute_json(&self.initialization_options, places),
            settings: substitute_json(&self.settings, places),
            contents: self
                .contents
                .iter()
                .map(|(prefix, dir)| (prefix.clone(), PathBuf::from(substitute(dir, places))))
                .collect(),
            completion_kinds: crate::convert::CompletionKinds::parse(&self.completion_kinds)
                .unwrap_or_default(),
            cache: places.cache.clone().filter(|_| uses_anywhere("cache")),
            data: places.data.clone().filter(|_| uses_anywhere("data")),
            secret_options: self
                .secret_options
                .iter()
                .map(|(key, id)| (key.clone(), id.clone()))
                .collect(),
            show_messages: self.show_messages,
            on_initialized: self.on_initialized.clone(),
            transport: self.transport,
        })
    }
}

/// The file answering `url` among `contents` (a URL prefix, its folder): the rest of the
/// URL, its query and fragment left out, as a path in the folder — never outside it.
pub fn content_file(contents: &[(String, PathBuf)], url: &str) -> Option<PathBuf> {
    let url = url.split(['#', '?']).next().unwrap_or(url);
    contents.iter().find_map(|(prefix, dir)| {
        let rest = url.strip_prefix(prefix.as_str())?;
        let safe = !rest.is_empty()
            && rest.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
            && !rest.contains('\\')
            && !rest.contains(':');
        safe.then(|| rest.split('/').fold(dir.clone(), |path, part| path.join(part)))
    })
}

/// Where `command` is: absolute, in the app's `lsp/`, or in PATH.
fn program(command: &str, places: &Places) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return executable(path);
    }
    if let Some(bundled) = places.lsp.as_ref().and_then(|l| executable(&l.join(path))) {
        return Some(bundled);
    }
    process::which(command)
}

/// The program at `path`: that file, or on Windows the same with `.exe` when it's named
/// without an extension.
fn executable(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let exe = path.with_extension("exe");
    (cfg!(windows) && path.extension().is_none() && exe.is_file()).then_some(exe)
}

/// `text` with its placeholders replaced (an unknown one left as it is).
pub fn substitute(text: &str, places: &Places) -> String {
    let shown = |p: &Option<PathBuf>| p.as_deref().map(|p| p.to_string_lossy().into_owned());
    let mut out = text.to_owned();
    for (name, value) in [
        ("node", shown(&places.node)),
        ("php", shown(&places.php)),
        ("server", shown(&places.server)),
        ("server-uri", places.server.as_deref().map(crate::uri::from_path)),
        ("lsp", shown(&places.lsp)),
        ("root", Some(places.root.to_string_lossy().into_owned())),
        ("cache", shown(&places.cache)),
        ("data", shown(&places.data)),
        ("max-memory", Some(places.max_memory.to_string())),
    ] {
        if let Some(value) = value {
            out = out.replace(&format!("${{{name}}}"), &value);
        }
    }
    out
}

/// A JSON value with its strings' placeholders replaced; a string that is only
/// `${max-memory}` becomes the number.
pub fn substitute_json(value: &Value, places: &Places) -> Value {
    match value {
        Value::String(s) if s == "${max-memory}" => Value::from(places.max_memory),
        Value::String(s) => Value::String(substitute(s, places)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| substitute_json(v, places)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter().map(|(k, v)| (k.clone(), substitute_json(v, places))).collect(),
        ),
        other => other.clone(),
    }
}

/// Whether `path` (`/`-separated) matches glob `pattern`: `**/` any folders (none too),
/// `*` any characters but `/`, `?` one, `{a,b}` either.
pub fn glob(pattern: &str, path: &str) -> bool {
    if let Some(open) = pattern.find('{')
        && let Some(close) = pattern[open..].find('}').map(|c| c + open)
    {
        let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
        return pattern[open + 1..close]
            .split(',')
            .any(|alt| glob(&format!("{head}{alt}{tail}"), path));
    }
    matches(pattern.as_bytes(), path.as_bytes())
}

fn matches(p: &[u8], s: &[u8]) -> bool {
    if p.starts_with(b"**/") {
        let rest = &p[3..];
        // No folder, or after any `/`.
        return matches(rest, s)
            || s.iter().enumerate().any(|(i, &c)| c == b'/' && matches(rest, &s[i + 1..]));
    }
    if p == b"**" {
        return true;
    }
    match (p.first(), s.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            matches(&p[1..], s) || (s.first().is_some_and(|&c| c != b'/') && matches(p, &s[1..]))
        }
        (Some(b'?'), Some(&c)) => c != b'/' && matches(&p[1..], &s[1..]),
        (Some(a), Some(b)) => a == b && matches(&p[1..], &s[1..]),
        _ => false,
    }
}

/// The root a server is started at for file `path`: its nearest folder holding one of
/// `markers`, not above the project's `root` (none found: the project's root).
pub fn root_for(path: &Path, markers: &[String], root: &Path) -> PathBuf {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if !d.starts_with(root) {
            break;
        }
        if markers.iter().any(|m| d.join(m).exists()) {
            return d.to_path_buf();
        }
        if d == root {
            break;
        }
        dir = d.parent();
    }
    root.to_path_buf()
}

#[cfg(test)]
#[path = "../tests/unit/spec.rs"]
mod tests;
