//! The servers' secrets (intelephense's licence key): kept in the system's keychain
//! (`secrets::Keychain`, the app's `<id>.language-servers` service), app-wide — a licence is
//! the user's, for every project — never in a settings file, never in a log. Read and
//! written off the UI thread only (the keychain may wait, or ask the user); what's known of
//! them (set or not) is kept for the settings. A licence file the server's own tools use
//! (`~/intelephense/licence.txt`) can be taken in: read once, only when the user asks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use kernel::SecretStore;

/// Where the secrets are (tests, snapshots: in memory, never the user's keychain).
static STORE: RwLock<Option<Arc<dyn SecretStore>>> = RwLock::new(None);
/// Whether each secret is set, as last seen (`None` while it's looked up).
static KNOWN: Mutex<Option<HashMap<String, Option<bool>>>> = Mutex::new(None);
/// The licence files looked for, when set (tests, snapshots: never the user's home).
static FILES: RwLock<Option<Vec<PathBuf>>> = RwLock::new(None);

/// The secrets are kept in `store` (tests, snapshots).
pub fn use_secrets(store: Arc<dyn SecretStore>) {
    if let Ok(mut s) = STORE.write() {
        *s = Some(store);
    }
    if let Ok(mut known) = KNOWN.lock() {
        *known = None;
    }
}

/// The licence files looked for are `files` (tests, snapshots).
pub fn use_licence_files(files: Vec<PathBuf>) {
    if let Ok(mut f) = FILES.write() {
        *f = Some(files);
    }
}

/// The store the servers' secrets are in: the keychain, unless another was set.
pub fn store() -> Arc<dyn SecretStore> {
    if let Some(store) = STORE.read().ok().and_then(|s| s.clone()) {
        return store;
    }
    Arc::new(secrets::Keychain { service: format!("{}.language-servers", kernel::brand::ID) })
}

fn with<R>(f: impl FnOnce(&mut HashMap<String, Option<bool>>) -> R) -> R {
    let mut known = KNOWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(known.get_or_insert_with(HashMap::new))
}

/// Whether secret `id` is set: known, or looked up in the background (`None` meanwhile,
/// `wake` once it's known).
pub fn is_set(id: &str, wake: Option<Arc<dyn Fn() + Send + Sync>>) -> Option<bool> {
    let first = with(|k| match k.get(id) {
        Some(known) => Err(*known),
        None => {
            k.insert(id.to_owned(), None);
            Ok(())
        }
    });
    if let Err(known) = first {
        return known;
    }
    let id = id.to_owned();
    kernel::work::run("lsp-secrets", move || {
        let set = store().get(&id).is_some_and(|s| !s.expose().trim().is_empty());
        with(|k| k.insert(id, Some(set)));
        if let Some(wake) = wake {
            wake();
        }
    });
    None
}

/// Secret `id` saved as `value` (an empty one removes it), on this thread (a background
/// one): the servers given it start again with it.
pub fn save(id: &str, value: &str) -> Result<(), String> {
    let value = value.trim();
    let outcome = if value.is_empty() { store().remove(id) } else { store().set(id, value) };
    match &outcome {
        Ok(()) => {
            with(|k| k.insert(id.to_owned(), Some(!value.is_empty())));
            crate::projects::restart_using_secret(id);
        }
        Err(e) => tracing::warn!("a language server's secret can't be saved: {e}"),
    }
    outcome
}

/// A licence file the server's own tools keep (`~/intelephense/licence.txt` or
/// `license.txt`), if there's one.
pub fn licence_file() -> Option<PathBuf> {
    let files = match FILES.read().ok().and_then(|f| f.clone()) {
        Some(files) => files,
        None => {
            let home = dirs_home()?;
            let dir = home.join("intelephense");
            vec![dir.join("licence.txt"), dir.join("license.txt")]
        }
    };
    files.into_iter().find(|f| f.is_file())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The key in licence file `file` saved as secret `id` (the file only read).
pub fn import(id: &str, file: &std::path::Path) -> Result<(), String> {
    let mut text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let key = text.lines().next().unwrap_or_default().trim().to_owned();
    kernel::secret::wipe(&mut text);
    if key.is_empty() {
        return Err(format!("{}: empty", file.display()));
    }
    let mut key = key;
    let outcome = save(id, &key);
    kernel::secret::wipe(&mut key);
    outcome
}

#[cfg(test)]
#[path = "../tests/unit/licence.rs"]
mod tests;
