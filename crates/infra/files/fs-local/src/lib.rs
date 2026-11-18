//! Adapter for the local file system: reading directories and files, atomic save, moving
//! entries, watching a project ([`watch`]).

pub mod entries;
mod own_saves;
pub mod remote_copy;
pub mod watch;

pub use watch::LocalWatching;

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use document::DocumentStorage;
use settings_model::EditorConfigSource;
use workspace::{DirEntry, FileSystem, NodeKind};

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFileSystem;

impl FileSystem for LocalFileSystem {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            // `metadata` follows symlinks, so a link to a directory can be expanded.
            let is_dir = match entry.file_type() {
                Ok(t) if t.is_symlink() => entry.path().is_dir(),
                Ok(t) => t.is_dir(),
                Err(_) => continue,
            };
            let kind = if is_dir { NodeKind::Directory } else { NodeKind::File };
            entries.push(DirEntry::new(entry.file_name().to_string_lossy(), kind));
        }
        Ok(entries)
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn kind(&self, path: &Path) -> Option<NodeKind> {
        let meta = std::fs::metadata(path).ok()?;
        Some(if meta.is_dir() { NodeKind::Directory } else { NodeKind::File })
    }

    fn stamp(&self, path: &Path) -> Option<workspace::Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.len(), meta.modified().ok()?))
    }

    fn canonical(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

impl DocumentStorage for LocalFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn head(&self, path: &Path, max: usize) -> io::Result<(u64, Vec<u8>)> {
        use std::io::Read as _;
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        let mut head = Vec::with_capacity(max.min(len as usize));
        file.take(max as u64).read_to_end(&mut head)?;
        Ok((len, head))
    }

    /// Writes to a temporary file in the same directory, then renames it over the
    /// original: a crash during saving can't leave the file half-written.
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        // A link: the file it points to is written (the link stays a link).
        let linked = std::fs::symlink_metadata(path)
            .is_ok_and(|m| m.file_type().is_symlink())
            .then(|| std::fs::canonicalize(path).ok())
            .flatten();
        let path = linked.as_deref().unwrap_or(path);
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        // A name of its own: two saves of the same file never share it.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(own_saves::temporary_name(&name, n));

        let original = std::fs::metadata(path).ok();
        let result = (|| {
            // Its own from the start: the original's permissions (a private `.env`, an
            // executable script), never briefly anyone's; new, never a link left there.
            let mut file = open_new(&tmp, original.as_ref())?;
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            // Put in its own place: the watcher hears it as written, not as a new entry.
            if original.is_some() {
                own_saves::replacing(path, &tmp);
            }
            replace(&tmp, path, contents, original.as_ref())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    fn read_only(&self, path: &Path) -> bool {
        read_only(path)
    }
}

/// The file can't be written by this process: no write permission for its user, a
/// read-only volume, an immutable flag (`access(W_OK)`, as the kernel decides).
#[cfg(unix)]
#[allow(unsafe_code)]
fn read_only(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call.
    let denied = unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } != 0;
    // Missing or unreadable: not a read-only file (opening it fails on its own).
    denied && std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// The file has the read-only attribute (Windows: what Explorer's "Read-only" sets).
#[cfg(not(unix))]
fn read_only(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().readonly())
}

/// A new file at `path` (failing if anything is there), with `original`'s permissions on
/// Unix (the process can still write it: they apply to later opens).
#[cfg(unix)]
fn open_new(path: &Path, original: Option<&std::fs::Metadata>) -> io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    if let Some(meta) = original {
        options.mode(meta.permissions().mode() & 0o7777);
    }
    options.open(path)
}

/// A new file at `path` (failing if anything is there). Windows: no read-only flag on it (it
/// couldn't be renamed over the original, nor removed if that failed).
#[cfg(not(unix))]
fn open_new(path: &Path, _original: Option<&std::fs::Metadata>) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// Puts the written `tmp` in `path`'s place.
#[cfg(unix)]
fn replace(
    tmp: &Path,
    path: &Path,
    _contents: &[u8],
    _original: Option<&std::fs::Metadata>,
) -> io::Result<()> {
    std::fs::rename(tmp, path)
}

/// Puts the written `tmp` in `path`'s place. On Windows another program holding the file
/// (an antivirus scan, a sync client, another editor) refuses it for a moment: tried again,
/// then written in place (not atomic, but saved). A read-only file stays refused.
#[cfg(not(unix))]
fn replace(
    tmp: &Path,
    path: &Path,
    contents: &[u8],
    original: Option<&std::fs::Metadata>,
) -> io::Result<()> {
    if original.is_some_and(|m| m.permissions().readonly()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the file is read-only"));
    }
    let mut last = None;
    for attempt in 0..5u64 {
        match std::fs::rename(tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(20 * (attempt + 1)));
    }
    if original.is_some() && std::fs::write(path, contents).is_ok() {
        let _ = std::fs::remove_file(tmp);
        return Ok(());
    }
    Err(last.unwrap_or_else(|| io::ErrorKind::PermissionDenied.into()))
}

impl EditorConfigSource for LocalFileSystem {
    fn read_editorconfig(&self, dir: &Path) -> io::Result<Option<String>> {
        match std::fs::read_to_string(dir.join(".editorconfig")) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Copies `from` (a file, or a folder with everything in it) to `to`, which must not
/// exist. Symbolic links are copied as the files they point to.
pub fn copy_recursive(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    copy_recursive_with(from, to, &mut |_| {}, &|| false)
}

/// [`copy_recursive`], calling `copied` with each file's size once it's copied, and
/// stopping (an `Interrupted` error) when `cancelled` says so.
pub fn copy_recursive_with(
    from: &std::path::Path,
    to: &std::path::Path,
    copied: &mut dyn FnMut(u64),
    cancelled: &dyn Fn() -> bool,
) -> std::io::Result<()> {
    if cancelled() {
        return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
    }
    // A link is never followed (one to a parent folder would never end): copied as a link.
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return copy_link(from, to);
    }
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive_with(&entry.path(), &to.join(entry.file_name()), copied, cancelled)?;
        }
        Ok(())
    } else {
        let size = std::fs::copy(from, to)?;
        copied(size);
        Ok(())
    }
}

/// A link copied as a link (pointing where it pointed).
#[cfg(unix)]
fn copy_link(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

/// A link copied as a link (pointing where it pointed); one to a folder needs a right
/// Windows may not give: then its target's files are not copied either (never followed).
#[cfg(windows)]
fn copy_link(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let target = std::fs::read_link(from)?;
    if std::fs::metadata(from).is_ok_and(|m| m.is_dir()) {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(&target, to)
            .or_else(|_| std::fs::copy(from, to).map(drop))
    }
}

/// How many files and bytes `path` holds (itself, or everything in a folder; a link counts
/// as itself, never followed).
pub fn measure(path: &std::path::Path) -> (u64, u64) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return (0, 0) };
    if !meta.is_dir() {
        return (1, meta.len());
    }
    let Ok(entries) = std::fs::read_dir(path) else { return (0, 0) };
    entries.flatten().map(|e| measure(&e.path())).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

/// A refactoring's moves on the local disk (`refactor::applying::rename_entry`).
impl refactor::applying::Entries for LocalFileSystem {
    fn is_taken(&self, from: &Path, to: &Path) -> bool {
        is_taken(from, to)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

/// Whether `to` is taken by an entry other than `from`. On a disk that ignores case (macOS',
/// Windows') `User.php` is `user.php` itself: renaming one to the other only changes case.
pub fn is_taken(from: &Path, to: &Path) -> bool {
    to.exists() && !is_same_entry(from, to)
}

/// `a` and `b` are the same file or folder on disk (by identity on Unix; on Windows the same
/// folder and the same name but for case).
fn is_same_entry(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
        a.exists() && a.parent() == b.parent() && name(a) == name(b)
    }
}

/// Moves `from` to `to` (not over another entry): a rename, or — across disks — a copy then
/// the original removed. A change of case only is a rename.
pub fn move_entry(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if is_same_entry(from, to) {
        return std::fs::rename(from, to);
    }
    if to.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "already exists"));
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_recursive(from, to)?;
    if std::fs::metadata(from)?.is_dir() {
        std::fs::remove_dir_all(from)
    } else {
        std::fs::remove_file(from)
    }
}

/// Moves `paths` to the system's trash (Recycle Bin on Windows): restorable from there.
/// On macOS through the file manager's API (no permission prompt, no Finder scripting).
pub fn move_to_trash(paths: &[std::path::PathBuf]) -> io::Result<()> {
    #[allow(unused_mut)]
    let mut context = trash::TrashContext::new();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete_all(paths).map_err(|e| io::Error::other(e.to_string()))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
