//! The server's token: kept in the system's keychain (the app's `<id>.mcp` service), never in
//! the settings' file — read, moved there (an older one from the settings) or made on a
//! worker (the keychain may wait, or ask), then kept in memory while the app runs. An agent
//! gets it through a helper command — this program run with [`HELPER_ARG`], which prints the
//! header it sends — so the token is never on a command line nor in an agent's
//! configuration.

use std::sync::{Arc, Mutex, PoisonError, RwLock};

use kernel::SecretStore;

pub use mcp::HELPER_ARG;

/// The token's name in the store.
const ID: &str = mcp::TOKEN_ID;

/// Where the token is kept (tests, snapshots: in memory, never the user's keychain).
static STORE: RwLock<Option<Arc<dyn SecretStore>>> = RwLock::new(None);
/// The token, once read or made (this run).
static TOKEN: Mutex<Option<String>> = Mutex::new(None);

/// The token is kept in `store` (tests, snapshots).
pub fn use_secrets(store: Arc<dyn SecretStore>) {
    *STORE.write().unwrap_or_else(PoisonError::into_inner) = Some(store);
    *TOKEN.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

fn store() -> Arc<dyn SecretStore> {
    if let Some(store) = STORE.read().unwrap_or_else(PoisonError::into_inner).clone() {
        return store;
    }
    Arc::new(secrets::Keychain { service: format!("{}.{}", kernel::brand::ID, mcp::TOKEN_SERVICE) })
}

/// A new token: 256 bits, as hex.
fn made() -> String {
    format!("{:032x}{:032x}", kernel::id::new_uuid(), kernel::id::new_uuid())
}

/// On a worker: the token — this run's, else the keychain's, else `older` (one the settings
/// kept, moved into the keychain), else a new one kept there. `false` beside it: it couldn't be
/// kept (only this run knows it; an agent configured with it won't reach the next).
pub(crate) fn token(older: Option<String>) -> (String, bool) {
    if let Some(token) = TOKEN.lock().unwrap_or_else(PoisonError::into_inner).clone() {
        return (token, true);
    }
    let store = store();
    let kept = store.get(ID).map(|s| s.expose().to_owned()).filter(|t| t.len() >= 32);
    let (token, saved) = match kept {
        Some(token) => (token, true),
        None => {
            let token = older.filter(|t| t.len() >= 32).unwrap_or_else(made);
            let saved = store.set(ID, &token);
            if let Err(e) = &saved {
                tracing::warn!(error = %e, "the MCP server's token wasn't kept in the keychain");
            }
            (token, saved.is_ok())
        }
    };
    *TOKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(token.clone());
    (token, saved)
}

/// What the helper prints: the header an agent sends, as a JSON object (`None`: no token
/// kept yet — the server never ran).
pub fn helper_headers() -> Option<String> {
    Some(mcp::helper_output(store().get(ID)?.expose()))
}

/// The helper as a shell runs it: this program, quoted, with [`HELPER_ARG`].
pub fn helper_command() -> Option<String> {
    let program = std::env::current_exe().ok()?;
    Some(format!("{} {HELPER_ARG}", quoted(&program.to_string_lossy())))
}

/// `path` quoted for the shell an agent runs its helper in: single quotes on Unix (nothing in
/// them expands), double quotes on Windows.
pub fn quoted(path: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", path.replace('"', ""))
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
#[path = "../tests/unit/secret.rs"]
pub(crate) mod tests;
