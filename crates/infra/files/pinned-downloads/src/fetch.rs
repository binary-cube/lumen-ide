//! What downloads come through: HTTPS only, only from the hosts their caller allows (every
//! redirect checked too), each download checked against its pinned SHA-256 or npm's SHA-512
//! integrity before anything uses it. A large one goes to a `.part` file on disk and is
//! resumed from where it stopped (`Range`); a small one stays in memory. A size cap stops a
//! server sending on forever.
//!
//! [`Fetch`] is the network ([`Http`]) or, in tests and snapshots, files in memory
//! ([`Memory`]): nothing here knows which.

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The most redirects followed for one download.
pub const MAX_REDIRECTS: usize = 5;

/// Whether `url` may be downloaded from: HTTPS, one of `hosts` (lowercase), no credentials,
/// no port.
pub fn allowed(url: &str, hosts: &[&str]) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', ':', '\\']) {
        return false;
    }
    hosts.contains(&authority.to_ascii_lowercase().as_str())
}

/// Why a download didn't happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The host can't be reached (no network, a DNS failure, a timeout).
    Offline(String),
    /// Not a URL anything may be downloaded from (or a redirect to one).
    Refused(String),
    /// The server answered with an error.
    Status(u16, String),
    /// Larger than it may be.
    TooLarge(String),
    /// What came isn't what's pinned.
    Mismatch(String),
    /// It broke off, or couldn't be written.
    Failed(String),
    /// Stopped by the user.
    Stopped,
}

impl FetchError {
    /// The network is the reason (said as such to the user).
    pub fn offline(&self) -> bool {
        matches!(self, Self::Offline(_))
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline(why) => write!(f, "no connection: {why}"),
            Self::Refused(url) => write!(f, "{url}: not a source downloads may come from"),
            Self::Status(code, url) => write!(f, "{url}: the server answered {code}"),
            Self::TooLarge(url) => write!(f, "{url}: larger than expected"),
            Self::Mismatch(url) => {
                write!(f, "{url}: the download doesn't match its pinned checksum")
            }
            Self::Failed(why) => f.write_str(why),
            Self::Stopped => f.write_str("stopped"),
        }
    }
}

impl From<FetchError> for String {
    fn from(e: FetchError) -> Self {
        e.to_string()
    }
}

/// A download's body, as it comes.
pub struct Body {
    /// The byte it starts at (0 when the server sends the whole file again, a resume asked
    /// for or not).
    pub from: u64,
    /// The whole file's size, when said.
    pub total: Option<u64>,
    pub reader: Box<dyn Read + Send>,
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Body").field("from", &self.from).field("total", &self.total).finish()
    }
}

/// What downloads (the network; a test's files in memory).
pub trait Fetch: Send + Sync {
    /// The file at `url`, from byte `from` on (if the source can: else from the start).
    fn open(&self, url: &str, from: u64) -> Result<Body, FetchError>;
}

/// The network: HTTPS from `hosts` only, redirects followed here (each one checked).
#[derive(Debug, Clone)]
pub struct Http {
    agent: ureq::Agent,
    hosts: &'static [&'static str],
}

impl Http {
    /// Downloads from `hosts` (lowercase) only, wherever they redirect.
    pub fn new(hosts: &'static [&'static str]) -> Self {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_resolve(Some(Duration::from_secs(30)))
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            // The largest download (~110 MB) on a slow line; a stalled one ends with it.
            .timeout_recv_body(Some(Duration::from_secs(2 * 60 * 60)))
            .build();
        Self { agent: config.into(), hosts }
    }
}

impl Fetch for Http {
    fn open(&self, url: &str, from: u64) -> Result<Body, FetchError> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            if !allowed(&url, self.hosts) {
                return Err(FetchError::Refused(url));
            }
            let mut request = self.agent.get(&url);
            if from > 0 {
                request = request.header("Range", format!("bytes={from}-"));
            }
            let response = request.call().map_err(|e| classify(&url, e))?;
            let status = response.status().as_u16();
            let header = |name: &str| {
                response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
            };
            if (300..400).contains(&status) {
                let location = header("location").ok_or_else(|| {
                    FetchError::Failed(format!("{url}: a redirect without a location"))
                })?;
                url = redirected(&url, &location);
                continue;
            }
            let (start, total) = match status {
                200 => (0, header("content-length").and_then(|v| v.parse().ok())),
                206 => {
                    // `bytes <start>-<end>/<total>`.
                    let range = header("content-range").unwrap_or_default();
                    let start = range
                        .strip_prefix("bytes ")
                        .and_then(|r| r.split('-').next())
                        .and_then(|s| s.parse().ok());
                    let total = range.rsplit('/').next().and_then(|t| t.parse().ok());
                    match start {
                        Some(start) if start == from => (start, total),
                        _ => return Err(FetchError::Failed(format!("{url}: a bad resumed range"))),
                    }
                }
                code => return Err(FetchError::Status(code, url)),
            };
            let reader = response.into_body().into_reader();
            return Ok(Body { from: start, total, reader: Box::new(reader) });
        }
        Err(FetchError::Failed(format!("{url}: too many redirects")))
    }
}

/// Where a redirect from `from` to `location` goes (an absolute URL, or a path on the same
/// host).
fn redirected(from: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_owned();
    }
    let host_end = from
        .strip_prefix("https://")
        .and_then(|rest| rest.find('/').map(|i| i + "https://".len()))
        .unwrap_or(from.len());
    match location.strip_prefix('/') {
        Some(path) => format!("{}/{path}", &from[..host_end]),
        None => format!("{}/{location}", from.rsplit_once('/').map_or(from, |(dir, _)| dir)),
    }
}

/// A network error as the user is told it: the network itself, or something else.
fn classify(url: &str, error: ureq::Error) -> FetchError {
    match error {
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(_)
        | ureq::Error::Io(_) => FetchError::Offline(format!("{url}: {error}")),
        ureq::Error::RequireHttpsOnly(_) => FetchError::Refused(url.to_owned()),
        other => FetchError::Failed(format!("{url}: {other}")),
    }
}

/// What a download is checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pin {
    /// A SHA-256, in hex (the pinned archives and files).
    Sha256(String),
    /// npm's `integrity`: `sha512-<base64>`.
    Integrity(String),
}

impl Pin {
    fn algorithm(&self) -> &'static ring::digest::Algorithm {
        match self {
            Self::Sha256(_) => &ring::digest::SHA256,
            Self::Integrity(_) => &ring::digest::SHA512,
        }
    }

    /// Whether `digest` (of the pin's algorithm) is the pinned one.
    pub fn matches(&self, digest: &[u8]) -> bool {
        match self {
            Self::Sha256(hex) => {
                hex.len() == 64 && crate::digest::hex(digest).eq_ignore_ascii_case(hex.trim())
            }
            Self::Integrity(integrity) => integrity
                .trim()
                .strip_prefix("sha512-")
                .is_some_and(|b64| crate::digest::base64(digest) == b64),
        }
    }

    /// Whether `bytes` are what's pinned.
    pub fn verify(&self, bytes: &[u8]) -> bool {
        self.matches(ring::digest::digest(self.algorithm(), bytes).as_ref())
    }
}

/// How often a download looks at its stop flag and tells its progress (bytes).
const CHUNK: usize = 1 << 16;

/// The file at `url` (on one of `hosts`) into memory, at most `limit` bytes, checked against
/// `pin`; `progress(received, total)` as it comes (the total when the source says it); `stop`
/// ends it.
pub fn fetch_bytes(
    fetch: &dyn Fetch,
    url: &str,
    hosts: &[&str],
    pin: &Pin,
    limit: u64,
    progress: &dyn Fn(u64, Option<u64>),
    stop: &AtomicBool,
) -> Result<Vec<u8>, FetchError> {
    if !allowed(url, hosts) {
        return Err(FetchError::Refused(url.to_owned()));
    }
    let mut body = fetch.open(url, 0)?;
    if body.total.is_some_and(|t| t > limit) {
        return Err(FetchError::TooLarge(url.to_owned()));
    }
    let mut out = Vec::with_capacity(body.total.unwrap_or(0).min(limit) as usize);
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(FetchError::Stopped);
        }
        let n = body.reader.read(&mut buffer).map_err(|e| {
            // Broken off because it was stopped (the connection closed under it).
            if stop.load(Ordering::Relaxed) { FetchError::Stopped } else { broken(url, &e) }
        })?;
        if n == 0 {
            break;
        }
        if out.len() as u64 + n as u64 > limit {
            return Err(FetchError::TooLarge(url.to_owned()));
        }
        out.extend_from_slice(&buffer[..n]);
        progress(out.len() as u64, body.total);
    }
    if !pin.verify(&out) {
        return Err(FetchError::Mismatch(url.to_owned()));
    }
    Ok(out)
}

/// The file at `url` (on one of `hosts`) into `part` (resumed from what's there already), at
/// most `limit` bytes, checked against `pin` once whole: `Ok` with it whole and checked. A
/// broken-off download leaves `part` to be resumed; one that doesn't match removes it.
#[allow(clippy::too_many_arguments)]
pub fn fetch_file(
    fetch: &dyn Fetch,
    url: &str,
    hosts: &[&str],
    part: &Path,
    pin: &Pin,
    limit: u64,
    progress: &dyn Fn(u64, Option<u64>),
    stop: &AtomicBool,
) -> Result<(), FetchError> {
    if !allowed(url, hosts) {
        return Err(FetchError::Refused(url.to_owned()));
    }
    let failed = |e: std::io::Error| FetchError::Failed(format!("{}: {e}", part.display()));
    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let mut have = std::fs::metadata(part).map_or(0, |m| m.len());
    if have > limit {
        std::fs::remove_file(part).map_err(failed)?;
        have = 0;
    }
    let mut body = match fetch.open(url, have) {
        // Asked from past its end (it was whole, or the file changed): from the start.
        Err(FetchError::Status(416, _)) if have > 0 => {
            std::fs::remove_file(part).map_err(failed)?;
            have = 0;
            fetch.open(url, 0)?
        }
        other => other?,
    };
    let mut context = ring::digest::Context::new(pin.algorithm());
    let mut file = if body.from > 0 && body.from == have {
        // The part's bytes go into the checksum first.
        let mut existing = std::fs::File::open(part).map_err(failed)?;
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let n = existing.read(&mut buffer).map_err(failed)?;
            if n == 0 {
                break;
            }
            context.update(&buffer[..n]);
        }
        std::fs::OpenOptions::new().append(true).open(part).map_err(failed)?
    } else {
        have = 0;
        std::fs::File::create(part).map_err(failed)?
    };
    if body.total.is_some_and(|t| t > limit) {
        drop(file);
        let _ = std::fs::remove_file(part);
        return Err(FetchError::TooLarge(url.to_owned()));
    }
    let mut received = have;
    progress(received, body.total);
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(FetchError::Stopped);
        }
        let n = body.reader.read(&mut buffer).map_err(|e| {
            // Broken off because it was stopped (the connection closed under it).
            if stop.load(Ordering::Relaxed) { FetchError::Stopped } else { broken(url, &e) }
        })?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > limit {
            drop(file);
            let _ = std::fs::remove_file(part);
            return Err(FetchError::TooLarge(url.to_owned()));
        }
        file.write_all(&buffer[..n]).map_err(failed)?;
        context.update(&buffer[..n]);
        progress(received, body.total);
    }
    file.flush().map_err(failed)?;
    drop(file);
    if !pin.matches(context.finish().as_ref()) {
        let _ = std::fs::remove_file(part);
        return Err(FetchError::Mismatch(url.to_owned()));
    }
    Ok(())
}

/// A body that broke off: the network (resumable), as the user is told.
fn broken(url: &str, error: &std::io::Error) -> FetchError {
    FetchError::Offline(format!("{url}: {error}"))
}

/// Files in memory (tests, snapshots: never the network): what's served at which URL, what
/// was asked, and the network's moods — offline, a download broken off after some bytes,
/// one that stalls until let go.
#[cfg(any(test, feature = "fake"))]
#[derive(Debug, Default)]
pub struct Memory {
    files: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    asked: std::sync::Mutex<Vec<(String, u64)>>,
    offline: AtomicBool,
    /// The next download breaks off after this many bytes.
    cut: std::sync::Mutex<Option<u64>>,
    /// Downloads stall after this many bytes, until [`Memory::release`].
    stall: std::sync::Mutex<Option<u64>>,
    released: std::sync::Arc<AtomicBool>,
}

#[cfg(any(test, feature = "fake"))]
impl Memory {
    /// `bytes` served at `url`.
    pub fn serve(&self, url: &str, bytes: Vec<u8>) {
        self.files
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(url.into(), bytes);
    }

    /// The URLs asked for, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(u, _)| u.clone())
            .collect()
    }

    /// The URLs asked for, with the byte each was asked from.
    pub fn asked_from(&self) -> Vec<(String, u64)> {
        self.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn forget_asked(&self) {
        self.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }

    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::Relaxed);
    }

    /// The next download breaks off after `bytes`.
    pub fn cut_next_after(&self, bytes: u64) {
        *self.cut.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(bytes);
    }

    /// Downloads stall after `bytes` (`None`: they don't) until [`Self::release`].
    pub fn stall_after(&self, bytes: Option<u64>) {
        *self.stall.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = bytes;
        self.released.store(false, Ordering::Relaxed);
    }

    /// Stalled downloads break off.
    pub fn release(&self) {
        self.released.store(true, Ordering::Relaxed);
    }
}

#[cfg(any(test, feature = "fake"))]
impl Fetch for Memory {
    fn open(&self, url: &str, from: u64) -> Result<Body, FetchError> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((url.to_owned(), from));
        if self.offline.load(Ordering::Relaxed) {
            return Err(FetchError::Offline(format!("{url}: offline")));
        }
        let bytes = self
            .files
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(url)
            .cloned()
            .ok_or_else(|| FetchError::Status(404, url.to_owned()))?;
        let total = bytes.len() as u64;
        if from > total {
            return Err(FetchError::Status(416, url.to_owned()));
        }
        let cut = self.cut.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        let stall = *self.stall.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let rest = bytes[from as usize..].to_vec();
        let reader = Moody {
            bytes: rest,
            at: 0,
            cut,
            stall,
            released: std::sync::Arc::clone(&self.released),
        };
        Ok(Body { from, total: Some(total), reader: Box::new(reader) })
    }
}

/// The longest a stalled body waits to be let go.
#[cfg(any(test, feature = "fake"))]
const STALLED_AT_MOST: Duration = Duration::from_secs(20);

/// A body in memory that may break off or stall.
#[cfg(any(test, feature = "fake"))]
struct Moody {
    bytes: Vec<u8>,
    at: usize,
    cut: Option<u64>,
    stall: Option<u64>,
    released: std::sync::Arc<AtomicBool>,
}

#[cfg(any(test, feature = "fake"))]
impl Read for Moody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let limit = match (self.cut, self.stall) {
            (Some(c), Some(s)) => Some(c.min(s)),
            (c, s) => c.or(s),
        };
        if let Some(limit) = limit
            && self.at as u64 >= limit
        {
            if self.cut.is_some_and(|c| self.at as u64 >= c) {
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "cut off"));
            }
            // Let go, or given up after a while: a test that failed before letting it go
            // leaves no install stalled for ever (the next ones would wait behind it).
            let until = std::time::Instant::now() + STALLED_AT_MOST;
            while !self.released.load(Ordering::Relaxed) && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "let go"));
        }
        let end = limit.map_or(self.bytes.len(), |l| (l as usize).min(self.bytes.len()));
        let n = buf.len().min(end - self.at).min(4096);
        buf[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

#[cfg(test)]
#[path = "../tests/unit/fetch.rs"]
mod tests;
