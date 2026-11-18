//! The app's sessions (plan 78): what each run of the app had open — its windows, each with
//! its project (or none: files opened alone) and its files — newest first, a few kept. The
//! home screen shows the past ones (a tree) to reopen; the current one is written as it
//! changes, so a crash leaves it as it was. Pure: the app records and stores it.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A window of a session: its project (`None`: files opened alone) and its open files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionWindow {
    pub project: Option<PathBuf>,
    pub files: Vec<PathBuf>,
}

/// A run of the app: when it started, when it last changed, whether it ended as it should
/// (`clean`: quit, not a crash), its windows (front first).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub id: u128,
    /// Seconds since the Unix epoch.
    pub started: u64,
    pub updated: u64,
    pub clean: bool,
    pub windows: Vec<SessionWindow>,
}

impl Session {
    /// How many files its windows had open.
    pub fn file_count(&self) -> usize {
        self.windows.iter().map(|w| w.files.len()).sum()
    }
}

/// The sessions kept, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sessions {
    pub list: Vec<Session>,
}

impl Sessions {
    /// Session `id`'s windows now (`now`: seconds since the epoch). A new session goes first
    /// once it has something open; nothing open (every window closed, or none yet) leaves
    /// what it had: it's what reopening it brings back. `true`: it changed (to be written).
    pub fn record(&mut self, id: u128, now: u64, windows: Vec<SessionWindow>) -> bool {
        let windows: Vec<SessionWindow> =
            windows.into_iter().filter(|w| w.project.is_some() || !w.files.is_empty()).collect();
        if windows.is_empty() {
            return false;
        }
        match self.list.iter_mut().find(|s| s.id == id) {
            Some(session) if session.windows == windows => false,
            Some(session) => {
                session.windows = windows;
                session.updated = now;
                true
            }
            None => {
                let session = Session { id, started: now, updated: now, clean: false, windows };
                self.list.insert(0, session);
                true
            }
        }
    }

    /// Session `id` ended as it should (the app quit). `true`: it changed.
    pub fn end(&mut self, id: u128) -> bool {
        match self.list.iter_mut().find(|s| s.id == id) {
            Some(session) if !session.clean => {
                session.clean = true;
                true
            }
            _ => false,
        }
    }

    /// Only the `most` newest kept.
    pub fn keep(&mut self, most: usize) {
        self.list.truncate(most.max(1));
    }

    /// The sessions before `current` (the one running), newest first.
    pub fn past(&self, current: u128) -> impl Iterator<Item = &Session> {
        self.list.iter().filter(move |s| s.id != current)
    }

    pub fn get(&self, id: u128) -> Option<&Session> {
        self.list.iter().find(|s| s.id == id)
    }

    /// Session `own` as kept here, beside the others as `disk` has them now (another
    /// instance of the app writes its own there too): the most recently changed first,
    /// `most` kept.
    pub fn merged(&self, own: u128, disk: Sessions, most: usize) -> Sessions {
        let mut list: Vec<Session> = disk.list.into_iter().filter(|s| s.id != own).collect();
        list.extend(self.get(own).cloned());
        list.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.started.cmp(&a.started)));
        let mut merged = Sessions { list };
        merged.keep(most);
        merged
    }
}

/// How long a window is waited for at most (to open, then to draw its first frame): one
/// that never shows (minimized, failed) doesn't hold the others back.
pub const REOPEN_WAIT: Duration = Duration::from_secs(10);

/// A session's windows reopened one after the other, kind to the system: the next one only
/// once the one before has drawn its first frame and a pause passed.
#[derive(Debug)]
pub struct Reopen {
    left: VecDeque<SessionWindow>,
    total: usize,
    pause: Duration,
    step: Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The next one opens at this moment.
    Ready(Instant),
    /// One was handed out; its window hasn't appeared yet (since when).
    Opening(Instant),
    /// Its window (`id`) appeared and hasn't drawn yet (since when).
    Drawing(u64, Instant),
}

impl Reopen {
    /// `windows` in order, `limit` at most (0: all), `pause` between one's first frame and
    /// the next one; the first is ready at `now`.
    pub fn new(
        mut windows: Vec<SessionWindow>,
        limit: usize,
        pause: Duration,
        now: Instant,
    ) -> Self {
        if limit > 0 {
            windows.truncate(limit);
        }
        Self { total: windows.len(), left: windows.into(), pause, step: Step::Ready(now) }
    }

    /// The next window to open, if its time came; its window is then waited for.
    pub fn next(&mut self, now: Instant) -> Option<SessionWindow> {
        match self.step {
            Step::Ready(at) if at <= now => {}
            Step::Opening(since) | Step::Drawing(_, since) if now >= since + REOPEN_WAIT => {}
            _ => return None,
        }
        let window = self.left.pop_front()?;
        self.step = Step::Opening(now);
        Some(window)
    }

    /// The window handed out appeared as window `id`.
    pub fn appeared(&mut self, id: u64, now: Instant) {
        if matches!(self.step, Step::Opening(_)) {
            self.step = Step::Drawing(id, now);
        }
    }

    /// The window waited for (`id`) drew its first frame: the next one after the pause.
    pub fn drawn(&mut self, id: u64, now: Instant) {
        if matches!(self.step, Step::Drawing(w, _) if w == id) {
            self.step = Step::Ready(now + self.pause);
        }
    }

    /// The window waited for, if one appeared.
    pub fn waiting_for(&self) -> Option<u64> {
        match self.step {
            Step::Drawing(id, _) => Some(id),
            _ => None,
        }
    }

    /// The window handed out was skipped (its project gone, already open): the next one now.
    pub fn skipped(&mut self, now: Instant) {
        self.step = Step::Ready(now);
    }

    /// When something is due (the loop wakes then): the next one, or the end of a wait.
    pub fn deadline(&self) -> Option<Instant> {
        match self.step {
            Step::Ready(at) => (!self.left.is_empty()).then_some(at),
            Step::Opening(since) | Step::Drawing(_, since) => Some(since + REOPEN_WAIT),
        }
    }

    /// How many were handed out, of how many.
    pub fn progress(&self) -> (usize, usize) {
        (self.total - self.left.len(), self.total)
    }

    /// All handed out, and the last one drawn (or given up on).
    pub fn is_done(&self, now: Instant) -> bool {
        self.left.is_empty()
            && match self.step {
                Step::Ready(_) => true,
                Step::Opening(since) | Step::Drawing(_, since) => now >= since + REOPEN_WAIT,
            }
    }
}

#[cfg(test)]
#[path = "../tests/unit/sessions.rs"]
mod tests;
