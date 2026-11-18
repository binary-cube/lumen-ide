//! Our own saves, as the watcher hears them: a save writes a temporary file beside the file
//! and renames it over it, which the system reports as a file created, renamed and gone.
//! None of that changes the project's entries: the temporary file is never one (ignored
//! whole), and the file put in place of itself is only written — so a save (an automatic
//! one at each pause in typing) never makes the tree, the indexes and the frameworks look
//! at the project's entries again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long after a save its events are taken as ours when the files' identities can't be
/// told (no inode): the system reports them within a fraction of a second, a busy one much
/// later (measured on macOS under load: past 5 s, up to ~30).
const HEARD_WITHIN: Duration = Duration::from_secs(30);

/// How long a save is remembered when the files' identities are known: a file still the one
/// our save put there (its inode) is ours however late its events come (measured on macOS
/// with every core busy: past 30 s), and one another program made since isn't.
const KNOWN_WITHIN: Duration = Duration::from_secs(600);

/// The saves remembered at most (the oldest let go first).
const KEPT: usize = 4096;

/// A file on disk, as the system tells it apart from any other (device, inode).
type Identity = (u64, u64);

/// `path`'s identity (`None`: nothing there, or not told on this system).
fn identity(path: &Path) -> Option<Identity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// A save of ours: when, the file it put in place (its temporary file's identity, which the
/// rename keeps) and the folder it was put in.
#[derive(Debug, Clone, Copy)]
struct Save {
    at: Instant,
    file: Option<Identity>,
    folder: Option<Identity>,
}

impl Save {
    /// Still to be remembered.
    fn fresh(&self) -> bool {
        let within = if self.file.is_some() { KNOWN_WITHIN } else { HEARD_WITHIN };
        self.at.elapsed() < within
    }

    /// `now` (an identity read now) is still what this save knew; without identities, only
    /// within the short window.
    fn same(&self, known: Option<Identity>, now: Option<Identity>) -> bool {
        match known {
            Some(id) => now == Some(id),
            None => self.at.elapsed() < HEARD_WITHIN,
        }
    }
}

/// The files replaced by a save (shared by the process's saves and watchers).
static REPLACED: Mutex<Option<HashMap<PathBuf, Save>>> = Mutex::new(None);

/// What a path the watcher heard is to us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Heard {
    /// A save's temporary file: nothing at all.
    Temporary,
    /// A file a save of ours just replaced (still the one it put there): written, not a new
    /// entry.
    Replaced,
    /// The folder a save of ours just wrote in (still the same folder): the system reports
    /// it again, with every kind of change it ever saw on it (macOS: "created" too), though
    /// its entries are as they were.
    Holder,
    /// Anything else, as heard.
    Other,
}

/// The temporary name a save of `name` writes to (`n`: its own number in the process).
pub(crate) fn temporary_name(name: &str, n: u64) -> String {
    format!(".{name}.saving-{}-{n}.tmp", std::process::id())
}

/// A save's temporary file (this process's or another instance's).
pub(crate) fn is_temporary(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|name| {
        name.starts_with('.')
            && name.ends_with(".tmp")
            && name.rsplit_once(".saving-").is_some_and(|(_, rest)| {
                let digits = rest.trim_end_matches(".tmp");
                !digits.is_empty()
                    && digits.split('-').all(|d| d.bytes().all(|b| b.is_ascii_digit()))
            })
    })
}

/// `path` (an existing file) is about to be replaced by `written` (the save's temporary
/// file, renamed over it next); the old saves forgotten.
pub(crate) fn replacing(path: &Path, written: &Path) {
    let save = Save {
        at: Instant::now(),
        file: identity(written),
        folder: path.parent().and_then(identity),
    };
    let mut replaced = REPLACED.lock().unwrap_or_else(PoisonError::into_inner);
    let replaced = replaced.get_or_insert_with(HashMap::new);
    replaced.retain(|_, s| s.fresh());
    if replaced.len() >= KEPT
        && let Some(oldest) = replaced.iter().min_by_key(|(_, s)| s.at).map(|(p, _)| p.clone())
    {
        replaced.remove(&oldest);
    }
    // The watcher may name it by its real path (`/tmp` is `/private/tmp` on macOS).
    if let Ok(real) = std::fs::canonicalize(path)
        && real != path
    {
        replaced.insert(real, save);
    }
    replaced.insert(path.to_path_buf(), save);
}

/// What `path`, heard by the watcher, is to us.
pub(crate) fn heard(path: &Path) -> Heard {
    if is_temporary(path) {
        return Heard::Temporary;
    }
    let (file, holder) = {
        let replaced = REPLACED.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(replaced) = replaced.as_ref() else { return Heard::Other };
        let file = replaced.get(path).filter(|s| s.fresh()).map(|s| (*s, s.file));
        let holder = file.is_none().then(|| {
            replaced
                .iter()
                .filter(|(f, s)| s.fresh() && f.parent() == Some(path))
                .map(|(_, s)| (*s, s.folder))
                .collect::<Vec<_>>()
        });
        (file, holder.unwrap_or_default())
    };
    // Deleted, moved away or made again since: an entry change after all (the identity
    // read outside the lock).
    if let Some((save, known)) = file {
        return if path.is_file() && save.same(known, identity(path)) {
            Heard::Replaced
        } else {
            Heard::Other
        };
    }
    if !holder.is_empty() && path.is_dir() {
        let now = identity(path);
        if holder.iter().any(|(save, known)| save.same(*known, now)) {
            return Heard::Holder;
        }
    }
    Heard::Other
}

#[cfg(test)]
#[path = "../tests/unit/own_saves.rs"]
mod tests;
