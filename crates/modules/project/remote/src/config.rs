//! Where a remote project's connection is kept — in the project's data folder (it goes with
//! the project) or in the user's settings (every connection, each with its local copy's
//! folder) — and how it's read back. Secrets are never here: the keychain, or the project's
//! encrypted vault ([`secret_store`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use remote::properties::Principal;
use remote::{Auth, Connection, Mode, Protocol, Retry, SavedIn, SecretStore, SecretsIn, WriteAs};
use serde::{Deserialize, Serialize};

/// A connection as kept, and its local copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub connection: Connection,
    /// The local copy's folder (the project opened).
    pub local: PathBuf,
    /// Certificate fingerprints the user trusts for this server (FTPS).
    pub trusted: Vec<String>,
}

/// The file format (no secrets).
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    id: String,
    name: String,
    protocol: String,
    host: String,
    port: u16,
    user: String,
    /// `password`, `key`, `agent`.
    auth: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<PathBuf>,
    root: String,
    /// `mirror`, `on-demand`.
    mode: String,
    #[serde(default)]
    excluded: Vec<String>,
    /// `keychain`, `project`.
    secrets: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_delay_ms: Option<u64>,
    /// Uploads / downloads at once (none: the global settings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uploads: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    downloads: Option<usize>,
    /// Moves and deletions made here go to the server too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    moves: bool,
    /// A save uploads at once (on unless the file says otherwise).
    #[serde(default = "on", skip_serializing_if = "is_on")]
    auto_upload: bool,
    /// Who files written on the server belong to: `project` (its folder's), `chosen` (the
    /// owner and group below); none: as the server makes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    write_as: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_id: Option<u32>,
    local: PathBuf,
    #[serde(default)]
    trusted: Vec<String>,
}

/// What a missing switch means: on.
fn on() -> bool {
    true
}

/// A switch left at its default (on) isn't written.
fn is_on(value: &bool) -> bool {
    *value
}

/// The chosen owner (`owner`) or group of `write_as`.
fn chosen(write_as: &WriteAs, owner: bool) -> Option<&Principal> {
    match write_as {
        WriteAs::Chosen { owner: o, .. } if owner => o.as_ref(),
        WriteAs::Chosen { group, .. } => group.as_ref(),
        _ => None,
    }
}

/// An account as saved: its id is what's set (a name alone is shown, not set).
fn principal(name: Option<String>, id: Option<u32>) -> Option<Principal> {
    (name.is_some() || id.is_some()).then_some(Principal { name, id })
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default, rename = "connection")]
    connections: Vec<Entry>,
}

impl Entry {
    fn from(saved: &Saved) -> Self {
        let c = &saved.connection;
        let (auth, key) = match &c.auth {
            Auth::Password => ("password", None),
            Auth::Key { path } => ("key", Some(path.clone())),
            Auth::Agent => ("agent", None),
        };
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            protocol: c.protocol.id().to_owned(),
            host: c.host.clone(),
            port: c.port,
            user: c.user.clone(),
            auth: auth.to_owned(),
            key,
            root: c.root.clone(),
            mode: match c.mode {
                Mode::Mirror => "mirror",
                Mode::OnDemand => "on-demand",
            }
            .to_owned(),
            excluded: c.excluded.clone(),
            secrets: match c.secrets_in {
                SecretsIn::Keychain => "keychain",
                SecretsIn::ProjectEncrypted => "project",
            }
            .to_owned(),
            retries: c.retry.map(|r| r.attempts),
            retry_delay_ms: c.retry.map(|r| r.delay.as_millis() as u64),
            uploads: c.uploads,
            downloads: c.downloads,
            moves: c.moves,
            auto_upload: c.auto_upload,
            write_as: match c.write_as {
                WriteAs::Server => None,
                WriteAs::Project => Some("project".into()),
                WriteAs::Chosen { .. } => Some("chosen".into()),
            },
            owner: chosen(&c.write_as, true).and_then(|p| p.name.clone()),
            owner_id: chosen(&c.write_as, true).and_then(|p| p.id),
            group: chosen(&c.write_as, false).and_then(|p| p.name.clone()),
            group_id: chosen(&c.write_as, false).and_then(|p| p.id),
            local: saved.local.clone(),
            trusted: saved.trusted.clone(),
        }
    }

    fn into_saved(self, saved_in: SavedIn) -> Option<Saved> {
        let auth = match self.auth.as_str() {
            "password" => Auth::Password,
            "key" => Auth::Key { path: self.key? },
            "agent" => Auth::Agent,
            _ => return None,
        };
        let retry = self.retries.map(|attempts| Retry {
            attempts,
            delay: std::time::Duration::from_millis(self.retry_delay_ms.unwrap_or(1000)),
        });
        Some(Saved {
            connection: Connection {
                id: self.id,
                name: self.name,
                protocol: Protocol::from_id(&self.protocol)?,
                host: self.host,
                port: self.port,
                user: self.user,
                auth,
                root: self.root,
                mode: if self.mode == "on-demand" { Mode::OnDemand } else { Mode::Mirror },
                excluded: self.excluded,
                saved_in,
                secrets_in: if self.secrets == "project" {
                    SecretsIn::ProjectEncrypted
                } else {
                    SecretsIn::Keychain
                },
                retry,
                uploads: self.uploads,
                downloads: self.downloads,
                moves: self.moves,
                auto_upload: self.auto_upload,
                write_as: match self.write_as.as_deref() {
                    Some("project") => WriteAs::Project,
                    Some("chosen") => WriteAs::Chosen {
                        owner: principal(self.owner, self.owner_id),
                        group: principal(self.group, self.group_id),
                    },
                    _ => WriteAs::Server,
                },
            },
            local: self.local,
            trusted: self.trusted,
        })
    }
}

/// The project's data folder (the brand's, inside the project: never committed).
pub fn data_dir(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR)
}

fn project_file(root: &Path) -> PathBuf {
    data_dir(root).join("remote.toml")
}

/// The user's file of connections (each with its local copy's folder).
fn global_file() -> Option<PathBuf> {
    user_file("remote-connections.toml")
}

/// Where trusted server keys are learned (ours; `~/.ssh/known_hosts` is only read).
pub fn known_hosts() -> Option<PathBuf> {
    user_file("known_hosts")
}

/// File `name` of the user's config folder; in this crate's tests a temporary one per
/// thread (tests never touch the user's settings).
fn user_file(name: &str) -> Option<PathBuf> {
    if cfg!(test) {
        let thread = format!("{:?}", std::thread::current().id());
        let name = format!("{name}-{}-{}", std::process::id(), thread.replace(['(', ')'], ""));
        return Some(std::env::temp_dir().join(name));
    }
    files::places::config_file(name)
}

fn read(file: &Path, saved_in: SavedIn) -> Vec<Saved> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let parsed: File = toml::from_str(&text).unwrap_or_else(|e| {
        tracing::warn!("{} is invalid: {e}", file.display());
        File::default()
    });
    parsed.connections.into_iter().filter_map(|e| e.into_saved(saved_in)).collect()
}

/// The connection of the project at `root`: its own, else a global one whose local copy
/// it is. `None`: a local project (nothing remote runs). The project's own copy is the
/// project itself, whatever its file says (it could come with a cloned repository).
pub fn find(root: &Path) -> Option<Saved> {
    if let Some(mut own) = read(&project_file(root), SavedIn::Project).into_iter().next() {
        own.local = root.to_path_buf();
        return Some(own);
    }
    let global = global_file()?;
    read(&global, SavedIn::Global).into_iter().find(|s| s.local == root)
}

/// Keeps `saved` where it says, replacing the one with its id or with its local copy (a
/// folder has one connection); a replaced connection's secret is removed with it (never
/// left behind in the keychain).
pub fn save(saved: &Saved) -> std::io::Result<()> {
    let file = match saved.connection.saved_in {
        SavedIn::Project => project_file(&saved.local),
        SavedIn::Global => global_file().ok_or(std::io::ErrorKind::NotFound)?,
    };
    let (replaced, kept): (Vec<Saved>, Vec<Saved>) = read(&file, saved.connection.saved_in)
        .into_iter()
        .partition(|s| s.connection.id == saved.connection.id || s.local == saved.local);
    let mut entries: Vec<Entry> = kept.iter().map(Entry::from).collect();
    entries.push(Entry::from(saved));
    let text = toml::to_string_pretty(&File { connections: entries })
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    files::write_atomic(&file, text.as_bytes())?;
    // Made here by the user: it runs without asking.
    approve(saved);
    for old in replaced.iter().filter(|s| s.connection.id != saved.connection.id) {
        if let Err(e) = secret_store(old).remove(&old.connection.id) {
            tracing::warn!("can't remove the replaced connection's secret: {e}");
        }
    }
    Ok(())
}

/// Why an edited connection wasn't saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsaved {
    /// Its secret couldn't be kept (the keychain, the vault).
    Secret(String),
    /// Its file couldn't be written.
    Save(String),
}

/// Saves `edited` over `original` — the same connection (its identity kept), its settings
/// changed — and its secret with it: a new one (`secret`) replaces the kept one; none keeps
/// it, moved along when it's now kept elsewhere; signing in with the agent drops it. Kept
/// elsewhere now (the project / the user's settings), it leaves where it was. `stores`:
/// where a connection's secret is ([`secret_store`]; a test's memory).
pub fn update(
    original: &Saved,
    edited: &Saved,
    secret: Option<&remote::Secret>,
    stores: &dyn Fn(&Saved) -> Arc<dyn SecretStore>,
) -> Result<(), Unsaved> {
    let id = &original.connection.id;
    let mut edited = edited.clone();
    edited.connection.id.clone_from(id);
    let (before, after) = (stores(original), stores(&edited));
    let moved = original.connection.secrets_in != edited.connection.secrets_in
        || original.local != edited.local;
    let needs_secret = edited.connection.auth != Auth::Agent;
    // The secret first: if it can't be kept, nothing changes.
    if needs_secret {
        match secret.filter(|s| !s.is_empty()) {
            Some(new) => after.set(id, new.expose()).map_err(Unsaved::Secret)?,
            None if moved => {
                if let Some(kept) = before.get(id) {
                    after.set(id, kept.expose()).map_err(Unsaved::Secret)?;
                }
            }
            None => {}
        }
    }
    save(&edited).map_err(|e| Unsaved::Save(e.to_string()))?;
    if original.connection.saved_in != edited.connection.saved_in
        && let Err(e) = forget(original)
    {
        tracing::warn!("the connection's old entry stays where it was: {e}");
    }
    // Where it was, the secret goes (never left behind); not needed, it goes too.
    if (moved || !needs_secret)
        && let Err(e) = before.remove(id)
    {
        tracing::warn!("can't remove the connection's old secret: {e}");
    }
    Ok(())
}

/// Removes `saved`'s entry from where it says it's kept (its secret stays).
fn forget(saved: &Saved) -> std::io::Result<()> {
    let file = match saved.connection.saved_in {
        SavedIn::Project => project_file(&saved.local),
        SavedIn::Global => global_file().ok_or(std::io::ErrorKind::NotFound)?,
    };
    let kept: Vec<Entry> = read(&file, saved.connection.saved_in)
        .iter()
        .filter(|s| s.connection.id != saved.connection.id)
        .map(Entry::from)
        .collect();
    let text = toml::to_string_pretty(&File { connections: kept })
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    files::write_atomic(&file, text.as_bytes())
}

/// The connections the user allowed to run (a project's own file may come with the project:
/// it runs only once allowed): one line per project folder, with what it was allowed as.
/// Kept with the user's settings, never in the project.
fn approvals_file() -> Option<PathBuf> {
    // Tests never touch the user's settings.
    if cfg!(test) {
        let thread = format!("{:?}", std::thread::current().id());
        let name =
            format!("remote-approved-{}-{}", std::process::id(), thread.replace(['(', ')'], ""));
        return Some(std::env::temp_dir().join(name));
    }
    files::places::config_file("remote-approved")
}

/// What a connection is allowed as: everything it does but its secret (server, user, folders,
/// mode, the certificates trusted), so a changed file asks again.
pub fn fingerprint(saved: &Saved) -> String {
    let text = toml::to_string(&Entry::from(saved)).unwrap_or_default();
    let hash = text
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

fn approval_line(saved: &Saved) -> String {
    format!("{}\t{}", saved.local.display().to_string().escape_default(), fingerprint(saved))
}

/// Whether `saved` may run: one kept in the user's settings always (the user made it), a
/// project's own once allowed as it is now.
pub fn approved(saved: &Saved) -> bool {
    if saved.connection.saved_in == SavedIn::Global {
        return true;
    }
    let Some(file) = approvals_file() else { return false };
    let line = approval_line(saved);
    std::fs::read_to_string(file).is_ok_and(|text| text.lines().any(|l| l == line))
}

/// Allows `saved` to run from now on (as it is now).
pub fn approve(saved: &Saved) {
    if saved.connection.saved_in == SavedIn::Global {
        return;
    }
    let Some(file) = approvals_file() else { return };
    let line = approval_line(saved);
    let prefix = format!("{}\t", saved.local.display().to_string().escape_default());
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.starts_with(&prefix)).collect();
    lines.push(&line);
    let text = lines.join("\n") + "\n";
    if let Err(e) = files::write_private(&file, text.as_bytes()) {
        tracing::warn!("can't keep the allowed remote connection: {e}");
    }
}

/// The keychain's service for remote projects' secrets (named after the app).
fn keychain() -> secrets::Keychain {
    secrets::Keychain { service: format!("{}.remote", kernel::brand::ID) }
}

/// Where `saved`'s secret is kept.
pub fn secret_store(saved: &Saved) -> Arc<dyn SecretStore> {
    match saved.connection.secrets_in {
        SecretsIn::Keychain => Arc::new(keychain()),
        SecretsIn::ProjectEncrypted => Arc::new(secrets::ProjectVault {
            file: data_dir(&saved.local).join("remote-secrets"),
            keys: Arc::new(keychain()),
            key_name: format!("vault:{}", saved.connection.id),
        }),
    }
}

#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
