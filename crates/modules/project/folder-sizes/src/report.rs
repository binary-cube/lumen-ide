//! A measure as kept: the folder, when it was measured, how long it took, whether it's whole,
//! and its sizes. Kept in the project's data folder (`folder-sizes/<folder's hash>.json`),
//! written and read on a worker; read back as hostile input (another folder's, broken:
//! not taken).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::scan::Node;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// The folder measured.
    pub path: PathBuf,
    /// When (seconds since the epoch).
    pub at: i64,
    /// How long it took (milliseconds).
    pub took_ms: u64,
    /// Stopped at its limit of entries: not everything counted.
    #[serde(default)]
    pub partial: bool,
    /// Folders that couldn't be read.
    #[serde(default)]
    pub unreadable: u64,
    pub root: Node,
}

/// Where the measure of `folder` is kept, in the project's data folder `storage`.
pub fn file(storage: &Path, folder: &Path) -> PathBuf {
    let hash =
        folder.as_os_str().as_encoded_bytes().iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
    storage.join("folder-sizes").join(format!("{hash:016x}.json"))
}

/// The measure kept of `folder`, if there's one (and it's that folder's).
pub fn load(storage: &Path, folder: &Path) -> Option<Report> {
    let text = std::fs::read_to_string(file(storage, folder)).ok()?;
    let report: Report = serde_json::from_str(&text).ok()?;
    (report.path == folder).then_some(report)
}

/// `report` kept (through a temporary file renamed over the old one).
pub fn save(storage: &Path, report: &Report) -> std::io::Result<()> {
    let path = file(storage, &report.path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(report).map_err(std::io::Error::other)?;
    let temp =
        path.with_extension(format!("{}.tmp", kernel::id::uuid_text(kernel::id::new_uuid())));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Seconds since the epoch, now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
#[path = "../tests/unit/report.rs"]
mod tests;
