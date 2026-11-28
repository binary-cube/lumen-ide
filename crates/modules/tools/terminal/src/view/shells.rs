//! The shells running, for the rest of the module: their processes (measured by the Task
//! Manager, ended with their views), their projects, the live ones counted; and a terminal
//! known by its key, renamed or asked whether it still runs. How a keyed terminal sleeps is
//! its sleeping slot's (`ui_components::sleep`); what's kept here is the terminal's own.

use super::*;

/// The terminals known by a key, each its name slot (set by [`rename`], taken by the view at
/// its poll): weak, gone with their views.
pub(crate) static NAMED: Mutex<Vec<(String, std::sync::Weak<NameSlot>)>> = Mutex::new(Vec::new());

/// What's asked of a terminal by its key, waiting for its view (a new name), and what its
/// view tells (its program still runs, its process, what was typed, how it resumes).
#[derive(Default)]
pub(crate) struct Asked {
    pub(crate) name: Option<String>,
    /// Its program runs (started, not ended; asleep, it doesn't).
    pub(crate) running: bool,
    /// The project it was opened in (its owner is asked on that project's clock).
    pub(crate) project: Option<kernel::ProjectId>,
    /// Text typed since the last ↵ (not sent yet: never lost to a sleep).
    pub(crate) typed: bool,
    /// What tells how long its program has been quiet, while it runs.
    pub(crate) watch: Option<Arc<dyn terminal_session::SessionWatch>>,
    /// Its program's process id while it runs (measured with what it started).
    pub(crate) pid: Option<u32>,
    /// When its program last started (how long it's been "starting").
    pub(crate) started: Option<std::time::Instant>,
    /// How its program resumes, as its owner last said (`None`: nobody knows, it can't
    /// sleep).
    pub(crate) resume: Option<agents::Launch>,
    /// Its owner's record was written within the idle time (a turn may run): it waits.
    pub(crate) owner_busy: bool,
    /// Its tab's title (its program's row in the Task Manager says it).
    pub(crate) title: Option<String>,
    /// Its sleeping slot (what the sleeping reads, what it's told of the terminal's reasons).
    pub(crate) sleeping: Option<Arc<ui_components::sleep::Slot>>,
}

impl std::fmt::Debug for Asked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asked").field("running", &self.running).finish_non_exhaustive()
    }
}

pub(crate) type NameSlot = Mutex<Asked>;

/// Every live terminal known by a key, with its key (those gone let go first).
pub(crate) fn keyed() -> Vec<(String, Arc<NameSlot>)> {
    let mut named = NAMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    named.retain(|(_, slot)| slot.strong_count() > 0);
    named.iter().filter_map(|(k, s)| Some((k.clone(), s.upgrade()?))).collect()
}

/// The keyed terminals' programs running now, each with its tab's title.
pub(crate) fn program_titles() -> Vec<(u32, String)> {
    keyed()
        .iter()
        .filter_map(|(_, slot)| {
            let asked = asked(slot);
            Some((asked.pid?, asked.title.clone()?))
        })
        .collect()
}

/// Whether some live terminal is known by a key.
pub(crate) fn any_keyed() -> bool {
    NAMED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|(_, slot)| slot.strong_count() > 0)
}

/// Locked, a poisoned lock taken as it is.
pub(crate) fn asked(slot: &NameSlot) -> std::sync::MutexGuard<'_, Asked> {
    slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The longest name a tab is given by its key.
pub(super) const TITLE_MOST: usize = 200;

/// The live terminal known by `key` (those gone let go first: a tab closed and opened again
/// with the same key is found, not its old one).
pub(crate) fn named(key: &str) -> Option<Arc<NameSlot>> {
    let mut named = NAMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    named.retain(|(_, slot)| slot.strong_count() > 0);
    named.iter().filter(|(k, _)| k == key).find_map(|(_, s)| s.upgrade())
}

/// The terminal known by `key` called `title`: `false` if there's none (its tab closed).
pub(crate) fn rename(key: &str, title: &str) -> bool {
    let Some(slot) = named(key) else { return false };
    let title: String = title.chars().take(TITLE_MOST).collect();
    slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).name = Some(title);
    true
}

/// Whether the program of the terminal known by `key` still runs (`false`: no such one).
pub(crate) fn running(key: &str) -> bool {
    named(key)
        .is_some_and(|slot| slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running)
}

/// A running shell: its process id (the Task Manager measures it, with what it runs), its
/// project, and what tells what runs in it (asked before closing).
#[derive(Clone)]
pub(crate) struct Shell {
    pub pid: u32,
    pub project: Option<kernel::ProjectId>,
    pub watch: Option<std::sync::Arc<dyn terminal_session::SessionWatch>>,
}

/// The running shells.
pub(crate) static SHELLS: std::sync::Mutex<Vec<Shell>> = std::sync::Mutex::new(Vec::new());

/// The running shells' process ids.
pub(crate) fn shell_pids() -> Vec<u32> {
    SHELLS.lock().map(|s| s.iter().map(|s| s.pid).collect()).unwrap_or_default()
}

/// The shells asked to close (the Task Manager's "Close the session", through the module):
/// their views end ([`Viewer::closed`]), their tabs close and their sessions with them.
pub(crate) static CLOSING: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// The running shells' projects, by process id (those opened in one).
pub(crate) fn shell_projects() -> Vec<(u32, kernel::ProjectId)> {
    SHELLS
        .lock()
        .map(|s| s.iter().filter_map(|s| Some((s.pid, s.project?))).collect())
        .unwrap_or_default()
}

/// Shell `pid` (a running one) asked to close: `false` if it isn't one.
pub(crate) fn close_shell(pid: u32) -> bool {
    let running = SHELLS.lock().is_ok_and(|s| s.iter().any(|s| s.pid == pid));
    if running && let Ok(mut closing) = CLOSING.lock() {
        closing.push(pid);
    }
    running
}

/// A shell's process id among [`SHELLS`], for as long as its view holds it.
#[derive(Debug)]
pub(super) struct ShellPid(pub(super) u32);

impl ShellPid {
    pub(super) fn new(shell: Shell) -> Self {
        let pid = shell.pid;
        if let Ok(mut shells) = SHELLS.lock() {
            shells.push(shell);
        }
        Self(pid)
    }
}

impl Drop for ShellPid {
    fn drop(&mut self) {
        if let Ok(mut shells) = SHELLS.lock() {
            shells.retain(|s| s.pid != self.0);
        }
        if let Ok(mut closing) = CLOSING.lock() {
            closing.retain(|p| *p != self.0);
        }
    }
}

/// One of the [`LIVE`] shells (of a project's, [`LIVE_IN`]), for as long as it's held.
#[derive(Debug)]
pub(super) struct Live(pub(super) Option<kernel::ProjectId>);

impl Live {
    pub(super) fn new(project: Option<kernel::ProjectId>) -> Self {
        LIVE.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut all) = LIVE_IN.lock() {
            *all.entry(project).or_default() += 1;
        }
        Self(project)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
        if let Ok(mut all) = LIVE_IN.lock()
            && let Some(n) = all.get_mut(&self.0)
        {
            *n = n.saturating_sub(1);
            if *n == 0 {
                all.remove(&self.0);
            }
        }
    }
}

/// The shells running now in `project`.
pub(crate) fn live_in(project: Option<kernel::ProjectId>) -> usize {
    LIVE_IN.lock().ok().and_then(|all| all.get(&project).copied()).unwrap_or(0)
}
