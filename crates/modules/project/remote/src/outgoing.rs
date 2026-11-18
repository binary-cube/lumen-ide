//! What's to upload, read once for both the Remote button's count and the "To upload" tab:
//! what couldn't go (and why), what changed here or was made here and never sent, and —
//! uploads waiting for the user — what was moved or deleted here (the outbox). Reading it
//! walks the local copy, so it's background work: one read at a time on a pool's thread
//! (gone when idle), the requests close together taken as one, a read asked during another
//! made right after it.

use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use kernel::work::{self, Pool, Priority};
use remote::application::{LocalFiles, Standing};

use workbench_model::BadgeKind;

use crate::session::State;

/// How long a read waits for the requests to calm down (a save is followed by its upload,
/// a paste by many changes).
const QUIET: Duration = Duration::from_millis(300);

/// Why a file is to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// It couldn't go: the reason (offline, an error).
    Waiting(String),
    /// Changed here since its last transfer.
    Changed,
    /// Made here, never sent.
    New,
    /// Moved here: `from` is its name on the server (its content may have changed too).
    Renamed { from: String },
    /// Deleted here; the server still has it.
    Deleted,
    /// Changed here and on the server since its last transfer: the user decides.
    Conflict,
}

/// The files to upload, each with why: what couldn't go, then what's changed or new here
/// (each once, by path).
pub type Rows = Arc<Vec<(String, Why)>>;

/// What's to upload, as last read (see the module's doc).
#[derive(Debug, Default)]
pub struct Outgoing {
    list: Mutex<Listed>,
    /// Its reads, one at a time (this project's: another's never waits for them).
    reads: std::sync::OnceLock<Pool>,
}

#[derive(Debug, Default)]
struct Listed {
    rows: Rows,
    /// Bumped at each read (0: never read yet).
    generation: u64,
    reading: bool,
    /// Asked again since the read going on started.
    stale: bool,
}

impl Outgoing {
    /// The list as last read, and its generation (0: not read yet).
    pub fn rows(&self) -> (u64, Rows) {
        let list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        (list.generation, Arc::clone(&list.rows))
    }

    /// A read is going on (or asked).
    pub fn busy(&self) -> bool {
        let list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        list.reading || list.stale
    }

    /// How many, and what the count means: one couldn't go (an error), one is in conflict
    /// (a warning), else what waits (`None`: not read yet).
    pub fn summary(&self) -> Option<(usize, BadgeKind)> {
        let list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        let any = |f: fn(&Why) -> bool| list.rows.iter().any(|(_, why)| f(why));
        let kind = if any(|w| matches!(w, Why::Waiting(_))) {
            BadgeKind::Error
        } else if any(|w| matches!(w, Why::Conflict)) {
            BadgeKind::Warning
        } else {
            BadgeKind::Count
        };
        (list.generation > 0).then_some((list.rows.len(), kind))
    }

    /// What's listed at or under `scope` (relative; `""`: everything), from the last read.
    pub fn under(&self, scope: &str) -> Vec<(String, Why)> {
        let list = self.list.lock().unwrap_or_else(PoisonError::into_inner);
        list.rows
            .iter()
            .filter(|(path, _)| remote::application::in_scope(path, scope))
            .cloned()
            .collect()
    }
}

/// Reads the list again: now, or right after the read going on.
pub fn refresh(state: &Arc<State>) {
    {
        let mut list = state.outgoing.list.lock().unwrap_or_else(PoisonError::into_inner);
        list.stale = true;
        if list.reading {
            return;
        }
        list.reading = true;
    }
    // Held weakly: a project closed meanwhile isn't read.
    let weak = Arc::downgrade(state);
    let reads = state.outgoing.reads.get_or_init(|| Pool::new("remote-pending", 1, work::IDLE));
    reads.spawn(Priority::Background, None, move || read(&weak));
}

fn read(weak: &Weak<State>) {
    // A panic ends the read too: the next request starts one.
    struct Done(Weak<State>);
    impl Drop for Done {
        fn drop(&mut self) {
            if std::thread::panicking()
                && let Some(state) = self.0.upgrade()
            {
                state.outgoing.list.lock().unwrap_or_else(PoisonError::into_inner).reading = false;
            }
        }
    }
    let _done = Done(weak.clone());
    loop {
        std::thread::sleep(QUIET);
        let Some(state) = weak.upgrade() else { return };
        state.outgoing.list.lock().unwrap_or_else(PoisonError::into_inner).stale = false;
        let rows = rows(&state);
        // Asked again meanwhile: read again; else done (under the lock: a request after
        // this sees the read ended and starts one).
        let again = {
            let mut list = state.outgoing.list.lock().unwrap_or_else(PoisonError::into_inner);
            // What came or went: its folders' marks shown again in the tree.
            let changed: Vec<&String> = rows
                .iter()
                .filter(|r| !list.rows.contains(r))
                .chain(list.rows.iter().filter(|r| !rows.contains(r)))
                .map(|(path, _)| path)
                .collect();
            state.relist_around(changed.into_iter().map(String::as_str));
            list.rows = Arc::new(rows);
            list.generation += 1;
            list.reading = list.stale;
            list.stale
        };
        (state.wake())();
        if !again {
            return;
        }
    }
}

/// What's to upload now (reads the disk): what couldn't go, what's changed or new here
/// (a moved file: its move), then what was deleted here.
pub(crate) fn rows(state: &State) -> Vec<(String, Why)> {
    let waiting = state.to_upload();
    let conflicts = state.conflicts.lock().map(|c| c.clone()).unwrap_or_default();
    let mut rows: Vec<(String, Why)> = conflicts
        .iter()
        .filter(|path| !waiting.contains_key(*path))
        .map(|path| (path.clone(), Why::Conflict))
        .collect();
    rows.extend(waiting.iter().map(|(path, why)| (path.clone(), Why::Waiting(why.clone()))));
    // A file made anew where one was deleted is an edit of the server's, no longer a deletion.
    let local = crate::session::local_copy(&state.saved.local);
    state.change_outbox(|o| {
        let back: Vec<String> =
            o.deleted().filter(|d| local.stat(d).is_some()).map(str::to_owned).collect();
        for path in back {
            o.revived(&path);
        }
    });
    let outbox = state.outbox.lock().map(|o| o.clone()).unwrap_or_default();
    let mut moved: Vec<(String, String)> =
        outbox.renamed().map(|(here, there)| (here.to_owned(), there.to_owned())).collect();
    for (path, standing) in state.local_changes() {
        if waiting.contains_key(&path) || conflicts.contains(&path) {
            continue;
        }
        let why = match outbox.server_name(&path) {
            Some(from) => Why::Renamed { from: from.to_owned() },
            None if standing == Standing::OnlyHere => Why::New,
            None => Why::Changed,
        };
        moved.retain(|(here, _)| *here != path);
        rows.push((path, why));
    }
    // Moves of files unchanged otherwise (the manifest knows them by their old name: listed
    // as new above, unless the server has their name here too).
    for (here, from) in moved {
        if local.stat(&here).is_some() && !waiting.contains_key(&here) && state.travels(&here) {
            rows.push((here, Why::Renamed { from }));
        }
    }
    rows.extend(
        outbox.deleted().filter(|p| state.travels(p)).map(|path| (path.to_owned(), Why::Deleted)),
    );
    rows
}
