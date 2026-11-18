//! Where the data sources' passwords are kept, by each source's [`SecretPolicy`] — never in
//! their configuration, never in a log:
//!
//! - **`ProjectVault`** (the default): in the project, encrypted — one vault file per source
//!   under `<project dir>/data-source-secrets/`, sealed with ChaCha20-Poly1305 by a key that
//!   lives in the system's keychain (`secrets::ProjectVault`, the same vault remote projects
//!   use): the file alone reveals nothing, and it goes with the project.
//! - **`Keychain`**: in the system's keychain (macOS Keychain, Windows Credential Manager,
//!   the Secret Service).
//! - **`Session`**: in memory until the project closes ([`Secret`]: wiped when dropped).
//! - **`Never`**: asked each time — given for one connection, then forgotten.
//!
//! A global source has no project to keep it in: its "project" password goes to the
//! keychain ([`effective`]). The stores are ports ([`SecretStore`]): the app gives the real
//! ones ([`Credentials::system`]), the tests and snapshots memory ones — never the user's
//! keychain. Every call that reaches a store may wait for the system (a keychain prompt): it
//! runs on a background thread, never the UI's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use database::{DataSource, DataSourceId, Scope, SecretPolicy};
use kernel::{Secret, SecretStore};

/// The folder of a project's data source vaults (in its data folder).
pub const VAULT_DIR: &str = "data-source-secrets";

/// The policy that applies to `source`: a global source's "in the project" is the
/// keychain (no project holds it).
pub fn effective(source: &DataSource) -> SecretPolicy {
    match (source.scope, source.secrets) {
        (Scope::Global, SecretPolicy::ProjectVault) => SecretPolicy::Keychain,
        (_, policy) => policy,
    }
}

/// A memory store (the tests', the snapshots'; nothing leaves the process).
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, Secret>>);

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore").field("len", &self.len()).finish()
    }
}

impl MemoryStore {
    /// How many secrets it holds.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, id: &str) -> Option<Secret> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).get(id).cloned()
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(id.to_owned(), secret.into());
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
        Ok(())
    }
}

/// A project's vaults: one encrypted file per source in `dir`, each sealed by its own key
/// kept in `keys` (the keychain) as `vault:<id>` — as remote projects keep theirs.
pub struct SourceVaults {
    pub dir: PathBuf,
    pub keys: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for SourceVaults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceVaults").field("dir", &self.dir).finish_non_exhaustive()
    }
}

impl SourceVaults {
    /// The vault of secret `id` (`data-source/<uuid>`): its file named by the uuid alone.
    fn vault(&self, id: &str) -> secrets::ProjectVault {
        let name: String = id
            .rsplit('/')
            .next()
            .unwrap_or(id)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        secrets::ProjectVault {
            file: self.dir.join(name),
            keys: Arc::clone(&self.keys),
            key_name: format!("vault:{id}"),
        }
    }

    /// Where secret `id`'s vault is.
    pub fn file(&self, id: &str) -> PathBuf {
        self.vault(id).file
    }
}

impl SecretStore for SourceVaults {
    fn get(&self, id: &str) -> Option<Secret> {
        self.vault(id).get(id)
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.vault(id).set(id, secret)
    }

    /// The vault's file and its key go (nothing of it is left).
    fn remove(&self, id: &str) -> Result<(), String> {
        let vault = self.vault(id);
        match std::fs::remove_file(&vault.file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        self.keys.remove(&vault.key_name)
    }
}

/// A project's passwords (see the module's doc).
pub struct Credentials {
    vault: Arc<dyn SecretStore>,
    keychain: Arc<dyn SecretStore>,
    /// `Session` and `Never` passwords, by source (`Never`'s taken by its connection).
    memory: Mutex<HashMap<DataSourceId, Secret>>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").finish_non_exhaustive()
    }
}

impl Default for Credentials {
    /// Kept in memory only (a project made without the system's stores: the tests).
    fn default() -> Self {
        Self::new(Arc::new(MemoryStore::default()), Arc::new(MemoryStore::default()))
    }
}

/// What a source's password became after a change of its settings (said in the modal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// It stayed where it was.
    Kept,
    /// It went from one place to another.
    To(SecretPolicy),
    /// It was dropped (now asked each time).
    Dropped,
}

impl Credentials {
    pub fn new(vault: Arc<dyn SecretStore>, keychain: Arc<dyn SecretStore>) -> Self {
        Self { vault, keychain, memory: Mutex::default() }
    }

    /// The system's: the project at `root`'s vaults (their keys in the keychain) and the
    /// keychain itself, both under the app's own service.
    pub fn system(root: &Path) -> Self {
        let keychain: Arc<dyn SecretStore> =
            Arc::new(secrets::Keychain { service: format!("{}.database", kernel::brand::ID) });
        let vault = SourceVaults {
            dir: root.join(kernel::brand::PROJECT_DIR).join(VAULT_DIR),
            keys: Arc::clone(&keychain),
        };
        Self::new(Arc::new(vault), keychain)
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, HashMap<DataSourceId, Secret>> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn store(&self, policy: SecretPolicy) -> Option<&Arc<dyn SecretStore>> {
        match policy {
            SecretPolicy::ProjectVault => Some(&self.vault),
            SecretPolicy::Keychain => Some(&self.keychain),
            SecretPolicy::Session | SecretPolicy::Never => None,
        }
    }

    /// `source`'s password for a connection (may wait for the keychain: never on the UI
    /// thread). One asked each time is given once (the next connection asks again).
    pub fn get(&self, source: &DataSource) -> Option<Secret> {
        match effective(source) {
            SecretPolicy::Session => self.memory().get(&source.id).cloned(),
            SecretPolicy::Never => self.memory().remove(&source.id),
            policy => self.store(policy)?.get(&source.secret_id()),
        }
    }

    /// `source`'s password without taking it (a test of its connection: one asked each
    /// time stays for the connection it was typed for).
    pub fn peek(&self, source: &DataSource) -> Option<Secret> {
        match effective(source) {
            SecretPolicy::Session | SecretPolicy::Never => self.memory().get(&source.id).cloned(),
            policy => self.store(policy)?.get(&source.secret_id()),
        }
    }

    /// Whether a password is known for `source` without asking (in memory; a stored one is
    /// assumed there).
    pub fn has(&self, source: &DataSource) -> bool {
        match effective(source) {
            SecretPolicy::Session | SecretPolicy::Never => self.memory().contains_key(&source.id),
            _ => true,
        }
    }

    /// `secret` kept for `source` as its policy says (an empty one: none kept).
    pub fn set(&self, source: &DataSource, secret: &Secret) -> Result<(), String> {
        if secret.is_empty() {
            return self.remove(source);
        }
        match effective(source) {
            SecretPolicy::Session | SecretPolicy::Never => {
                self.memory().insert(source.id, secret.clone());
                Ok(())
            }
            policy => match self.store(policy) {
                Some(store) => store.set(&source.secret_id(), secret.expose()),
                None => Ok(()),
            },
        }
    }

    /// `source`'s password forgotten, wherever it was.
    pub fn remove(&self, source: &DataSource) -> Result<(), String> {
        self.memory().remove(&source.id);
        match self.store(effective(source)) {
            Some(store) => store.remove(&source.secret_id()),
            None => Ok(()),
        }
    }

    /// `old`'s settings became `new`'s (another policy, or moved between the project and
    /// every project): its password follows — taken from where it was, kept where it goes,
    /// and only then removed from there (a failure loses nothing). To `Never`, it's dropped.
    pub fn moved(&self, old: &DataSource, new: &DataSource) -> Result<Moved, String> {
        let (from, to) = (effective(old), effective(new));
        if from == to && old.id == new.id {
            return Ok(Moved::Kept);
        }
        let secret = match from {
            SecretPolicy::Session | SecretPolicy::Never => self.memory().get(&old.id).cloned(),
            policy => self.store(policy).and_then(|s| s.get(&old.secret_id())),
        };
        if to == SecretPolicy::Never {
            self.remove(old)?;
            return Ok(Moved::Dropped);
        }
        if let Some(secret) = &secret {
            self.set(new, secret)?;
        }
        // Gone from where it was (memory too, unless it's where it stays).
        let in_memory = |p: SecretPolicy| matches!(p, SecretPolicy::Session | SecretPolicy::Never);
        if in_memory(from) && !(in_memory(to) && old.id == new.id) {
            self.memory().remove(&old.id);
        }
        if let Some(store) = self.store(from) {
            store.remove(&old.secret_id())?;
        }
        Ok(Moved::To(to))
    }
}

#[cfg(test)]
#[path = "../tests/unit/credentials.rs"]
mod tests;
