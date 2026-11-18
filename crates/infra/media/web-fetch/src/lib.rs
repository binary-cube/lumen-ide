//! What a document shows from the web (a Markdown preview's `https://` images): fetched
//! with limits — `http(s)` only, no credentials in the URL and none sent (no cookies), a
//! plain User-Agent, a timeout, few redirects, only what the server says is of an allowed
//! kind, at most so many bytes — and kept in a disk cache of capped size (the least
//! recently used files go first), so a document opens again without the network.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What a fetch may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: u64,
    pub timeout: Duration,
    pub max_redirects: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_bytes: 20 * 1024 * 1024, timeout: Duration::from_secs(20), max_redirects: 5 }
    }
}

/// Why a file wasn't fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not an `http(s)` URL, or one with credentials in it.
    Refused,
    /// No answer: offline, a DNS failure, a timeout.
    Offline(String),
    /// The server answered with an error.
    Status(u16),
    /// Larger than allowed.
    TooLarge,
    /// Not of an allowed kind (its content type).
    Kind(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused => write!(f, "not an http(s) URL without credentials"),
            Self::Offline(e) => write!(f, "unreachable: {e}"),
            Self::Status(s) => write!(f, "HTTP {s}"),
            Self::TooLarge => write!(f, "too large"),
            Self::Kind(k) => write!(f, "not an image ({k})"),
        }
    }
}

/// A file fetched: its bytes and its content type (lowercase, without parameters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub kind: String,
}

/// The User-Agent sent: plain, naming no one.
const USER_AGENT: &str = "Mozilla/5.0 (compatible; MarkdownPreview/1.0)";

/// Whether `url` may be fetched: `http(s)://`, a host, no `user:password@`.
pub fn allowed(url: &str) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    let Some(rest) = rest else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty() && !authority.contains(['@', '\\', ' '])
}

/// The file at `url`, if its content type is one `kinds` accepts (`image/`).
pub fn fetch(url: &str, limits: Limits, kinds: &dyn Fn(&str) -> bool) -> Result<Fetched, Error> {
    if !allowed(url) {
        return Err(Error::Refused);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(limits.timeout))
        .max_redirects(limits.max_redirects)
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| Error::Offline(e.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::Status(status));
    }
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !kinds(&kind) {
        return Err(Error::Kind(kind));
    }
    let declared =
        response.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    if declared.is_some_and(|n| n > limits.max_bytes) {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    let reader = response.body_mut().as_reader();
    reader
        .take(limits.max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Offline(e.to_string()))?;
    if bytes.len() as u64 > limits.max_bytes {
        return Err(Error::TooLarge);
    }
    Ok(Fetched { bytes, kind })
}

/// A folder of fetched files, capped in size: each file named by its URL's hash (and an
/// extension for its kind), the least recently used going first.
#[derive(Debug, Clone)]
pub struct DiskCache {
    dir: PathBuf,
    cap: u64,
}

impl DiskCache {
    pub fn new(dir: PathBuf, cap: u64) -> Self {
        Self { dir, cap }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file kept for `url` (any extension), if there's one; touched (used now).
    pub fn get(&self, url: &str) -> Option<PathBuf> {
        let stem = stem(url);
        let entries = std::fs::read_dir(&self.dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_stem().and_then(|s| s.to_str()) == Some(stem.as_str()) {
                // Used now: the last to go.
                if let Ok(file) = std::fs::File::options().write(true).open(&path) {
                    let _ = file.set_modified(std::time::SystemTime::now());
                }
                return Some(path);
            }
        }
        None
    }

    /// `bytes` kept for `url` with `extension`: the file's path. Over the cap, the oldest go.
    pub fn put(&self, url: &str, extension: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{}.{extension}", stem(url)));
        let part = path.with_extension("part");
        std::fs::write(&part, bytes)?;
        std::fs::rename(&part, &path)?;
        self.trim();
        Ok(path)
    }

    /// The oldest files gone until the folder is within its cap.
    pub fn trim(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                meta.is_file().then(|| {
                    (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len(), e.path())
                })
            })
            .collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= self.cap {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }

    /// Every file gone ("Clear and Rebuild Caches").
    pub fn clear(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            if entry.path().is_file() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// The bytes it holds.
    pub fn size(&self) -> u64 {
        std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }
}

/// A URL's file name: a hash of it (FNV-1a, stable across runs).
fn stem(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
