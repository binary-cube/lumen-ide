//! SFTP for remote projects: the `remote` domain's [`Connector`] / [`RemoteFs`] over SSH, in
//! pure Rust (russh, ring) — the same on macOS, Linux and Windows. A session keeps its own
//! single-threaded runtime and offers blocking calls (each transfer worker has one).
//!
//! The server's key is checked against `known_hosts` (the user's `~/.ssh/known_hosts` and
//! our own file): a changed key is always refused; an unknown one is refused with its
//! fingerprint until the user accepts it (then it's learned into our file only).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use remote::application::{Connector, Moved, RemoteFs};
use remote::properties::{Accounts, Algo, Capabilities, Principal, Properties, SetProperties};
use remote::{RemoteEntry, RemoteError};
use russh::client;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey};
use russh_sftp::client::SftpSession;

/// How long connecting (and signing in) may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How the user signs in (secrets come from the keychain, given here in memory only).
#[derive(Clone)]
pub enum Credentials {
    Password(remote::Secret),
    Key { path: PathBuf, passphrase: Option<remote::Secret> },
    Agent,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the secrets.
        f.write_str(match self {
            Self::Password(_) => "Password(…)",
            Self::Key { .. } => "Key(…)",
            Self::Agent => "Agent",
        })
    }
}

/// Which server keys are trusted.
#[derive(Debug, Clone)]
pub struct HostKeys {
    /// Checked, in order (`~/.ssh/known_hosts`, ours).
    pub known: Vec<PathBuf>,
    /// Where a key the user accepted is learned.
    pub learn_into: PathBuf,
    /// The key the user accepted for this server (its fingerprint), learned on connecting.
    pub accepted: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SftpConnector {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub credentials: Credentials,
    /// The project's folder on the server.
    pub root: String,
    pub host_keys: HostKeys,
}

/// What the key check found (read after connecting).
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyCheck {
    Known,
    /// Not known: its fingerprint, for the user.
    Unknown(String),
    /// Different from the recorded one.
    Changed(String),
}

struct Handler {
    host: String,
    port: u16,
    keys: HostKeys,
    seen: Arc<Mutex<Option<KeyCheck>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        offered: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match offered {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            // A certificate: its key is the one checked.
            russh::keys::PublicKeyOrCertificate::Certificate(cert) => {
                PublicKey::new(cert.public_key().clone(), "")
            }
        };
        let key = &key;
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let mut check = KeyCheck::Unknown(fingerprint.clone());
        for file in self.keys.known.iter().filter(|f| f.exists()) {
            match russh::keys::check_known_hosts_path(&self.host, self.port, key, file) {
                Ok(true) => {
                    check = KeyCheck::Known;
                    break;
                }
                Ok(false) => {}
                Err(russh::keys::Error::KeyChanged { .. }) => {
                    check = KeyCheck::Changed(fingerprint);
                    break;
                }
                Err(_) => {}
            }
        }
        // Accepted by the user just now: learned (into our file only).
        if let KeyCheck::Unknown(seen) = &check
            && self.keys.accepted.as_deref() == Some(seen.as_str())
        {
            if let Some(dir) = self.keys.learn_into.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = russh::keys::known_hosts::learn_known_hosts_path(
                &self.host,
                self.port,
                key,
                &self.keys.learn_into,
            );
            check = KeyCheck::Known;
        }
        let ok = check == KeyCheck::Known;
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(check);
        }
        Ok(ok)
    }
}

/// An open SFTP session.
pub struct SftpFs {
    runtime: tokio::runtime::Runtime,
    sftp: SftpSession,
    /// Kept open with the session (a second channel opens on it when needed).
    ssh: client::Handle<Handler>,
    /// A raw SFTP channel, opened the first time names are needed: its listings keep the
    /// long names (`ls -l`), the only place SFTP v3 tells the owner's and group's names.
    raw: Option<russh_sftp::client::RawSftpSession>,
    root: String,
    /// The last file written that kept a setuid / setgid mode (its path, its mode): a change
    /// of its owner after (the kernel clears those bits) puts the mode back.
    special: Option<(String, u32)>,
}

impl std::fmt::Debug for SftpFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpFs").field("root", &self.root).finish_non_exhaustive()
    }
}

impl SftpConnector {
    fn full(root: &str, path: &str) -> String {
        let root = root.trim_end_matches('/');
        match (root.is_empty(), path.is_empty()) {
            // The server's root itself.
            (true, true) => "/".to_owned(),
            (_, true) => root.to_owned(),
            _ => format!("{root}/{path}"),
        }
    }

    async fn open(&self) -> Result<(client::Handle<Handler>, SftpSession), RemoteError> {
        let seen = Arc::new(Mutex::new(None));
        let handler = Handler {
            host: self.host.clone(),
            port: self.port,
            keys: self.host_keys.clone(),
            seen: Arc::clone(&seen),
        };
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(300)),
            ..Default::default()
        });
        let connected = client::connect(config, (self.host.as_str(), self.port), handler).await;
        let mut ssh = match connected {
            Ok(ssh) => ssh,
            Err(e) => {
                // A refused key says why.
                return Err(match seen.lock().ok().and_then(|s| s.clone()) {
                    Some(KeyCheck::Changed(f)) => RemoteError::HostKey(format!("changed: {f}")),
                    Some(KeyCheck::Unknown(f)) => RemoteError::HostKey(f),
                    _ => RemoteError::Connect(e.to_string()),
                });
            }
        };
        let signed_in = match &self.credentials {
            Credentials::Password(password) => {
                ssh.authenticate_password(&self.user, password.expose()).await
            }
            Credentials::Key { path, passphrase } => {
                let key = russh::keys::load_secret_key(
                    path,
                    passphrase.as_ref().map(remote::Secret::expose),
                )
                .map_err(|e| RemoteError::Auth(format!("the key: {e}")))?;
                let hash = ssh.best_supported_rsa_hash().await.ok().flatten().flatten();
                ssh.authenticate_publickey(
                    &self.user,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
            }
            Credentials::Agent => return self.agent_sign_in(ssh).await,
        };
        match signed_in {
            Ok(result) if result.success() => {}
            Ok(_) => return Err(RemoteError::Auth("refused".into())),
            Err(e) => return Err(RemoteError::Auth(e.to_string())),
        }
        let sftp = start_sftp(&ssh).await?;
        Ok((ssh, sftp))
    }

    /// Signs in with each of the agent's keys until one works.
    async fn agent_sign_in(
        &self,
        mut ssh: client::Handle<Handler>,
    ) -> Result<(client::Handle<Handler>, SftpSession), RemoteError> {
        let mut agent = agent().await?;
        let identities = agent
            .request_identities()
            .await
            .map_err(|e| RemoteError::Auth(format!("the agent: {e}")))?;
        let hash = ssh.best_supported_rsa_hash().await.ok().flatten().flatten();
        for identity in identities {
            let key = identity.public_key().into_owned();
            if let Ok(result) =
                ssh.authenticate_publickey_with(&self.user, key, hash, &mut agent).await
                && result.success()
            {
                let sftp = start_sftp(&ssh).await?;
                return Ok((ssh, sftp));
            }
        }
        Err(RemoteError::Auth("no key of the agent was accepted".into()))
    }
}

/// The SSH agent, whichever kind the system has.
type Agent = russh::keys::agent::client::AgentClient<
    Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>,
>;

#[cfg(unix)]
async fn agent() -> Result<Agent, RemoteError> {
    russh::keys::agent::client::AgentClient::connect_env()
        .await
        .map(|a| a.dynamic())
        .map_err(|e| RemoteError::Auth(format!("no SSH agent: {e}")))
}

/// Windows' own OpenSSH agent, else Pageant.
#[cfg(windows)]
async fn agent() -> Result<Agent, RemoteError> {
    use russh::keys::agent::client::AgentClient;
    if let Ok(agent) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        return Ok(agent.dynamic());
    }
    AgentClient::connect_pageant()
        .await
        .map(|a| a.dynamic())
        .map_err(|e| RemoteError::Auth(format!("no SSH agent (OpenSSH or Pageant): {e}")))
}

/// A channel with the SFTP subsystem started on it.
async fn sftp_channel(
    ssh: &client::Handle<Handler>,
) -> Result<russh::ChannelStream<client::Msg>, RemoteError> {
    let channel =
        ssh.channel_open_session().await.map_err(|e| RemoteError::Connect(e.to_string()))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| RemoteError::Server(format!("no SFTP: {e}")))?;
    Ok(channel.into_stream())
}

async fn start_sftp(ssh: &client::Handle<Handler>) -> Result<SftpSession, RemoteError> {
    SftpSession::new(sftp_channel(ssh).await?).await.map_err(|e| RemoteError::Server(e.to_string()))
}

/// A raw SFTP session on its own channel (its listings with their long names).
async fn start_raw(
    ssh: &client::Handle<Handler>,
) -> Result<russh_sftp::client::RawSftpSession, RemoteError> {
    let raw = russh_sftp::client::RawSftpSession::new(sftp_channel(ssh).await?);
    raw.init().await.map_err(|e| RemoteError::Server(e.to_string()))?;
    Ok(raw)
}

/// A command run on the server (an exec channel beside the SFTP one): its exit status
/// and what it printed (at most [`EXEC_MAX`] bytes). A server that gives no shell (an
/// SFTP-only account) refuses the channel or the command.
async fn exec(ssh: &client::Handle<Handler>, command: &str) -> Result<(u32, String), russh::Error> {
    let mut channel = ssh.channel_open_session().await?;
    channel.exec(true, command).await?;
    let (mut out, mut status) = (Vec::new(), None);
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } if out.len() < EXEC_MAX => {
                out.extend_from_slice(&data);
            }
            russh::ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
            _ => {}
        }
    }
    Ok((status.unwrap_or(1), String::from_utf8_lossy(&out).into_owned()))
}

/// What a command's output is read up to (a digest's line is short).
const EXEC_MAX: usize = 4096;

/// `text` as one word for a POSIX shell: in single quotes, a quote in it closed, escaped
/// and opened again.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The tools that compute `algo` on a POSIX server: coreutils', else Perl's `shasum`
/// (macOS, BSDs).
fn digest_commands(algo: remote::properties::Algo) -> [String; 2] {
    use remote::properties::Algo;
    let (sum, bits) = match algo {
        Algo::Sha512 => ("sha512sum", "512"),
        Algo::Sha384 => ("sha384sum", "384"),
        Algo::Sha256 => ("sha256sum", "256"),
        Algo::Sha224 => ("sha224sum", "224"),
        Algo::Sha1 => ("sha1sum", "1"),
        Algo::Md5 => return ["md5sum".into(), "md5 -q".into()],
    };
    [sum.into(), format!("shasum -a {bits}")]
}

/// The setuid and setgid bits: a change of owner or group clears them.
const SPECIAL_BITS: u32 = 0o6000;

/// The mode to set again after `set` on `path`: the one `special` (the last file written
/// with setuid / setgid) kept, when `set` changes its owner or group and not its mode.
fn mode_to_restore(special: Option<(String, u32)>, path: &str, set: &SetProperties) -> Option<u32> {
    let (written, mode) = special?;
    let owners = set.owner.is_some() || set.group.is_some();
    (written == path && owners && set.mode.is_none()).then_some(mode)
}

/// The most read of an accounts' file (`/etc/passwd`): a real one is a few KB.
const ACCOUNTS_MAX: u64 = 1024 * 1024;

impl Connector for SftpConnector {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|e| RemoteError::Connect(e.to_string()))?;
        let (ssh, sftp) = runtime.block_on(async {
            tokio::time::timeout(CONNECT_TIMEOUT, self.open())
                .await
                .map_err(|_| RemoteError::TimedOut)?
        })?;
        Ok(Box::new(SftpFs {
            runtime,
            sftp,
            ssh,
            raw: None,
            root: self.root.clone(),
            special: None,
        }))
    }
}

/// An SFTP failure, as the domain says it.
/// Bytes per read / write: progress told, a stop checked, between them.
const CHUNK: usize = 64 * 1024;

fn error(e: russh_sftp::client::error::Error, path: &str) -> RemoteError {
    use russh_sftp::client::error::Error;
    use russh_sftp::protocol::StatusCode;
    match e {
        Error::Status(status) if status.status_code == StatusCode::NoSuchFile => {
            RemoteError::NotFound(path.to_owned())
        }
        Error::Timeout => RemoteError::TimedOut,
        other => RemoteError::Server(other.to_string()),
    }
}

fn entry(path: String, attrs: &russh_sftp::protocol::FileAttributes) -> RemoteEntry {
    RemoteEntry {
        path,
        is_dir: attrs.is_dir(),
        size: attrs.size.unwrap_or(0),
        modified: i64::from(attrs.mtime.unwrap_or(0)),
    }
}

impl SftpFs {
    fn full(&self, path: &str) -> String {
        SftpConnector::full(&self.root, path)
    }

    /// Folder `dir`'s (a full path) entries as the raw channel lists them — each name, its
    /// long name (`ls -l`) and attributes —, those `wanted` only; read until `enough` of them
    /// are found (`None`: all of it).
    fn raw_listing(
        &mut self,
        dir: &str,
        wanted: &dyn Fn(&str) -> bool,
        enough: Option<usize>,
    ) -> Result<Vec<russh_sftp::protocol::File>, RemoteError> {
        use russh_sftp::client::error::Error;
        use russh_sftp::protocol::StatusCode;
        if self.raw.is_none() {
            self.raw = Some(self.runtime.block_on(start_raw(&self.ssh))?);
        }
        let Some(raw) = self.raw.as_ref() else { return Ok(Vec::new()) };
        let dir = dir.to_owned();
        self.runtime.block_on(async {
            let mut found = Vec::new();
            let handle = raw.opendir(dir.clone()).await.map_err(|e| error(e, &dir))?.handle;
            loop {
                match raw.readdir(handle.as_str()).await {
                    Ok(listed) => {
                        found.extend(listed.files.into_iter().filter(|f| wanted(&f.filename)));
                        // All found: the rest of a big folder isn't read.
                        if enough.is_some_and(|n| found.len() >= n) {
                            break;
                        }
                    }
                    Err(Error::Status(s)) if s.status_code == StatusCode::Eof => break,
                    Err(e) => {
                        let _ = raw.close(handle.as_str()).await;
                        return Err(error(e, &dir));
                    }
                }
            }
            let _ = raw.close(handle).await;
            Ok(found)
        })
    }

    /// The owner's and group's words (names, else ids) of `names` in folder `dir` (a full
    /// path), from its long names: one listing for all of them.
    fn listed_owners(
        &mut self,
        dir: &str,
        names: &[&str],
    ) -> Result<std::collections::HashMap<String, (Principal, Principal)>, RemoteError> {
        let files = self.raw_listing(dir, &|n| names.contains(&n), Some(names.len()))?;
        Ok(files
            .into_iter()
            .filter_map(|f| {
                let ls = remote::properties::parse_ls_line(&f.longname)?;
                Some((f.filename, (ls.owner, ls.group)))
            })
            .collect())
    }

    /// A small file outside the project (`/etc/passwd`); `None`: not there, not readable or
    /// too big.
    fn read_system(&mut self, path: &str) -> Option<String> {
        let size = self.runtime.block_on(self.sftp.metadata(path)).ok()?.size.unwrap_or(0);
        if size > ACCOUNTS_MAX {
            return None;
        }
        let bytes = self.runtime.block_on(self.sftp.read(path)).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `temp` (a full path) given `before`'s owner and group, then its permissions. Apart:
    /// an account that may not give a file away (only root may) still keeps the group when
    /// it's in it, and the mode. The mode last: a change of owner clears setuid / setgid.
    /// What the server refuses is left as it made it.
    fn keep_attributes(&mut self, temp: &str, before: &russh_sftp::protocol::FileAttributes) {
        let set = |uid: Option<u32>, gid: Option<u32>, mode: Option<u32>| {
            let mut attrs = russh_sftp::protocol::FileAttributes::empty();
            (attrs.uid, attrs.gid, attrs.permissions) = (uid, gid, mode);
            self.runtime.block_on(self.sftp.set_metadata(temp.to_owned(), attrs)).is_ok()
        };
        if (before.uid.is_some() || before.gid.is_some())
            && !set(before.uid, before.gid, None)
            && before.uid.is_some()
            && before.gid.is_some()
        {
            set(None, before.gid, None);
        }
        if let Some(mode) = before.permissions {
            set(None, None, Some(mode & 0o7777));
        }
    }

    /// The folders `path` goes in, made where missing: each new one as its parent is (its
    /// permissions — a `2775` keeps its group for what's made in it — its owner, its group).
    fn make_parents(&mut self, path: &str) -> Result<(), RemoteError> {
        let mut dir = String::new();
        let mut above = String::new();
        // The parent's attributes, read once (the first folder made), then passed down.
        let mut inherited: Option<Option<russh_sftp::protocol::FileAttributes>> = None;
        let parents: Vec<&str> = path.split('/').collect();
        for part in &parents[..parents.len().saturating_sub(1)] {
            dir = if dir.is_empty() { (*part).to_owned() } else { format!("{dir}/{part}") };
            let full = self.full(&dir);
            let exists = self.runtime.block_on(self.sftp.try_exists(full.clone())).unwrap_or(false);
            if !exists {
                if inherited.is_none() {
                    inherited = Some(self.attributes(&above).ok().flatten());
                }
                // Uploads run side by side: another one may have made it meanwhile (the
                // server then refuses with a bare "Failure"); there now, it's fine.
                if let Err(e) = self.runtime.block_on(self.sftp.create_dir(full.clone())) {
                    let there =
                        self.runtime.block_on(self.sftp.try_exists(full.clone())).unwrap_or(false);
                    if there {
                        above.clone_from(&dir);
                        continue;
                    }
                    return Err(error(e, &dir).during(&format!("making the folder /{dir}")));
                }
                if let Some(Some(parent)) = &inherited {
                    let parent = parent.clone();
                    self.keep_attributes(&full, &parent);
                }
            }
            above.clone_from(&dir);
        }
        Ok(())
    }
}

/// A suffix no other upload of this process (nor another process's) shares.
fn unique() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}.tmp", std::process::id())
}

impl RemoteFs for SftpFs {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        let full = self.full(dir);
        let read = self.runtime.block_on(self.sftp.read_dir(full)).map_err(|e| error(e, dir))?;
        Ok(read
            // A name that would lead out of the folder: left out.
            .filter(|e| remote::is_plain_name(&e.file_name()))
            .map(|e| {
                let path =
                    if dir.is_empty() { e.file_name() } else { format!("{dir}/{}", e.file_name()) };
                entry(path, &e.metadata())
            })
            .collect())
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        self.read_moving(path, &mut |_| true)
    }

    fn read_moving(&mut self, path: &str, moved: Moved<'_>) -> Result<Vec<u8>, RemoteError> {
        let full = self.full(path);
        let read = self.runtime.block_on(async {
            let mut file = self.sftp.open(full).await?;
            let mut data = Vec::new();
            let mut chunk = vec![0; CHUNK];
            loop {
                let n = tokio::io::AsyncReadExt::read(&mut file, &mut chunk).await?;
                if n == 0 {
                    return Ok(Some(data));
                }
                data.extend_from_slice(&chunk[..n]);
                if !moved(n as u64) {
                    return Ok(None);
                }
            }
        });
        match read.map_err(|e: russh_sftp::client::error::Error| error(e, path))? {
            Some(data) => Ok(data),
            None => Err(RemoteError::Cancelled),
        }
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError> {
        self.write_moving(path, data, &mut |_| true)
    }

    fn write_moving(
        &mut self,
        path: &str,
        data: &[u8],
        moved: Moved<'_>,
    ) -> Result<RemoteEntry, RemoteError> {
        self.make_parents(path)?;
        // What it replaces keeps who it belongs to and its permissions (the new file would
        // be the account's, with the server's default mode).
        let before = self.attributes(path).ok().flatten();
        // Written beside it, then put in its place: a dropped connection (or a stop) never
        // leaves half a file.
        let full = self.full(path);
        // A name of its own: two uploads of the same file never share it.
        let temp = format!("{full}.upload-{}", unique());
        let written = self
            .runtime
            .block_on(async {
                let mut file = self.sftp.create(temp.clone()).await?;
                for chunk in data.chunks(CHUNK) {
                    tokio::io::AsyncWriteExt::write_all(&mut file, chunk).await?;
                    if !moved(chunk.len() as u64) {
                        return Ok(false);
                    }
                }
                tokio::io::AsyncWriteExt::shutdown(&mut file).await?;
                Ok::<bool, russh_sftp::client::error::Error>(true)
            })
            .map_err(|e| error(e, path).during(&format!("writing /{path}")))?;
        if !written {
            let _ = self.runtime.block_on(self.sftp.remove_file(temp));
            return Err(RemoteError::Cancelled);
        }
        if let Some(before) = &before {
            self.keep_attributes(&temp, before);
        }
        self.special = before
            .as_ref()
            .and_then(|b| b.permissions)
            .map(|mode| mode & 0o7777)
            .filter(|mode| mode & SPECIAL_BITS != 0)
            .map(|mode| (path.to_owned(), mode));
        if self.runtime.block_on(self.sftp.rename(temp.clone(), full.clone())).is_err() {
            // SFTP v3 doesn't replace: the old one is moved aside first, put back if the new
            // one can't take its place (the server's file is never lost), then removed.
            let aside = format!("{full}.replaced-{}", unique());
            let moved_aside =
                self.runtime.block_on(self.sftp.rename(full.clone(), aside.clone())).is_ok();
            if let Err(e) = self.runtime.block_on(self.sftp.rename(temp.clone(), full.clone())) {
                if moved_aside {
                    let _ = self.runtime.block_on(self.sftp.rename(aside, full));
                }
                let _ = self.runtime.block_on(self.sftp.remove_file(temp));
                return Err(error(e, path).during(&format!("putting /{path} in place")));
            }
            if moved_aside {
                let _ = self.runtime.block_on(self.sftp.remove_file(aside));
            }
        }
        self.stat(path)?.ok_or_else(|| RemoteError::NotFound(path.to_owned()))
    }

    fn remove(&mut self, path: &str) -> Result<(), RemoteError> {
        let full = self.full(path);
        let is_dir = self.stat(path)?.is_some_and(|e| e.is_dir);
        let removed = if is_dir {
            self.runtime.block_on(self.sftp.remove_dir(full))
        } else {
            self.runtime.block_on(self.sftp.remove_file(full))
        };
        removed.map_err(|e| error(e, path))
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError> {
        self.make_parents(to)?;
        let (f, t) = (self.full(from), self.full(to));
        self.runtime.block_on(self.sftp.rename(f, t)).map_err(|e| error(e, from))
    }

    fn home(&mut self) -> Result<String, RemoteError> {
        self.runtime.block_on(self.sftp.canonicalize(".")).map_err(|e| error(e, "."))
    }

    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        Ok(self.attributes(path)?.map(|attrs| entry(path.to_owned(), &attrs)))
    }

    /// One raw listing: each entry's attributes (the mode, the ids) and long name (the
    /// owner's and group's names).
    fn list_properties(&mut self, dir: &str) -> Result<Vec<Properties>, RemoteError> {
        let full = self.full(dir);
        let files = self.raw_listing(&full, &remote::is_plain_name, None)?;
        Ok(files
            .into_iter()
            .map(|f| {
                let path = if dir.is_empty() {
                    f.filename.clone()
                } else {
                    format!("{dir}/{}", f.filename)
                };
                let mut props = Properties::of(&entry(path, &f.attrs));
                props.mode = f.attrs.permissions.map(remote::properties::Mode::new);
                props.owner = f.attrs.uid.map(Principal::id);
                props.group = f.attrs.gid.map(Principal::id);
                match remote::properties::parse_ls_line(&f.longname) {
                    Some(ls) => props.with_listed(&ls.owner, &ls.group),
                    None => props,
                }
            })
            .collect())
    }

    /// `stat` gives the mode and the ids; the names come from the folders' long names (one
    /// listing per folder, whatever the number of files chosen in it).
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            out.push(self.attributes(path)?.map(|attrs| {
                let mut props = Properties::of(&entry(path.clone(), &attrs));
                props.mode = attrs.permissions.map(remote::properties::Mode::new);
                props.owner = attrs.uid.map(Principal::id);
                props.group = attrs.gid.map(Principal::id);
                props
            }));
        }
        // The names, folder by folder (the root's own folder is outside the project: its
        // owner keeps its id).
        let mut by_dir: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        for (i, props) in out.iter().enumerate() {
            if props.is_some() {
                let full = self.full(&paths[i]);
                if let Some((dir, _)) = full.rsplit_once('/') {
                    by_dir
                        .entry(if dir.is_empty() { "/".into() } else { dir.to_owned() })
                        .or_default()
                        .push(i);
                }
            }
        }
        for (dir, chosen) in by_dir {
            let fulls: Vec<String> = chosen.iter().map(|&i| self.full(&paths[i])).collect();
            let names: Vec<&str> =
                fulls.iter().map(|f| f.rsplit_once('/').map_or(f.as_str(), |(_, n)| n)).collect();
            // A server without long names (or a folder that can't be listed): ids only.
            let Ok(listed) = self.listed_owners(&dir, &names) else { continue };
            for (&i, name) in chosen.iter().zip(&names) {
                if let Some((owner, group)) = listed.get(*name) {
                    out[i] = out[i].take().map(|props| props.with_listed(owner, group));
                }
            }
        }
        Ok(out)
    }

    /// The system's accounts files, when the server lets them be read (not in a chroot).
    fn accounts(&mut self) -> Result<Accounts, RemoteError> {
        let read = |fs: &mut Self, path| {
            fs.read_system(path).map(|t| remote::properties::parse_accounts(&t)).unwrap_or_default()
        };
        Ok(Accounts { users: read(self, "/etc/passwd"), groups: read(self, "/etc/group") })
    }

    /// Computed on the server by its tools (`sha256sum` …) when the account has a shell;
    /// `None` otherwise (an SFTP-only account, no such tool).
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        let file = shell_quote(&self.full(path));
        for tool in digest_commands(algo) {
            let command = format!("{tool} -- {file}");
            match self.runtime.block_on(exec(&self.ssh, &command)) {
                Ok((0, out)) => return Ok(algo.digest_in(&out)),
                // That tool isn't there (127) or failed: the next.
                Ok(_) => {}
                // No shell at all: computed here.
                Err(_) => return Ok(None),
            }
        }
        Ok(None)
    }

    /// `setstat`: the mode; the owner and the group (SFTP v3 sets them together: the one not
    /// changed is sent as it is).
    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        let mut attrs = russh_sftp::protocol::FileAttributes::empty();
        attrs.permissions = set.mode.map(remote::properties::Mode::bits);
        if set.owner.is_some() || set.group.is_some() {
            let now = if set.owner.is_none() || set.group.is_none() {
                self.attributes(path)?
            } else {
                None
            };
            attrs.uid = set.owner.or_else(|| now.as_ref().and_then(|a| a.uid));
            attrs.gid = set.group.or_else(|| now.as_ref().and_then(|a| a.gid));
        }
        let full = self.full(path);
        let restore = mode_to_restore(self.special.take(), path, set);
        self.runtime
            .block_on(self.sftp.set_metadata(full.clone(), attrs))
            .map_err(|e| error(e, path))?;
        if let Some(mode) = restore {
            let mut attrs = russh_sftp::protocol::FileAttributes::empty();
            attrs.permissions = Some(mode);
            self.runtime
                .block_on(self.sftp.set_metadata(full, attrs))
                .map_err(|e| error(e, path))?;
        }
        Ok(())
    }

    /// SFTP sets modes, owners and groups (the server refuses what the user may not).
    fn capabilities(&mut self) -> Capabilities {
        Capabilities { chmod: true, chown: true, chgrp: true, server_hashes: Vec::new() }
    }
}

impl SftpFs {
    /// `path`'s attributes (`None`: not there).
    fn attributes(
        &mut self,
        path: &str,
    ) -> Result<Option<russh_sftp::protocol::FileAttributes>, RemoteError> {
        let full = self.full(path);
        match self.runtime.block_on(self.sftp.metadata(full)) {
            Ok(attrs) => Ok(Some(attrs)),
            Err(e) => match error(e, path) {
                RemoteError::NotFound(_) => Ok(None),
                other => Err(other),
            },
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
