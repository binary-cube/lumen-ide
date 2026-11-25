//! The shells running, for the rest of the module: their processes (measured by the Task
//! Manager, ended with their views), their projects, the live ones counted; and a terminal
//! known by its key, renamed or asked whether it still runs.

use super::*;

/// The terminals known by a key, each its name slot (set by [`rename`], taken by the view at
/// its poll): weak, gone with their views.
pub(crate) static NAMED: Mutex<Vec<(String, std::sync::Weak<NameSlot>)>> = Mutex::new(Vec::new());

/// What's asked of a terminal by its key, waiting for its view (a new name, to sleep), and
/// what its view tells (its program still runs, when it was last shown, what it holds).
pub(crate) struct Asked {
    pub(crate) name: Option<String>,
    pub(crate) running: bool,
    /// The project it was opened in (whose window is woken to put it to sleep).
    pub(crate) project: Option<kernel::ProjectId>,
    /// Its tab is on screen now (told by the app: it never sleeps then).
    pub(crate) visible: bool,
    /// When it was last on screen (drawn, or hidden at; `None`: never yet).
    pub(crate) shown: Option<std::time::Instant>,
    /// Text typed since the last ↵ (not sent yet: never lost to a sleep).
    pub(crate) typed: bool,
    /// What tells how long its program has been quiet, while it runs.
    pub(crate) watch: Option<Arc<dyn terminal_session::SessionWatch>>,
    /// Its program's process id while it runs (measured with what it started).
    pub(crate) pid: Option<u32>,
    /// Its program stopped, its tab kept: resumed when shown.
    pub(crate) asleep: bool,
    /// Put to sleep: how its program starts again, and when that was asked (taken at its
    /// view's poll; let go if it was drawn since).
    pub(crate) sleep: Option<(agents::Launch, std::time::Instant)>,
    /// That order is the user's (by hand: whatever the idle time, shown or not).
    pub(crate) by_hand: bool,
    /// Asked to wake (from elsewhere: the sessions' panel), taken at its view's poll.
    pub(crate) wake: bool,
    /// Waking up: asked to by the user, or its program started again and not settled yet
    /// (the menus offer nothing to do meanwhile).
    pub(crate) waking: bool,
    /// Its tab is in the editor (not the bottom window): brought to the front there.
    pub(crate) in_editor: bool,
    /// How its program resumes, as its owner last said (asked now and then off the menus:
    /// they read only this; `None`: nobody knows, it can't sleep).
    pub(crate) resume: Option<agents::Launch>,
    /// Its memory with everything it started, as last measured.
    pub(crate) memory: Option<u64>,
    /// What wakes its window (given at its view's paint): an order from elsewhere is taken
    /// at once, not at an unrelated poll.
    pub(crate) waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Its tab's title (its program's row in the Task Manager says it).
    pub(crate) title: Option<String>,
    /// How it stood when that was last published ([`announce`]).
    pub(crate) announced: Option<contracts::terminal::TerminalState>,
}

impl std::fmt::Debug for Asked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asked")
            .field("running", &self.running)
            .field("asleep", &self.asleep)
            .finish_non_exhaustive()
    }
}

impl Default for Asked {
    /// Made to run a program: running until its view says otherwise.
    fn default() -> Self {
        Self {
            name: None,
            running: true,
            project: None,
            visible: false,
            shown: None,
            typed: false,
            watch: None,
            pid: None,
            asleep: false,
            sleep: None,
            by_hand: false,
            wake: false,
            waking: false,
            in_editor: false,
            resume: None,
            memory: None,
            title: None,
            waker: None,
            announced: None,
        }
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

/// Whether some live terminal is known by a key (asked by the watching thread to go on).
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

/// What the terminal known by `key` holds now (`None`: no such one).
pub(crate) fn state(key: &str) -> Option<contracts::terminal::TerminalState> {
    let slot = named(key)?;
    let state = asked(&slot).state();
    Some(state)
}

/// The terminal in `slot`, known by `key`, published as it stands now on its project's bus
/// ([`contracts::terminal::TerminalStateChanged`]) — only if that changed since it last was.
/// Cheap (its lock and a comparison): called wherever its state may have moved.
pub(crate) fn announce(slot: &NameSlot, key: &str) {
    let (project, state) = {
        let mut asked = asked(slot);
        let state = asked.state();
        if asked.announced == Some(state) {
            return;
        }
        asked.announced = Some(state);
        (asked.project, state)
    };
    // Published with the slot's lock let go (its listeners may ask it again).
    let event =
        contracts::terminal::TerminalStateChanged { key: key.to_owned(), state: Some(state) };
    crate::sleep::publish(project, &event);
}

/// The terminal known by `key` told again as it stands, changed or not: what was asked of it
/// was refused (what shows it may have guessed the answer).
pub(crate) fn announce_again(key: &str) {
    let Some(slot) = named(key) else { return };
    asked(&slot).announced = None;
    announce(&slot, key);
}

/// The terminal known by `key`, of project `project`, gone (its tab closed): told on its bus.
pub(crate) fn announce_gone(project: Option<kernel::ProjectId>, key: &str) {
    let event = contracts::terminal::TerminalStateChanged { key: key.to_owned(), state: None };
    crate::sleep::publish(project, &event);
}

/// How long an order to sleep stays good: one not taken by then is let go (never applied
/// later, when its tab is shown).
pub(crate) const SLEEP_ORDER_FRESH: std::time::Duration = std::time::Duration::from_secs(20);

impl Asked {
    /// How it stands now, as told to others (from memory only).
    pub(crate) fn state(&self) -> contracts::terminal::TerminalState {
        contracts::terminal::TerminalState {
            memory: self.memory,
            asleep: self.asleep,
            can_sleep: self.can_sleep(),
        }
    }

    /// Whether it can be put to sleep by hand now (from memory only).
    pub(crate) fn can_sleep(&self) -> contracts::terminal::CanSleep {
        use contracts::terminal::CanSleep;
        // On its way: the menus say so, greyed out, until it's there.
        if self.wake || self.waking {
            return CanSleep::Waking;
        }
        if self.sleep.is_some() {
            return CanSleep::Sleeping;
        }
        if self.resume.is_none() || self.asleep || !self.running || self.pid.is_none() {
            return CanSleep::No;
        }
        // By hand, the user chose: only text typed and not sent stops it (never lost).
        if self.typed { CanSleep::Typed } else { CanSleep::Yes }
    }

    /// Put to sleep by hand now (its view stops it at its poll): `false` if it can't now.
    pub(crate) fn sleep_by_hand(&mut self) -> bool {
        if self.can_sleep() != contracts::terminal::CanSleep::Yes {
            return false;
        }
        let Some(launch) = self.resume.clone() else { return false };
        self.sleep = Some((launch, std::time::Instant::now()));
        self.by_hand = true;
        true
    }
}

/// The terminal known by `key` put to sleep by hand (`false`: no such one, or not now).
pub(crate) fn sleep_by_key(key: &str) -> bool {
    let Some(slot) = named(key) else { return false };
    let waker = {
        let mut asked = asked(&slot);
        if !asked.sleep_by_hand() {
            return false;
        }
        asked.waker.clone()
    };
    announce(&slot, key);
    // Its window woken (its view takes the order at the poll that follows), not left for
    // an unrelated one; called with the slot's lock let go.
    if let Some(waker) = waker {
        waker();
    }
    true
}

/// Where the sleeping terminal known by `key` is: `Some(in_editor)` if it's asleep, `None`
/// if not or no such one. Nothing changed (asked before its tab is shown).
pub(crate) fn asleep_where(key: &str) -> Option<bool> {
    let slot = named(key)?;
    let asked = asked(&slot);
    asked.asleep.then_some(asked.in_editor)
}

/// Whether a wake waits for the terminal known by `key` (the tests).
#[cfg(test)]
pub(crate) fn wake_pending(key: &str) -> bool {
    named(key).is_some_and(|slot| asked(&slot).wake)
}

/// The terminal known by `key` woken (at its view's poll): `Some(in_editor)` if it's asleep
/// (whether its tab is in the editor, to bring it to the front), `None` if not or no such one.
pub(crate) fn wake_by_key(key: &str) -> Option<bool> {
    let slot = named(key)?;
    let mut asked = asked(&slot);
    if !asked.asleep {
        return None;
    }
    asked.wake = true;
    let in_editor = asked.in_editor;
    let waker = asked.waker.clone();
    drop(asked);
    announce(&slot, key);
    if let Some(waker) = waker {
        waker();
    }
    Some(in_editor)
}

/// The terminal known by `key` marked asleep (the tests: no program needed).
#[cfg(test)]
pub(crate) fn asleep_for_tests(key: &str) {
    if let Some(slot) = named(key) {
        asked(&slot).asleep = true;
    }
}

/// Whether the keyed terminal with the keyboard sleeps now.
pub(crate) fn focused_asleep() -> bool {
    let focused = with_focused(|f| f.upgrade());
    focused.is_some_and(|slot| asked(&slot).asleep)
}

/// The keyed terminal that last had the keyboard (drawn focused): what "Put Session to Sleep"
/// acts on.
static FOCUSED: Mutex<std::sync::Weak<NameSlot>> = Mutex::new(std::sync::Weak::new());

/// [`FOCUSED`], given to `f` (in the tests, each thread's own: they draw in parallel).
pub(crate) fn with_focused<R>(f: impl FnOnce(&mut std::sync::Weak<NameSlot>) -> R) -> R {
    #[cfg(test)]
    {
        thread_local! {
            static TEST: std::cell::RefCell<std::sync::Weak<NameSlot>> =
                const { std::cell::RefCell::new(std::sync::Weak::new()) };
        }
        let _ = &FOCUSED;
        TEST.with(|focused| f(&mut focused.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        f(&mut FOCUSED.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

/// Whether the keyed terminal with the keyboard can sleep by hand now.
pub(crate) fn focused_can_sleep() -> contracts::terminal::CanSleep {
    let focused = with_focused(|f| f.upgrade());
    focused.map_or(contracts::terminal::CanSleep::No, |slot| asked(&slot).can_sleep())
}

/// How many keyed terminals sleep now.
pub(crate) fn asleep_count() -> usize {
    keyed().iter().filter(|(_, slot)| asked(slot).asleep).count()
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

/// A keyed terminal's view alive: dropped with it, it tells on its project's bus that the
/// terminal is gone (what shows it lets it go at once, never left mid-way).
#[derive(Debug)]
pub(crate) struct Gone {
    pub(crate) key: String,
    pub(crate) project: Option<kernel::ProjectId>,
}

impl Drop for Gone {
    fn drop(&mut self) {
        announce_gone(self.project, &self.key);
    }
}
