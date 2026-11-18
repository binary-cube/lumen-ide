//! Remote projects (FTP, FTPS, SFTP): a project on a server worked on through a local copy —
//! the whole project (`Mode::Mirror`), or only what's opened (`Mode::OnDemand`) — kept in
//! step with the server: what changed on either side since the last transfer is found
//! ([`application::plan`]), reviewed (a file changed on both sides is a conflict: the user
//! keeps theirs, takes the server's or skips it) and applied, each transfer logged.
//!
//! The server and the local copy are ports ([`application::RemoteFs`],
//! [`application::LocalFiles`]); protocols and the disk live in infra.

pub mod application;
pub mod events;
pub mod manifest;
pub mod outbox;
pub mod ownership;
pub mod properties;

/// How a server is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Sftp,
    Ftp,
    /// FTP over TLS: `implicit` (port 990, TLS from the start) or explicit (`AUTH TLS`).
    Ftps {
        implicit: bool,
    },
}

impl Protocol {
    pub fn default_port(self) -> u16 {
        match self {
            Self::Sftp => 22,
            Self::Ftp | Self::Ftps { implicit: false } => 21,
            Self::Ftps { implicit: true } => 990,
        }
    }

    /// Its id in the saved connections.
    pub fn id(self) -> &'static str {
        match self {
            Self::Sftp => "sftp",
            Self::Ftp => "ftp",
            Self::Ftps { implicit: false } => "ftps",
            Self::Ftps { implicit: true } => "ftps-implicit",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "sftp" => Self::Sftp,
            "ftp" => Self::Ftp,
            "ftps" => Self::Ftps { implicit: false },
            "ftps-implicit" => Self::Ftps { implicit: true },
            _ => return None,
        })
    }
}

/// How the user signs in (secrets are in the system's keychain, never in files).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Auth {
    Password,
    /// A private key file (its passphrase, if any, in the keychain).
    Key {
        path: std::path::PathBuf,
    },
    /// The running SSH agent.
    Agent,
}

/// How much of the project is local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The whole project (but the exclusions) is downloaded and kept in step: everything
    /// works offline (index, search, refactorings, local history).
    Mirror,
    /// Folders are listed and files downloaded when they're opened; saving uploads.
    OnDemand,
}

/// Where a connection's settings are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SavedIn {
    /// In the project's data folder: the connection goes with the project.
    Project,
    /// In the user's settings: available to every project.
    Global,
}

/// Where a connection's secrets (password, key passphrase) are kept — never in plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretsIn {
    /// The system's keychain (macOS Keychain, Windows Credential Manager, Secret Service).
    Keychain,
    /// In the project's data folder, encrypted (authenticated encryption); the key is in
    /// the system's keychain, so the file alone reveals nothing.
    ProjectEncrypted,
}

/// A password or passphrase: never printed, wiped from memory once dropped.
pub use kernel::Secret;

/// The secrets' port (by connection id), shared with the other contexts that sign in.
pub use kernel::SecretStore;

/// How failed transfers are tried again (a dropped network, a busy server): `attempts` in
/// all, waiting `delay` before the second, twice as long before each next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    pub attempts: u32,
    pub delay: std::time::Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self { attempts: 4, delay: std::time::Duration::from_secs(1) }
    }
}

impl Retry {
    /// The wait before attempt `n` (1-based; nothing before the first).
    pub fn wait_before(&self, n: u32) -> std::time::Duration {
        if n <= 1 { std::time::Duration::ZERO } else { self.delay * 2u32.saturating_pow(n - 2) }
    }
}

/// How many transfers of a project run at once: uploads, downloads (many servers limit the
/// connections of a user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub uploads: usize,
    pub downloads: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { uploads: 2, downloads: 4 }
    }
}

/// What every connection follows unless it says otherwise (the global settings, kept by
/// the module that shows them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Defaults {
    pub limits: Limits,
    pub retry: Retry,
}

/// A saved connection to a remote project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// A UUID v4, made once with the connection: names the local copy's folder.
    pub id: String,
    /// As the user named it.
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    /// The project's folder on the server (`/var/www/shop`).
    pub root: String,
    pub mode: Mode,
    /// What's never transferred, relative to the root with `/` (`vendor`, `storage/logs`,
    /// `.env`): a folder excludes everything in it; `*.log` every file named so, anywhere.
    pub excluded: Vec<String>,
    pub saved_in: SavedIn,
    pub secrets_in: SecretsIn,
    /// This connection's own retries (`None`: the global setting).
    pub retry: Option<Retry>,
    /// This connection's own uploads / downloads at once (`None`: the global setting).
    pub uploads: Option<usize>,
    pub downloads: Option<usize>,
    /// What's moved or deleted here is moved / deleted on the server too (deleting asks
    /// first); off by default — a deletion on the server is hard to take back.
    pub moves: bool,
    /// A save uploads at once (on by default); off, nothing goes by itself: saves, new
    /// files, moves and deletions wait in the list to upload until the user sends them.
    pub auto_upload: bool,
    /// Who the files written on the server belong to.
    pub write_as: WriteAs,
}

/// Who a file written on the server (saved, uploaded, new) belongs to: as the server makes
/// it (the account signed in), as the project's folder on the server, or a chosen owner and
/// group. Set after each one is written, where the protocol can (SFTP; FTP can't: the
/// server decides) and the server lets the account.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WriteAs {
    #[default]
    Server,
    /// The owner and the group of the project's folder on the server (read when connecting).
    Project,
    /// These (`None`: as the server makes it).
    Chosen { owner: Option<properties::Principal>, group: Option<properties::Principal> },
}

impl Connection {
    /// Its transfers at once: its own, else the global settings' (`global`).
    pub fn limits(&self, global: Limits) -> Limits {
        Limits {
            uploads: self.uploads.unwrap_or(global.uploads).max(1),
            downloads: self.downloads.unwrap_or(global.downloads).max(1),
        }
    }

    /// Its retries: its own, else the global settings' (`global`).
    pub fn retries(&self, global: Retry) -> Retry {
        self.retry.unwrap_or(global)
    }

    /// `path` (relative, `/`) is excluded: it or a folder above it is in the list, or it's
    /// the project's own data folder or git's (always: the connection, its manifest and
    /// secrets, a repository's config and hooks are never sent nor taken, whatever the list
    /// says). Names match whatever their case: on macOS and Windows `.GIT` is `.git`.
    pub fn is_excluded(&self, path: &str) -> bool {
        let path = path.to_lowercase();
        let name = path.rsplit('/').next().unwrap_or(&path);
        let under = |e: &str| {
            let e = e.trim_matches('/').to_lowercase();
            if let Some(suffix) = e.strip_prefix('*') {
                return !suffix.is_empty() && !suffix.contains('/') && name.ends_with(suffix);
            }
            path == e || path.strip_prefix(&e).is_some_and(|rest| rest.starts_with('/'))
        };
        ALWAYS_EXCLUDED.iter().any(|e| under(e)) || self.excluded.iter().any(|e| under(e))
    }

    /// `user@host:port/root`, for the user.
    pub fn address(&self) -> String {
        let port = if self.port == self.protocol.default_port() {
            String::new()
        } else {
            format!(":{}", self.port)
        };
        format!("{}@{}{port}{}", self.user, self.host, self.root)
    }
}

/// A file or folder on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    /// Relative to the project's root, with `/`.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since the Unix epoch (as the server says: compared only with itself).
    pub modified: i64,
}

/// A file of the local copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEntry {
    /// Relative to the project's root, with `/`.
    pub path: String,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: i64,
}

/// Why talking to the server failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// The server can't be reached (address, network, refused).
    Connect(String),
    /// Signing in failed (user, password, key).
    Auth(String),
    /// The server's key isn't the one known for it (a different server, or an attack).
    HostKey(String),
    NotFound(String),
    /// The server refused (permissions) or failed.
    Server(String),
    TimedOut,
    /// Stopped by the user, mid-way (nothing half-written is left in place).
    Cancelled,
}

impl RemoteError {
    /// Worth trying again (the network, a busy server, a session that dropped); not a wrong
    /// password, a changed server key or a missing file.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Connect(_) | Self::TimedOut)
    }

    /// The same failure, saying what was being done when it happened (`listing app/Models`),
    /// before the server's own words.
    pub fn during(self, what: &str) -> Self {
        match self {
            Self::Connect(why) => Self::Connect(format!("{what}: {why}")),
            Self::Server(why) => Self::Server(format!("{what}: {why}")),
            Self::TimedOut => Self::Connect(format!("{what}: timed out")),
            other => other,
        }
    }
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(why) => write!(f, "can't connect: {why}"),
            Self::Auth(why) => write!(f, "sign-in failed: {why}"),
            Self::HostKey(why) => write!(f, "the server's key changed: {why}"),
            Self::NotFound(path) => write!(f, "not found: {path}"),
            Self::Server(why) => write!(f, "the server refused: {why}"),
            Self::TimedOut => f.write_str("timed out"),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for RemoteError {}

/// The folders never sent nor taken: the project's data (the connection, its secrets) and
/// git's (a repository's config and hooks run programs).
const ALWAYS_EXCLUDED: [&str; 2] = [kernel::brand::PROJECT_DIR, ".git"];

/// Whether a name a server listed can be a file of the local copy: one plain name, never a
/// way out of its folder (`.`, `..`, separators — `\\` is one on Windows —, a drive's `:`, a
/// control character). A server could list anything; the others are left out. On Windows
/// also none the system would read as another (`.git.` and `.git ` are `.git`, `GIT~1` may
/// be, `CON` / `nul.txt` are devices).
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.chars().any(|c| matches!(c, '/' | '\\' | ':') || c.is_control())
        && (!cfg!(windows) || is_plain_on_windows(name))
}

/// Windows' own rules for a name: no trailing dot or space, no short name (`GIT~1`), no
/// device (`CON`, `PRN`, `AUX`, `NUL`, `COM1`…`LPT9`, with any extension).
fn is_plain_on_windows(name: &str) -> bool {
    if name.ends_with(['.', ' '])
        || name.chars().any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end().to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    let short = stem
        .split_once('~')
        .is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    !device && !short
}

/// Whether a path relative to the project (with `/`) stays in it: each part a plain name.
pub fn is_inside(relative: &str) -> bool {
    relative.split('/').filter(|p| !p.is_empty()).all(is_plain_name)
}

/// A 64-bit FNV-1a hash of a file's bytes (changes found without keeping copies).
pub fn content_hash(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
