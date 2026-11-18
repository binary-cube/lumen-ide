//! The app's sessions (plan 78), kept in `sessions.toml` beside the recent projects: this
//! run's windows recorded as they change (looked at most every [`LOOK_EVERY`], written on a
//! worker at most every [`WRITE_EVERY`], so a crash leaves them nearly as they were), marked
//! ended when the app quits; the past ones read once at start, for the home screen to show
//! and reopen. Each write reads the file again and keeps what another instance of the app
//! wrote there (its own session), replacing only this one's.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use workspace::sessions::{SessionWindow, Sessions};

/// How often the windows are looked at at most (an event between two looks changes nothing
/// here: the next look sees it).
const LOOK_EVERY: Duration = Duration::from_secs(1);
/// How often the running session is written at most (the latest state, when the interval
/// passes).
const WRITE_EVERY: Duration = Duration::from_secs(2);
/// The worker the file is written on.
pub const THREAD: &str = "sessions";

/// The sessions before this run (read at start, as they were then): the home screen's.
static PAST: OnceLock<Arc<Sessions>> = OnceLock::new();

/// The sessions before this run, newest first: read the first time they're asked for (a
/// small file), then kept as they were.
pub fn past() -> Arc<Sessions> {
    let read = || Arc::new(file().map(|f| storage::app_sessions::load(&f)).unwrap_or_default());
    Arc::clone(PAST.get_or_init(read))
}

fn file() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("sessions.toml"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The sessions and when they were last written (held while writing, so an older state is
/// never written over a newer one); this run's id and how many are kept.
#[derive(Debug, Default)]
struct Stored {
    sessions: Sessions,
    written: Option<Instant>,
    id: u128,
    kept: usize,
}

/// This run's session and the ones before it.
#[derive(Debug)]
pub struct AppSessions {
    /// This run's identity.
    pub id: u128,
    stored: Arc<Mutex<Stored>>,
    /// A write is waiting on the worker (it writes the latest state when it runs).
    queued: Arc<AtomicBool>,
    looked: Option<Instant>,
    /// The app quit: nothing more is recorded (its windows closing aren't a change).
    ended: bool,
}

impl AppSessions {
    /// This run's session, beside the past ones ([`past`]).
    pub fn load() -> Self {
        let sessions = (*past()).clone();
        let id = kernel::id::new_uuid();
        Self {
            id,
            stored: Arc::new(Mutex::new(Stored { sessions, written: None, id, kept: 10 })),
            queued: Arc::new(AtomicBool::new(false)),
            looked: None,
            ended: false,
        }
    }

    /// Whether the windows should be looked at now (`force`: a window opened or closed).
    pub fn due(&self, force: bool) -> bool {
        !self.ended && (force || self.looked.is_none_or(|at| at.elapsed() >= LOOK_EVERY))
    }

    /// The windows now, `kept` sessions at most: a change is written soon.
    pub fn record(&mut self, windows: Vec<SessionWindow>, kept: usize) {
        if self.ended {
            return;
        }
        self.looked = Some(Instant::now());
        let changed = {
            let mut stored = self.stored.lock().unwrap_or_else(PoisonError::into_inner);
            let changed = stored.sessions.record(self.id, now(), windows);
            let before = stored.sessions.list.len();
            stored.sessions.keep(kept);
            stored.kept = kept;
            changed || stored.sessions.list.len() != before
        };
        if changed {
            self.write_soon();
        }
    }

    /// The app quits as it should: this session ended, written before it does.
    pub fn end(&mut self) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        let mut stored = self.stored.lock().unwrap_or_else(PoisonError::into_inner);
        if stored.sessions.end(self.id)
            && let Some(path) = file()
        {
            save(&path, &mut stored);
        }
    }

    /// The latest state written on the worker, once [`WRITE_EVERY`] passed since the last
    /// write (one write waiting at a time).
    fn write_soon(&self) {
        let Some(path) = file() else { return };
        if self.queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let (stored, queued) = (Arc::clone(&self.stored), Arc::clone(&self.queued));
        kernel::work::run(THREAD, move || {
            let wait = {
                let s = stored.lock().unwrap_or_else(PoisonError::into_inner);
                s.written.map_or(Duration::ZERO, |at| WRITE_EVERY.saturating_sub(at.elapsed()))
            };
            std::thread::sleep(wait);
            queued.store(false, Ordering::Release);
            let mut s = stored.lock().unwrap_or_else(PoisonError::into_inner);
            save(&path, &mut s);
        });
    }
}

/// This run's session written beside what the file has now (another instance's).
fn save(path: &std::path::Path, stored: &mut Stored) {
    stored.written = Some(Instant::now());
    let disk = storage::app_sessions::load(path);
    stored.sessions = stored.sessions.merged(stored.id, disk, stored.kept.max(1));
    if let Err(e) = storage::app_sessions::save(path, &stored.sessions) {
        tracing::warn!("the session couldn't be written: {e}");
    }
}
