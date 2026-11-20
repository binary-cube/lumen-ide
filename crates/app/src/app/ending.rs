//! The app ending (its last window closed, or Quit from it): what it started (terminals'
//! shells, agents, the helpers they detached) stopped while the window still shows, so
//! nothing outlives it — on macOS such a leftover keeps the app "running in the
//! background" until the system closes it itself and asks the user about it. Taken while
//! the tree still hangs together, stopped on a thread of its own (each asked, then made
//! to after a grace), shown in the closing question's window as a progress bar and the
//! programs' list; nothing closes that window meanwhile, and the window closes (or the app
//! quits) by itself once they have all ended. macOS only (elsewhere a browser a file was
//! opened in can be the app's child): the ending goes on at once there.

use std::sync::{Mutex, PoisonError};

use workbench_model::running::{Closing, ProgramState, RunningPrompt, Stopping, StoppingProgram};

use super::*;

/// How long a program has, once asked, to end by itself before it's made to.
const GRACE: Duration = Duration::from_secs(2);

/// Once all have ended, the window stays this long (the last of them seen ending).
const LINGER: Duration = Duration::from_millis(450);

/// The window shows at least this long (never a flash).
const LEAST: Duration = Duration::from_millis(800);

/// What the app started being stopped as it ends.
#[derive(Debug)]
pub(super) struct Ending {
    /// Where each is, written by the stopping thread.
    state: Arc<Mutex<Stopping>>,
    /// What's shown (compared to tell a change).
    shown: Stopping,
    /// What follows: the window closing, the app quitting.
    then: Control,
    started: Instant,
    /// When all were seen ended.
    ended: Option<Instant>,
}

impl EditorApp {
    /// The ending `then` (closing this, the last window; quitting from it) held while what
    /// the app started is stopped: `true` if it is (the ending goes on by itself after,
    /// [`Self::follow_ending`]), `false` with nothing to stop or another window still open
    /// (whose work isn't asked about yet: stopped as the app exits).
    pub(super) fn begin_ending(&mut self, then: Control) -> bool {
        // A real window only: never in tests (the test runner's other programs aren't
        // this app's to stop) nor in a snapshot.
        let Some(waker) = self.window.as_ref().map(|w| w.waker()) else { return false };
        if !cfg!(target_os = "macos") || self.shared.windows.get() > 1 {
            return false;
        }
        let found = process::descendants::Descendants::of(std::process::id());
        if found.is_empty() {
            return false;
        }
        let programs = found
            .found()
            .iter()
            .map(|f| StoppingProgram {
                name: f.name.clone(),
                pid: f.pid,
                state: ProgramState::Asked,
            })
            .collect();
        let shown = Stopping { programs };
        tracing::info!(programs = shown.programs.len(), "the app ends: what it started stopped");
        let state = Arc::new(Mutex::new(shown.clone()));
        let writes = Arc::clone(&state);
        let spawned = std::thread::Builder::new().name("app-ending".into()).spawn(move || {
            found.stop_each(GRACE, |pid, ended| {
                let mut state = writes.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(p) = state.programs.iter_mut().find(|p| p.pid == pid) {
                    p.state = match ended {
                        process::descendants::Ended::Asked => ProgramState::Ended,
                        process::descendants::Ended::Forced => ProgramState::Forced,
                    };
                }
                drop(state);
                waker();
            });
        });
        if spawned.is_err() {
            tracing::warn!("the app ends: no thread to stop what it started (stopped as it exits)");
            return false;
        }
        let prompt = RunningPrompt {
            closing: Closing::App,
            groups: Vec::new(),
            projects: Vec::new(),
            modules: Vec::new(),
            waiting: false,
            stopping: Some(shown.clone()),
        };
        self.workbench.close_running_prompt();
        self.workbench.open_ending_prompt(&prompt);
        self.pending.ending =
            Some(Ending { state, shown, then, started: Instant::now(), ended: None });
        self.redraw();
        true
    }

    /// The app is ending: nothing else closes it meanwhile (a window's ×, Quit again).
    pub(super) fn ending(&self) -> bool {
        self.pending.ending.is_some()
    }

    /// The ending at `now`: what has ended shown; once all have (and the window has shown
    /// long enough), the ending goes on.
    pub(super) fn follow_ending(&mut self, now: Instant) {
        let Some(ending) = &mut self.pending.ending else { return };
        let state = ending.state.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if state != ending.shown {
            ending.shown = state;
            let prompt = RunningPrompt {
                closing: Closing::App,
                groups: Vec::new(),
                projects: Vec::new(),
                modules: Vec::new(),
                waiting: false,
                stopping: Some(ending.shown.clone()),
            };
            self.workbench.update_running_prompt(&prompt);
            self.redraw();
        }
        let Some(ending) = &mut self.pending.ending else { return };
        if !ending.shown.done() {
            return;
        }
        let ended = *ending.ended.get_or_insert(now);
        let due = (ended + LINGER).max(ending.started + LEAST);
        if now < due {
            // Woken again when it's due.
            if let Some(window) = &self.window {
                window.request_redraw_at(due);
            }
            return;
        }
        let Some(ending) = self.pending.ending.take() else { return };
        let forced = ending.shown.programs.iter().filter(|p| p.state == ProgramState::Forced);
        tracing::info!(
            programs = ending.shown.programs.len(),
            forced = forced.count(),
            took = ?now.saturating_duration_since(ending.started),
            "the app ends: what it started has ended"
        );
        self.pending.exit_request = Some(ending.then);
    }
}
