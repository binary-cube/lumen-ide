//! Local history: every save keeps a version of the file (and the version on disk before
//! it, so external edits and the first state aren't lost), independent of git. Versions
//! are kept for a while ([`Retention`]) and can be compared with the current text or
//! brought back. Storage is a port ([`HistoryStore`], implemented on disk in infra).

pub mod application;

use std::time::Duration;

/// Why a version was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RevisionKind {
    /// The file as saved.
    Saved,
    /// The file as it was on disk just before a save (the first version, or one changed
    /// outside the editor).
    BeforeSave,
}

impl RevisionKind {
    /// Its letter in a stored name.
    pub fn code(self) -> char {
        match self {
            Self::Saved => 's',
            Self::BeforeSave => 'b',
        }
    }

    pub fn from_code(code: char) -> Option<Self> {
        match code {
            's' => Some(Self::Saved),
            'b' => Some(Self::BeforeSave),
            _ => None,
        }
    }
}

/// A kept version of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// When it was kept (milliseconds since the Unix epoch): its id.
    pub at: u64,
    pub kind: RevisionKind,
    /// The text's size (bytes).
    pub size: u64,
    /// A hash of the text (identical versions aren't kept twice in a row).
    pub hash: u64,
}

/// How long versions are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub max_age: Duration,
    /// Per file, the newest ones.
    pub max_per_file: usize,
    /// Files larger than this aren't kept (a generated bundle, a dump).
    pub max_file_size: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            max_age: Duration::from_secs(30 * 24 * 3600),
            max_per_file: 200,
            max_file_size: 2 * 1024 * 1024,
        }
    }
}

impl Retention {
    /// The revisions to drop from `revisions` (newest first) at `now` (ms): too old, or past
    /// the per-file count. The newest one always stays.
    pub fn expired<'a>(&self, revisions: &'a [Revision], now: u64) -> Vec<&'a Revision> {
        let max_age = self.max_age.as_millis() as u64;
        revisions
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                *i > 0 && (*i >= self.max_per_file || now.saturating_sub(r.at) > max_age)
            })
            .map(|(_, r)| r)
            .collect()
    }
}

/// A text's hash (FNV-1a: stable across runs and systems, unlike `DefaultHasher`).
pub fn text_hash(text: &[u8]) -> u64 {
    text.iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Where versions are kept. `path` is relative to the project's root, with `/`.
pub trait HistoryStore: Send + Sync + std::fmt::Debug {
    /// Keeps `text` as a new version of `path`.
    fn put(&self, path: &str, revision: &Revision, text: &[u8]) -> std::io::Result<()>;
    /// The versions of `path`, newest first.
    fn revisions(&self, path: &str) -> Vec<Revision>;
    /// A version's text.
    fn read(&self, path: &str, at: u64) -> Option<Vec<u8>>;
    /// Drops a version.
    fn remove(&self, path: &str, at: u64) -> std::io::Result<()>;
    /// Every file with versions.
    fn paths(&self) -> Vec<String>;
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
