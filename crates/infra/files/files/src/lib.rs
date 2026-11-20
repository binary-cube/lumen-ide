//! Safe file writes, shared by every adapter that persists something.
//!
//! The content goes to a temporary file next to the target (a unique name: two writes,
//! two processes or two targets with the same stem never share it), is flushed to the
//! disk, then renamed over the target: after a crash or a power loss the file is either
//! the old one or the new one, never empty or half-written.

pub mod ignore;
pub mod places;

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes `bytes` to `path` atomically (its folder is made if missing).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write(path, bytes, false)
}

/// Like [`write_atomic`], for secrets: on Unix the file is readable only by the user from
/// the moment it exists (mode 0600), never briefly with the default permissions. On Windows
/// it's then given to the user alone (its inherited permissions removed).
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write(path, bytes, true)?;
    restrict(path);
    Ok(())
}

/// Windows: only the user may use the file (`icacls`: no inherited permissions, full
/// control to the user), as `0600` does on Unix. Best effort: the file's folder is the
/// user's already.
#[cfg(windows)]
fn restrict(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Some(user) = std::env::var_os("USERNAME") else { return };
    let mut account = std::env::var_os("USERDOMAIN").unwrap_or_default();
    if !account.is_empty() {
        account.push("\\");
    }
    account.push(user);
    account.push(":F");
    let done = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(account)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if !done.is_ok_and(|s| s.success()) {
        tracing::warn!(path = %path.display(), "can't make a file the user's alone");
    }
}

#[cfg(not(windows))]
fn restrict(_path: &Path) {}

fn write(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = temporary(dir, path);
    let result = (|| {
        let mut file = open_new(&tmp, private)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// A temporary name next to `path`, unique to this process and this write.
/// The temporary files [`write_atomic`] left in `dir` (a write killed half way: never put in
/// place, never removed) older than `age`, removed; how many. A younger one may be another
/// process's write still going.
pub fn remove_left_temporaries(dir: &Path, age: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((_, suffix)) = name.strip_suffix(".tmp").and_then(|n| n.rsplit_once('.')) else {
            continue;
        };
        let ours = name.starts_with('.')
            && suffix.split_once('-').is_some_and(|(pid, n)| {
                [pid, n].iter().all(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
            });
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|e| e >= age));
        if ours && old && entry.file_type().is_ok_and(|t| t.is_file()) {
            removed += usize::from(std::fs::remove_file(entry.path()).is_ok());
        }
    }
    removed
}

fn temporary(dir: &Path, path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()))
}

#[cfg(unix)]
fn open_new(path: &Path, private: bool) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    if private {
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(not(unix))]
fn open_new(path: &Path, _private: bool) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
