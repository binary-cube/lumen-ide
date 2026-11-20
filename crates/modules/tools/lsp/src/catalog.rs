//! The servers known: those the languages' modules describe (their `language-servers.toml`,
//! registered by the app as [`code_intelligence::servers::ServerData`]) and the user's own
//! (`<config folder>/language-servers.toml`, the same format; one named like a module's
//! replaces it). Read once, again when the user adds a server.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use code_intelligence::servers::ServerData;
use lsp_client::bundle::Bundle;
use lsp_client::spec::{self, Missing, Places, ServerSpec};

/// The user's file's name in the config folder.
pub const USER_FILE: &str = "language-servers.toml";

/// The servers known, and what couldn't be read.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub servers: Vec<ServerSpec>,
    /// What couldn't be read (whose, and why).
    pub errors: Vec<String>,
}

impl Catalog {
    /// The modules' descriptions, then the user's file (`user`, if it's there).
    pub fn load(data: &[ServerData], user: Option<&Path>) -> Self {
        let mut catalog = Self::default();
        for d in data {
            match spec::parse(d.toml, d.module) {
                Ok(servers) => catalog.servers.extend(servers),
                Err(e) => catalog.errors.push(format!("{}: {e}", d.module)),
            }
        }
        if let Some(text) = user.and_then(|p| std::fs::read_to_string(p).ok()) {
            match spec::parse(&text, "user") {
                Ok(servers) => {
                    for server in servers {
                        catalog.servers.retain(|s| s.name != server.name);
                        catalog.servers.push(server);
                    }
                }
                Err(e) => catalog.errors.push(format!("{USER_FILE}: {e}")),
            }
        }
        catalog
    }

    /// The languages some server serves, in the order they were described.
    pub fn languages(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for language in self.servers.iter().flat_map(|s| &s.languages) {
            if !out.contains(language) {
                out.push(language.clone());
            }
        }
        out
    }

    /// The servers of `language`, in order (a fallback after the one it stands in for).
    pub fn for_language<'a>(
        &'a self,
        language: &'a str,
    ) -> impl Iterator<Item = &'a ServerSpec> + 'a {
        self.servers.iter().filter(move |s| s.languages.iter().any(|l| l == language))
    }

    pub fn named(&self, name: &str) -> Option<&ServerSpec> {
        self.servers.iter().find(|s| s.name == name)
    }

    /// The server to start for `language` (`preferred` first, if the user chose one; then
    /// the first that can start, a fallback only when the ones before it can't; `disabled`:
    /// turned off by the user), with how — or why none can (the first one's reason).
    pub fn choose(
        &self,
        language: &str,
        preferred: Option<&str>,
        disabled: &[String],
        places: impl Fn(&ServerSpec) -> Places,
    ) -> Result<(ServerSpec, lsp_client::spec::Launch), Option<Missing>> {
        // The first that starts wins: a fallback is reached only when the ones before it
        // can't (or are turned off).
        let mut first_missing = None;
        let chosen =
            preferred.and_then(|name| self.for_language(language).find(|s| s.name == name));
        for server in chosen.into_iter().chain(self.for_language(language)) {
            if disabled.contains(&server.name) {
                continue;
            }
            match server.launch(&places(server)) {
                Ok(launch) => return Ok((server.clone(), launch)),
                Err(missing) => {
                    first_missing.get_or_insert(missing);
                }
            }
        }
        Err(first_missing)
    }
}

/// The places a server's placeholders stand for, for a project at `root`: each part where
/// it is — in `bundle` (the app's `lsp/`, if it has one) first, else downloaded (the shared
/// folder, `crate::downloads`); a server the user installs has its install's folder as
/// `${server}`.
pub fn places(
    bundle: Option<&Bundle>,
    server: &ServerSpec,
    root: &Path,
    max_memory: u64,
) -> Places {
    Places {
        lsp: bundle.map(|b| b.root.clone()).or_else(crate::downloads::root),
        node: bundle.and_then(Bundle::node).or_else(crate::downloads::node),
        php: bundle.and_then(Bundle::php).or_else(crate::downloads::php),
        server: match &server.install {
            Some(_) => crate::install::installed(server).map(|i| i.dir),
            None => server.bundle.as_deref().and_then(|name| {
                bundle
                    .and_then(|b| b.server(name))
                    .map(|(p, _)| p)
                    .or_else(|| crate::downloads::server(name).map(|(p, _)| p))
            }),
        },
        root: root.to_path_buf(),
        cache: Some(spec::cache_dir(root, &server.name)),
        data: crate::install::data_dir(&server.name),
        max_memory,
    }
}

/// The servers' version in the bundle, or installed (`None`: neither).
pub fn bundled_version(bundle: Option<&Bundle>, server: &ServerSpec) -> Option<String> {
    if server.install.is_some() {
        return crate::install::installed(server).map(|i| i.version);
    }
    let name = server.bundle.as_deref()?;
    bundle
        .and_then(|b| b.server(name))
        .map(|(_, v)| v.to_owned())
        .or_else(|| crate::downloads::server(name).map(|(_, v)| v))
}

static CATALOG: RwLock<Option<Arc<Catalog>>> = RwLock::new(None);
static USER: RwLock<Option<PathBuf>> = RwLock::new(None);
static BUNDLE: RwLock<Option<Option<Arc<Bundle>>>> = RwLock::new(None);

/// Where the user's file is: set (a test's, the snapshots'), else the config folder's.
pub fn user_file() -> Option<PathBuf> {
    if let Some(path) = USER.read().ok().and_then(|u| u.clone()) {
        return Some(path);
    }
    storage::config_dir().map(|d| d.join(USER_FILE))
}

/// The user's file is at `path` (tests, snapshots: never the real one); read again.
pub fn use_user_file(path: PathBuf) {
    if let Ok(mut user) = USER.write() {
        *user = Some(path);
    }
    reload();
}

/// The servers known (read on first use).
pub fn catalog() -> Arc<Catalog> {
    if let Some(catalog) = CATALOG.read().ok().and_then(|c| c.clone()) {
        return catalog;
    }
    let loaded =
        Arc::new(Catalog::load(&code_intelligence::servers::data(), user_file().as_deref()));
    for error in &loaded.errors {
        tracing::warn!(%error, "a language server's description can't be read");
    }
    if let Ok(mut c) = CATALOG.write() {
        *c = Some(Arc::clone(&loaded));
    }
    loaded
}

/// Reads the descriptions again (the user added a server).
pub fn reload() {
    if let Ok(mut c) = CATALOG.write() {
        *c = None;
    }
}

/// The bundled servers are those at `root` (`None`: none — the snapshots, which use the
/// fake server only).
pub fn use_bundle(root: Option<PathBuf>) {
    let found = root.and_then(|r| Bundle::open(&r)).map(Arc::new);
    if let Ok(mut b) = BUNDLE.write() {
        *b = Some(found);
    }
    reload();
}

/// The app's bundled servers (looked for once, beside the app).
pub fn bundle() -> Option<Arc<Bundle>> {
    if let Some(found) = BUNDLE.read().ok().and_then(|b| b.clone()) {
        return found;
    }
    let found = std::env::current_exe().ok().and_then(|exe| Bundle::locate(&exe)).map(Arc::new);
    match &found {
        Some(b) => tracing::info!(root = %b.root.display(), "language servers bundled"),
        None => tracing::info!("no bundled language servers"),
    }
    if let Ok(mut b) = BUNDLE.write() {
        *b = Some(found.clone());
    }
    found
}

/// A server added by the user (from the settings): appended to the user's file.
pub fn add_user_server(
    name: &str,
    command: &str,
    args: &[String],
    languages: &[String],
) -> Result<PathBuf, String> {
    let path = user_file().ok_or("no config folder")?;
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let list = |items: &[String]| items.iter().map(|i| quote(i)).collect::<Vec<_>>().join(", ");
    let table = format!(
        "\n[[language-servers]]\nname = {}\nlanguages = [{}]\ncommand = {}\nargs = [{}]\n",
        quote(name),
        list(languages),
        quote(command),
        list(args)
    );
    // Checked before it's written: a description that can't be read isn't added.
    spec::parse(&table, "user")?;
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if spec::parse(&text, "user").is_ok_and(|servers| servers.iter().any(|s| s.name == name)) {
        return Err(format!("a server named {name} exists already"));
    }
    text.push_str(&table);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    storage::write_atomic(&path, &text).map_err(|e| e.to_string())?;
    reload();
    Ok(path)
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
