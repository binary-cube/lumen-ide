//! An instance's file: the app's own pid and start time (to know whether it still runs),
//! then each program it started and still runs. Small and bounded: at most
//! [`MAX_RECORDS`] programs written, a file bigger than [`MAX_FILE`] never read.

use std::path::{Path, PathBuf};

use process::launched::Kind;
use serde::{Deserialize, Serialize};

/// The most programs one file keeps (the newest let go past it).
pub const MAX_RECORDS: usize = 256;

/// The biggest file read (anything bigger isn't one of ours).
pub const MAX_FILE: u64 = 256 * 1024;

/// A program the app started, still running when last written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub pid: u32,
    /// When it started (seconds since the epoch, as the system says): a pid used again by
    /// another program has another start time.
    pub started: u64,
    /// What it is ([`Kind::id`]).
    pub kind: String,
    /// Its name only (never its arguments).
    pub program: String,
    /// The project (a folder in it) it ran for, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

impl Record {
    /// What it is.
    pub fn kind(&self) -> Kind {
        Kind::from_id(&self.kind)
    }

    /// The folder it ran for, when known.
    pub fn project_path(&self) -> Option<PathBuf> {
        self.project.as_deref().filter(|p| !p.is_empty()).map(PathBuf::from)
    }

    /// Whether it ran for the project at `root` (in it or in one of its folders).
    pub fn belongs_to(&self, root: &Path) -> bool {
        self.project_path().is_some_and(|p| p.starts_with(root))
    }
}

/// One instance's file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceFile {
    /// The app's own pid.
    pub app_pid: u32,
    /// When the app started (seconds since the epoch).
    pub app_started: u64,
    #[serde(default)]
    pub programs: Vec<Record>,
}

impl InstanceFile {
    /// Its text (at most [`MAX_RECORDS`] programs; names and paths as one line each).
    pub fn to_text(&self) -> String {
        let mut file = self.clone();
        file.programs.truncate(MAX_RECORDS);
        toml::to_string(&file).unwrap_or_default()
    }

    /// Read from `text`; `None`: not one of ours.
    pub fn parse(text: &str) -> Option<Self> {
        let mut file: Self = toml::from_str(text).ok()?;
        file.programs.truncate(MAX_RECORDS);
        Some(file)
    }

    /// Read from `path` (bounded); `None`: missing, too big or not one of ours.
    pub fn read(path: &Path) -> Option<Self> {
        let size = std::fs::metadata(path).ok()?.len();
        if size > MAX_FILE {
            return None;
        }
        Self::parse(&std::fs::read_to_string(path).ok()?)
    }
}

#[cfg(test)]
#[path = "../tests/unit/record.rs"]
mod tests;
