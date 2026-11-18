//! What the question about a program left running shows of it: what it is, its project,
//! how long it has run, whether it works now. Pure: the app words it.

use std::path::Path;
use std::time::Duration;

use process::launched::Kind;

use crate::survey::Found;

/// One program left running, as shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub kind: Kind,
    /// Its name.
    pub program: String,
    /// Its project's name (the folder's), when known.
    pub project: Option<String>,
    /// How long it has run.
    pub running_for: Duration,
    /// Its CPU now (percent of one core) and whether that's working.
    pub cpu: f32,
    pub working: bool,
}

/// `found` as shown at `now` (seconds since the epoch).
pub fn row(found: &Found, now: u64) -> Row {
    let record = &found.record;
    Row {
        kind: record.kind(),
        program: record.program.clone(),
        project: record.project_path().and_then(|p| folder_name(&p)),
        running_for: Duration::from_secs(now.saturating_sub(record.started)),
        cpu: found.cpu,
        working: found.working(),
    }
}

/// The last part of `path` (a project's name).
fn folder_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let name: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    (!name.is_empty()).then_some(name)
}

/// Now, in seconds since the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
#[path = "../tests/unit/row.rs"]
mod tests;
