//! FTP and FTPS for remote projects: the `remote` domain's [`Connector`] / [`RemoteFs`]
//! over suppaftp, TLS by rustls (ring) — the same on macOS, Linux and Windows. Explicit
//! FTPS (`AUTH TLS` on port 21) or implicit (TLS from the start, port 990).
//!
//! A server's certificate is trusted when a public authority vouches for it (Mozilla's
//! roots, bundled: the same everywhere, nothing slow to read from the system), or when its
//! fingerprint is one the user accepted (servers often sign their own); otherwise the
//! connection is refused with the fingerprint, for the user to decide.

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use remote::application::{Connector, Moved, RemoteFs};
use remote::properties::{Algo, Capabilities, Properties, SetProperties};
use remote::{RemoteEntry, RemoteError};
use suppaftp::rustls::client::WebPkiServerVerifier;
use suppaftp::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use suppaftp::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use suppaftp::rustls::{self, ClientConfig, DigitallySignedStruct, SignatureScheme};
use suppaftp::types::FileType;
use suppaftp::{FtpError, Mode, RustlsConnector, RustlsFtpStream, Status};

/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Plain FTP, or FTPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Plain,
    /// `AUTH TLS` after connecting.
    Explicit,
    /// TLS from the start.
    Implicit,
}

#[derive(Clone)]
pub struct FtpConnector {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// From the keychain, in memory only.
    pub password: remote::Secret,
    pub security: Security,
    pub root: String,
    /// The certificate fingerprints the user accepted for this server.
    pub trusted: Vec<String>,
    /// Whether the server takes `SITE CHMOD` (`CHMOD_UNKNOWN` until it's first asked):
    /// learned once, shared by this connection's sessions.
    pub chmod: Arc<std::sync::atomic::AtomicU8>,
    /// What the server says it computes (`FEAT`: `HASH`'s algorithms, `XSHA256`…), asked
    /// once, shared by this connection's sessions.
    pub hashes: Arc<std::sync::OnceLock<Vec<String>>>,
    /// TLS 1.2 only: set once a server needs it (vsftpd's `require_ssl_reuse` can't resume
    /// TLS 1.3 sessions on data connections); shared by this connection's sessions.
    pub tls12: Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for FtpConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the password.
        f.debug_struct("FtpConnector")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .finish_non_exhaustive()
    }
}

/// The public authorities' verifier, built once per process.
fn roots(provider: &Arc<rustls::crypto::CryptoProvider>) -> Option<Arc<WebPkiServerVerifier>> {
    static ROOTS: std::sync::OnceLock<Arc<rustls::RootCertStore>> = std::sync::OnceLock::new();
    let store = ROOTS.get_or_init(|| {
        Arc::new(rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()))
    });
    WebPkiServerVerifier::builder_with_provider(Arc::clone(store), Arc::clone(provider))
        .build()
        .ok()
}

/// A certificate's fingerprint: `SHA256:` and its hash in hex.
pub fn fingerprint(cert: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, cert);
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("SHA256:{hex}")
}

/// The system's roots, else the user's accepted fingerprints.
#[derive(Debug)]
struct Verifier {
    roots: Option<Arc<WebPkiServerVerifier>>,
    trusted: Vec<String>,
    provider: Arc<rustls::crypto::CryptoProvider>,
    /// The certificate refused (its fingerprint), for the error.
    refused: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let by_roots = self
            .roots
            .as_ref()
            .map(|v| v.verify_server_cert(end_entity, intermediates, server_name, ocsp, now));
        if let Some(Ok(verified)) = by_roots {
            return Ok(verified);
        }
        let seen = fingerprint(end_entity.as_ref());
        if self.trusted.contains(&seen) {
            return Ok(ServerCertVerified::assertion());
        }
        if let Ok(mut refused) = self.refused.lock() {
            *refused = Some(seen);
        }
        Err(rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

impl FtpConnector {
    fn full(&self, path: &str) -> String {
        let root = self.root.trim_end_matches('/');
        if path.is_empty() { format!("{root}/") } else { format!("{root}/{path}") }
    }

    fn tls(&self, refused: &Arc<Mutex<Option<String>>>) -> Result<RustlsConnector, RemoteError> {
        let tls12 = self.tls12.load(std::sync::atomic::Ordering::Relaxed);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = roots(&provider);
        let verifier = Verifier {
            roots,
            trusted: self.trusted.clone(),
            provider: Arc::clone(&provider),
            refused: Arc::clone(refused),
        };
        let builder = ClientConfig::builder_with_provider(provider);
        let builder = if tls12 {
            builder.with_protocol_versions(&[&rustls::version::TLS12])
        } else {
            builder.with_safe_default_protocol_versions()
        };
        let config = builder
            .map_err(|e| RemoteError::Connect(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(RustlsConnector::from(Arc::new(config)))
    }

    fn open(&self) -> Result<RustlsFtpStream, RemoteError> {
        let refused = Arc::new(Mutex::new(None));
        let address = std::net::ToSocketAddrs::to_socket_addrs(&(self.host.as_str(), self.port))
            .map_err(|e| RemoteError::Connect(e.to_string()))?
            .next()
            .ok_or_else(|| RemoteError::Connect(format!("no address for {}", self.host)))?;
        let tls_error = |e: FtpError| match refused.lock().ok().and_then(|r| r.clone()) {
            Some(fingerprint) => RemoteError::HostKey(fingerprint),
            None => connect_error(e),
        };
        let mut ftp = match self.security {
            Security::Implicit => {
                RustlsFtpStream::connect_secure_implicit(address, self.tls(&refused)?, &self.host)
                    .map_err(tls_error)?
            }
            Security::Explicit => RustlsFtpStream::connect_timeout(address, CONNECT_TIMEOUT)
                .map_err(connect_error)?
                .into_secure(self.tls(&refused)?, &self.host)
                .map_err(tls_error)?,
            Security::Plain => {
                RustlsFtpStream::connect_timeout(address, CONNECT_TIMEOUT).map_err(connect_error)?
            }
        };
        ftp.login(self.user.as_str(), self.password.expose())
            .map_err(|e| RemoteError::Auth(e.to_string()))?;
        ftp.set_mode(Mode::Passive);
        // Servers behind NAT announce a private address for data: the control one is used.
        ftp.set_passive_nat_workaround(true);
        ftp.transfer_type(FileType::Binary).map_err(server_error)?;
        Ok(ftp)
    }
}

fn connect_error(e: FtpError) -> RemoteError {
    RemoteError::Connect(e.to_string())
}

fn server_error(e: FtpError) -> RemoteError {
    RemoteError::Server(e.to_string())
}

/// "Not there" (550), told apart from other failures.
fn is_missing(e: &FtpError) -> bool {
    matches!(e, FtpError::UnexpectedResponse(r) if r.status == Status::FileUnavailable)
}

impl Connector for FtpConnector {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        Ok(Box::new(FtpFs { ftp: self.open()?, connector: self.clone(), mlst: true }))
    }
}

/// `SITE CHMOD` not asked yet, taken, refused.
pub const CHMOD_UNKNOWN: u8 = 0;
const CHMOD_YES: u8 = 1;
const CHMOD_NO: u8 = 2;

/// What a 522 (the TLS session not reused on a data connection) is reported as.
const REUSE: &str = "TLS session reuse required";

pub struct FtpFs {
    ftp: RustlsFtpStream,
    connector: FtpConnector,
    /// `MLST` may be asked (cleared once the server refuses it: `LIST` from then on).
    mlst: bool,
}

impl std::fmt::Debug for FtpFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FtpFs").field("root", &self.connector.root).finish_non_exhaustive()
    }
}

fn seconds(time: std::time::SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl FtpFs {
    fn full(&self, path: &str) -> String {
        self.connector.full(path)
    }

    /// A failure as the domain says it. A server needing the TLS session reused on data
    /// connections (vsftpd's `require_ssl_reuse`, which TLS 1.3 can't do here) switches the
    /// connection to TLS 1.2: the transfer is tried again on a new session.
    fn fail(&self, e: FtpError) -> RemoteError {
        let text = e.to_string();
        if text.contains("522") && self.connector.security != Security::Plain {
            self.connector.tls12.store(true, std::sync::atomic::Ordering::Relaxed);
            return RemoteError::Connect(format!("{REUSE}: {text}"));
        }
        match e {
            FtpError::ConnectionError(_) => RemoteError::Connect(text),
            _ => RemoteError::Server(text),
        }
    }

    /// `op`, done again once on a new control session when a data connection couldn't reuse
    /// the TLS session (522: the server ties each data connection to the control one's
    /// session, which a data connection that didn't resume breaks for the next ones).
    fn again<T>(
        &mut self,
        mut op: impl FnMut(&mut Self) -> Result<T, RemoteError>,
    ) -> Result<T, RemoteError> {
        match op(self) {
            Err(RemoteError::Connect(why)) if why.starts_with(REUSE) => {
                self.ftp = self.connector.open()?;
                op(self)
            }
            other => other,
        }
    }

    /// A folder's entries: `MLSD` (exact), else `LIST` (POSIX / DOS listings).
    fn entries(&mut self, dir: &str) -> Result<Vec<suppaftp::list::File>, RemoteError> {
        let full = self.full(dir);
        match self.ftp.mlsd(Some(&full)) {
            Ok(lines) => Ok(lines
                .iter()
                .filter_map(|l| suppaftp::list::ListParser::parse_mlsd(l).ok())
                .collect()),
            Err(e) if is_missing(&e) => Err(RemoteError::NotFound(dir.to_owned())),
            Err(_) => {
                let listed = self.ftp.list(Some(&full));
                let lines = listed.map_err(|e| {
                    if is_missing(&e) {
                        RemoteError::NotFound(dir.to_owned())
                    } else {
                        self.fail(e)
                    }
                })?;
                // Some servers (vsftpd) list a folder that isn't there as empty: told apart
                // (else a wrong folder would look like an empty project).
                if lines.is_empty() && self.ftp.cwd(&full).is_err() {
                    return Err(RemoteError::NotFound(dir.to_owned()));
                }
                Ok(lines
                    .iter()
                    .filter_map(|l| {
                        suppaftp::list::ListParser::parse_posix(l)
                            .or_else(|_| suppaftp::list::ListParser::parse_dos(l))
                            .ok()
                    })
                    .collect())
            }
        }
    }

    /// The server's features (`FEAT`'s lines, upper-cased), asked once per connection.
    fn features(&mut self) -> Vec<String> {
        if let Some(known) = self.connector.hashes.get() {
            return known.clone();
        }
        let features: Vec<String> = self
            .ftp
            .feat()
            .map(|f| {
                f.into_iter()
                    .map(|(k, v)| match v {
                        Some(v) => format!("{} {}", k.to_ascii_uppercase(), v.to_ascii_uppercase()),
                        None => k.to_ascii_uppercase(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.connector.hashes.get_or_init(|| features).clone()
    }

    /// `path`'s properties by `MLST` (one answer on the control connection, no data
    /// connection). `Err(None)`: the server doesn't know `MLST` (or its facts say nothing).
    fn mlst_properties(&mut self, path: &str) -> Result<Option<Properties>, Option<RemoteError>> {
        let full = self.full(path);
        match self.ftp.mlst(Some(&full)) {
            Ok(line) => remote::properties::parse_facts(&line)
                .and_then(|(facts, _)| remote::properties::facts_entry(path, &facts))
                .map(Some)
                .ok_or(None),
            Err(e) if is_missing(&e) => Ok(None),
            Err(FtpError::UnexpectedResponse(r)) if r.status.code() / 100 == 5 => Err(None),
            Err(FtpError::BadResponse) => Err(None),
            Err(e) => Err(Some(self.fail(e))),
        }
    }

    /// Folder `dir`'s (relative) entries by one `LIST`, by name: what any listing says
    /// (POSIX or DOS) and, from an `ls -l` line, the mode, the owner and the group.
    fn listed_properties(
        &mut self,
        dir: &str,
    ) -> Result<std::collections::HashMap<String, Properties>, RemoteError> {
        use suppaftp::list::ListParser;
        let full = self.full(dir);
        let lines = match self.again(|fs| match fs.ftp.list(Some(&full)) {
            Ok(lines) => Ok(lines),
            Err(e) if is_missing(&e) => Err(RemoteError::NotFound(full.clone())),
            Err(e) => Err(fs.fail(e)),
        }) {
            Ok(lines) => lines,
            // The folder is gone: nothing of it there.
            Err(RemoteError::NotFound(_)) => return Ok(Default::default()),
            Err(e) => return Err(e),
        };
        let mut found = std::collections::HashMap::with_capacity(lines.len());
        for line in &lines {
            let Ok(f) = ListParser::parse_posix(line).or_else(|_| ListParser::parse_dos(line))
            else {
                continue;
            };
            let path =
                if dir.is_empty() { f.name().to_owned() } else { format!("{dir}/{}", f.name()) };
            let entry = RemoteEntry {
                path,
                is_dir: f.is_directory(),
                size: f.size() as u64,
                modified: seconds(f.modified()),
            };
            let mut props = Properties::of(&entry);
            if let Some(ls) = remote::properties::parse_ls_line(line) {
                props.mode = Some(ls.mode);
                props = props.with_listed(&ls.owner, &ls.group);
            }
            found.insert(f.name().to_owned(), props);
        }
        Ok(found)
    }

    /// Creates `path`'s missing folders.
    fn make_parents(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').collect();
        let mut dir = String::new();
        for part in &parts[..parts.len().saturating_sub(1)] {
            dir = if dir.is_empty() { (*part).to_owned() } else { format!("{dir}/{part}") };
            // Already there: refused, fine.
            let _ = self.ftp.mkdir(self.full(&dir));
        }
    }
}

/// Bytes per read: progress told, a stop checked, between them.
const CHUNK: usize = 64 * 1024;

/// A reader telling `moved` the bytes read through it; stopped (an error) when it answers
/// `false`.
struct Counted<'a, R> {
    inner: R,
    moved: Moved<'a>,
    stopped: bool,
}

impl<R: std::io::Read> std::io::Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 && !(self.moved)(n as u64) {
            self.stopped = true;
            return Err(std::io::Error::other("cancelled"));
        }
        Ok(n)
    }
}

impl<R> Counted<'_, R> {
    /// `from`'s bytes into `out`, told as they come.
    fn copy_from(
        &mut self,
        from: &mut dyn std::io::Read,
        out: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        let mut chunk = vec![0; CHUNK];
        loop {
            let n = from.read(&mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            out.extend_from_slice(&chunk[..n]);
            if !(self.moved)(n as u64) {
                self.stopped = true;
                return Err(std::io::Error::other("cancelled"));
            }
        }
    }
}

/// A suffix no other upload of this process (nor another process's) shares.
fn unique() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}.tmp", std::process::id())
}

impl RemoteFs for FtpFs {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        Ok(self
            .again(|fs| fs.entries(dir))?
            .into_iter()
            // A name that would lead out of the folder: left out.
            .filter(|f| remote::is_plain_name(f.name()))
            .map(|f| RemoteEntry {
                path: if dir.is_empty() {
                    f.name().to_owned()
                } else {
                    format!("{dir}/{}", f.name())
                },
                is_dir: f.is_directory(),
                size: f.size() as u64,
                modified: seconds(f.modified()),
            })
            .collect())
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        self.read_moving(path, &mut |_| true)
    }

    fn read_moving(&mut self, path: &str, moved: Moved<'_>) -> Result<Vec<u8>, RemoteError> {
        let full = self.full(path);
        let mut stopped = false;
        let read = self.again(|fs| {
            let mut counted =
                Counted { inner: std::io::empty(), moved: &mut *moved, stopped: false };
            let outcome = fs.ftp.retr(&full, |stream| {
                let mut data = Vec::new();
                counted
                    .copy_from(stream, &mut data)
                    .map_err(suppaftp::FtpError::ConnectionError)?;
                Ok(data)
            });
            stopped = counted.stopped;
            match outcome {
                Ok(data) => Ok(data),
                Err(_) if stopped => Err(RemoteError::Cancelled),
                Err(e) if is_missing(&e) => Err(RemoteError::NotFound(path.to_owned())),
                Err(e) => Err(fs.fail(e)),
            }
        });
        if stopped { Err(RemoteError::Cancelled) } else { read }
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
        self.make_parents(path);
        // Written beside it, then put in its place: a dropped connection (or a stop) never
        // leaves half a file.
        let full = self.full(path);
        // A name of its own: two uploads of the same file never share it.
        let temp = format!("{full}.upload-{}", unique());
        let mut stopped = false;
        let sent = self.again(|fs| {
            let mut counted =
                Counted { inner: Cursor::new(data), moved: &mut *moved, stopped: false };
            let sent = fs.ftp.put_file(&temp, &mut counted).map(|_| ()).map_err(|e| fs.fail(e));
            stopped = counted.stopped;
            sent
        });
        if stopped {
            let _ = self.ftp.rm(&temp);
            return Err(RemoteError::Cancelled);
        }
        sent?;
        if self.ftp.rename(&temp, &full).is_err() {
            // A server that doesn't replace: the old one is moved aside first, put back if the
            // new one can't take its place (the server's file is never lost), then removed.
            let aside = format!("{full}.replaced-{}", unique());
            let moved_aside = self.ftp.rename(&full, &aside).is_ok();
            if let Err(e) = self.ftp.rename(&temp, &full) {
                if moved_aside {
                    let _ = self.ftp.rename(&aside, &full);
                }
                let _ = self.ftp.rm(&temp);
                return Err(self.fail(e));
            }
            if moved_aside {
                let _ = self.ftp.rm(&aside);
            }
        }
        self.stat(path)?.ok_or_else(|| RemoteError::NotFound(path.to_owned()))
    }

    fn remove(&mut self, path: &str) -> Result<(), RemoteError> {
        let full = self.full(path);
        match self.ftp.rm(&full) {
            Ok(()) => Ok(()),
            // A folder.
            Err(_) => self.ftp.rmdir(&full).map_err(|e| self.fail(e)),
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError> {
        self.make_parents(to);
        let (f, t) = (self.full(from), self.full(to));
        self.ftp.rename(&f, &t).map_err(|e| self.fail(e))
    }

    fn home(&mut self) -> Result<String, RemoteError> {
        self.ftp.pwd().map_err(|e| self.fail(e))
    }

    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let entries = match self.again(|fs| fs.entries(dir)) {
            Ok(entries) => entries,
            Err(RemoteError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        Ok(entries.into_iter().find(|f| f.name() == name).map(|f| RemoteEntry {
            path: path.to_owned(),
            is_dir: f.is_directory(),
            size: f.size() as u64,
            modified: seconds(f.modified()),
        }))
    }

    /// Computed by the server: `HASH` (its draft, the algorithm chosen by `OPTS HASH`) where
    /// `FEAT` names it, else `XSHA512` / `XSHA256` / `XSHA1` / `XMD5`; `None`: it can't.
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        let full = self.full(path);
        if full.contains(['\r', '\n']) {
            return Ok(None);
        }
        let features = self.features();
        let name = algo.label();
        let ok = [Status::File, Status::RequestedFileActionOk, Status::CommandOk];
        if features.iter().any(|f| f == "HASH" || (f.starts_with("HASH ") && f.contains(name))) {
            let chosen = self.ftp.custom_command(format!("OPTS HASH {name}"), &[Status::CommandOk]);
            if chosen.is_ok()
                && let Ok(reply) = self.ftp.custom_command(format!("HASH {full}"), &ok)
            {
                return Ok(algo.digest_in(&String::from_utf8_lossy(&reply.body)));
            }
        }
        let x = match algo {
            Algo::Sha512 => "XSHA512",
            Algo::Sha256 => "XSHA256",
            Algo::Sha1 => "XSHA1",
            Algo::Md5 => "XMD5",
            Algo::Sha384 | Algo::Sha224 => return Ok(None),
        };
        if features.iter().any(|f| f == x)
            && let Ok(reply) = self.ftp.custom_command(format!("{x} {full}"), &ok)
        {
            return Ok(algo.digest_in(&String::from_utf8_lossy(&reply.body)));
        }
        Ok(None)
    }

    /// `MLST` per file where the server knows it (ProFTPD, Pure-FTPd, FileZilla Server);
    /// else one `LIST` per folder, its `ls -l` lines read (vsftpd).
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        let mut out = vec![None; paths.len()];
        let mut listed: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        for (i, path) in paths.iter().enumerate() {
            if self.mlst {
                match self.mlst_properties(path) {
                    Ok(props) => {
                        out[i] = props;
                        continue;
                    }
                    Err(Some(e)) => return Err(e),
                    Err(None) => self.mlst = false,
                }
            }
            let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
            listed.entry(dir.to_owned()).or_default().push(i);
        }
        for (dir, chosen) in listed {
            let mut found = self.listed_properties(&dir)?;
            for i in chosen {
                let name = paths[i].rsplit('/').next().unwrap_or(&paths[i]);
                out[i] = found.remove(name);
            }
        }
        Ok(out)
    }

    /// `MLSD`'s facts (the mode, the owner) where the server has it, else `LIST`'s `ls -l`
    /// lines: one listing.
    fn list_properties(&mut self, dir: &str) -> Result<Vec<Properties>, RemoteError> {
        let full = self.full(dir);
        let child =
            |name: &str| if dir.is_empty() { name.to_owned() } else { format!("{dir}/{name}") };
        if self.mlst
            && let Ok(lines) = self.ftp.mlsd(Some(&full))
        {
            return Ok(lines
                .iter()
                .filter_map(|l| remote::properties::parse_facts(l))
                .filter(|(facts, name)| {
                    !matches!(facts.get("type").map(String::as_str), Some("cdir" | "pdir"))
                        && remote::is_plain_name(name)
                })
                .filter_map(|(facts, name)| remote::properties::facts_entry(&child(&name), &facts))
                .collect());
        }
        Ok(self
            .listed_properties(dir)?
            .into_iter()
            .filter(|(name, _)| remote::is_plain_name(name))
            .map(|(_, props)| props)
            .collect())
    }

    /// FTP changes modes by `SITE CHMOD` where the server has it (until it's refused it
    /// once); owners and groups are only shown.
    fn capabilities(&mut self) -> Capabilities {
        let chmod = self.connector.chmod.load(std::sync::atomic::Ordering::Relaxed) != CHMOD_NO;
        Capabilities { chmod, ..Capabilities::default() }
    }

    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        use std::sync::atomic::Ordering::Relaxed;
        if set.owner.is_some() || set.group.is_some() {
            return Err(RemoteError::Server("FTP can't change owners or groups".into()));
        }
        let Some(mode) = set.mode else { return Ok(()) };
        if self.connector.chmod.load(Relaxed) == CHMOD_NO {
            return Err(RemoteError::Server("the server doesn't take SITE CHMOD".into()));
        }
        let full = self.full(path);
        // One line on the control connection: a line break would be a second command.
        if full.contains(['\r', '\n']) {
            return Err(RemoteError::Server(format!("{path}: not a plain name")));
        }
        match self.ftp.site(format!("CHMOD {} {full}", mode.octal())) {
            Ok(_) => {
                self.connector.chmod.store(CHMOD_YES, Relaxed);
                Ok(())
            }
            Err(e) if is_missing(&e) => Err(RemoteError::NotFound(path.to_owned())),
            // Not a command it knows (500 / 502 / 504): never asked again.
            Err(FtpError::UnexpectedResponse(r))
                if matches!(r.status.code(), 500 | 502 | 504)
                    && self.connector.chmod.load(Relaxed) != CHMOD_YES =>
            {
                self.connector.chmod.store(CHMOD_NO, Relaxed);
                Err(RemoteError::Server("the server doesn't take SITE CHMOD".into()))
            }
            Err(e) => Err(self.fail(e)),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
