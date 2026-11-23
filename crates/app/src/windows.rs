//! All windows in a single process.
//!
//! - **Shared:** fonts and the text system, grammars and modules (`LanguageSupport`),
//!   themes, the GPU (device, glyph atlas, textures), the semantic model.
//! - **Isolated per window / project:** the workbench (tabs, documents), the indexer and
//!   `<project dir>/`, terminals, watchers, the render surface.
//! - **Error isolation:** a panic while handling an event closes only its window;
//!   the others keep going.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

use platform_api::{Application, Control, PlatformEvent, Window, WindowId};

use crate::app::{EditorApp, Shared};
use crate::modules::LanguageSupport;
use crate::workbench::{Ports, Workbench};

pub struct AppWindows {
    shared: Rc<Shared>,
    support: Arc<LanguageSupport>,
    /// The ports each window's workbench reaches the outside through.
    ports: Ports,
    /// The first window's workbench (prepared in `main`, with the command-line arguments).
    first: Option<Workbench>,
    /// The command line gave only a project: the first window reopens its session.
    first_restores: bool,
    windows: Vec<(WindowId, EditorApp)>,
    /// The recent projects' version the app icon's menu shows (`None`: not set yet).
    dock: Option<u64>,
    /// This run's session and the past ones (plan 78).
    sessions: crate::sessions::AppSessions,
    /// A window opened or closed since the session was last recorded.
    windows_changed: bool,
    /// A past session reopening, and the window that asked (its status bar shows it).
    reopen: Option<(workspace::sessions::Reopen, WindowId)>,
    /// The loop woken when the reopening's next step is due.
    reopen_wake: Option<kernel::work::Later>,
    /// A session asked to reopen while what of it still exists is looked at (on a worker),
    /// and the window that asked.
    reopen_found:
        Option<(std::sync::mpsc::Receiver<Vec<workspace::sessions::SessionWindow>>, WindowId)>,
    /// The progress the asking window shows (changed only when it does).
    reopen_shown: Option<(usize, usize)>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The session to reopen once the first window is there (the app started again after an
    /// update: `--reopen-session`).
    reopen_at_start: Option<u128>,
    /// An update in place (its staged folder): the app started again as it ends.
    update: Option<std::path::PathBuf>,
    /// What the app started, taken as its last window goes (while the tree still hangs
    /// together): stopped as it ends.
    leaving: Option<process::descendants::Descendants>,
}

impl AppWindows {
    pub fn new(
        shared: Shared,
        support: Arc<LanguageSupport>,
        ports: Ports,
        first: Workbench,
        first_restores: bool,
        sessions: crate::sessions::AppSessions,
    ) -> Self {
        Self {
            shared: Rc::new(shared),
            support,
            ports,
            first: Some(first),
            first_restores,
            windows: Vec::new(),
            dock: None,
            sessions,
            windows_changed: false,
            reopen: None,
            reopen_wake: None,
            reopen_found: None,
            reopen_shown: None,
            waker: None,
            reopen_at_start: None,
            update: None,
            leaving: None,
        }
    }

    /// Session `id` reopened once the first window is there (started again after an update).
    pub fn reopen_at_start(mut self, id: Option<u128>) -> Self {
        self.reopen_at_start = id;
        self
    }
}

impl Application for AppWindows {
    /// Opened as a later start's (the first window's, once there's one).
    fn paths_opened(&mut self, paths: Vec<std::path::PathBuf>) {
        crate::instance::hand_in(paths);
        self.open_handed();
    }

    fn window_opened(&mut self, window: Arc<dyn Window>) {
        let (workbench, paths, restore) = match self.first.take() {
            Some(workbench) => (workbench, Vec::new(), std::mem::take(&mut self.first_restores)),
            None => {
                let workbench = Workbench::new(Arc::clone(&self.support), self.ports.clone());
                (workbench, self.shared.take_pending(), false)
            }
        };
        let mut app = match EditorApp::new(workbench, Rc::clone(&self.shared)) {
            Ok(app) => app,
            Err(e) => {
                tracing::error!(error = %e, "the window couldn't start");
                return;
            }
        };
        let id = window.id();
        // Later starts' paths wake the loop.
        if self.windows.is_empty() {
            crate::instance::set_waker(window.app_waker());
            crate::recent::set_waker(window.app_waker());
            workbench_model::app_wide::set_waker(window.app_waker());
            crate::peers::set_waker(window.app_waker());
            crate::left_behind::set_waker(window.app_waker());
            self.waker = Some(window.app_waker());
        }
        // Diagnostic: `EDITOR_DEBUG_WINDOWS=N` opens N windows at startup (shared memory).
        if self.windows.is_empty()
            && let Some(n) =
                std::env::var("EDITOR_DEBUG_WINDOWS").ok().and_then(|n| n.parse::<usize>().ok())
        {
            for _ in 1..n {
                self.shared.pending.borrow_mut().push_back(Vec::new());
                window.open_window();
            }
        }
        let opened = catch_unwind(AssertUnwindSafe(|| {
            app.window_opened(window);
            app.open_initial_paths(&paths);
            if restore {
                app.restore_session();
            }
            crate::startup::mark("window's paths opened");
        }));
        match opened {
            Ok(()) => {
                tracing::info!(windows = self.windows.len() + 1, "window opened");
                self.windows.push((id, app));
                self.shared.windows.set(self.windows.len());
                self.windows_changed = true;
                if let Some((reopen, _)) = &mut self.reopen {
                    reopen.appeared(id.0, std::time::Instant::now());
                }
                // Started again after an update: the windows of the session before it.
                if let Some(asked) = self.reopen_at_start.take() {
                    // Not written before the app ended (another instance's file, a crash):
                    // the newest one there instead of a blank window.
                    let past = crate::sessions::past();
                    let session = match past.get(asked) {
                        Some(_) => Some(asked),
                        None => past.past(self.sessions.id).next().map(|s| s.id),
                    };
                    tracing::info!(
                        asked = %kernel::id::uuid_text(asked),
                        found = session == Some(asked),
                        "the session before the update reopened"
                    );
                    if let Some(session) = session {
                        self.shared.reopen_ask.set(Some((session, id)));
                    }
                }
            }
            Err(_) => tracing::error!("the new window crashed on start"),
        }
    }

    fn event(&mut self, id: WindowId, event: PlatformEvent) -> Control {
        let Some(i) = self.windows.iter().position(|(w, _)| *w == id) else {
            return Control::Continue;
        };
        let _busy = crate::watchdog::busy(event_name(&event));
        // The last window asked to close: the session as it is, before its tabs go.
        if self.windows.len() == 1 && matches!(event, PlatformEvent::CloseRequested) {
            self.record_session(true);
        }
        let app = &mut self.windows[i].1;
        let started = std::time::Instant::now();
        let mut crashed = false;
        let control = catch_unwind(AssertUnwindSafe(|| app.event(event))).unwrap_or_else(|_| {
            tracing::error!(?id, "the window crashed; closing it, the others stay open");
            crashed = true;
            Control::Close
        });
        let control = match control {
            Control::Close => {
                // A window's going is timed: its parts are what a slow close comes from.
                let asked = started.elapsed();
                let gone = std::time::Instant::now();
                let project_leaving = if self.windows.len() == 1 {
                    self.leaving = leaving();
                    None
                } else {
                    self.project_leaving(i)
                };
                self.windows.remove(i);
                // Its window gone (its terminals closed): what its programs detached is
                // stopped, on a thread of its own.
                if let Some(left) = project_leaving {
                    stop_project_leaving(left);
                }
                if asked + gone.elapsed() >= std::time::Duration::from_millis(100) {
                    tracing::info!(?asked, dropped = ?gone.elapsed(), "a window's close was slow");
                }
                self.shared.windows.set(self.windows.len());
                self.windows_changed = true;
                // The last window closed: the app quits as it should (unless it crashed: the
                // session stays marked as ended badly).
                if self.windows.is_empty() && !crashed {
                    self.sessions.end();
                }
                Control::Close
            }
            // Quitting the app: each window closes its documents (asking about changes); if one
            // cancels, the app stays open.
            Control::Exit => {
                // The windows as they are before their documents close.
                self.record_session(true);
                let cancelled = self.windows.iter_mut().any(|(_, app)| !app.prepare_quit());
                if cancelled {
                    // Asked for an update: not now (the user kept a window open).
                    for (_, app) in &mut self.windows {
                        app.take_update();
                    }
                    Control::Continue
                } else {
                    // Only the window that quit: an update asked for in another and cancelled
                    // there is never put in place by this Quit.
                    self.update = self.windows.get_mut(i).and_then(|(_, app)| app.take_update());
                    for (_, app) in &mut self.windows {
                        app.take_update();
                    }
                    self.sessions.end();
                    Control::Exit
                }
            }
            Control::Continue => Control::Continue,
        };
        // Shared changes (theme, language, a view over every window) made in one window
        // reach all of them.
        let mut told = None;
        for (_, app) in &mut self.windows {
            told = app.publish_everywhere().or(told);
        }
        match told {
            // Opened or closed here: the app's other instances too.
            Some(wanted) => crate::peers::tell(wanted),
            // Opened or closed in another instance: every window here follows.
            None => {
                if let Some(wanted) = crate::peers::take() {
                    let mut everywhere = self.shared.everywhere.borrow_mut();
                    everywhere.0 += 1;
                    everywhere.1 = wanted;
                }
            }
        }
        for (_, app) in &mut self.windows {
            app.sync_shared();
        }
        self.sync_projects();
        let changed = std::mem::take(&mut self.windows_changed);
        self.record_session(changed);
        self.step_reopen();
        self.open_handed();
        self.offer_left_behind();
        control
    }

    /// The settings still being written behind reach the disk; after an update (in place
    /// already), the app started again (it waits for this process to end, then reopens
    /// this session).
    fn exiting(&mut self) {
        crate::watchdog::exiting();
        storage::settings::flush();
        // Before the app starts again after an update: that one isn't among them.
        if let Some(left) = self.leaving.take().or_else(leaving) {
            stop_leaving(&left);
        }
        if let Some(staged) = self.update.take() {
            tracing::info!(session = %kernel::id::uuid_text(self.sessions.id), "started again after the update");
            crate::updating::restart(&staged, self.sessions.id);
        }
    }
}

/// What this app started and what those started (macOS): taken while they still hang
/// together under it. An agent's helper that detached into its own process group would
/// otherwise outlive the app, and the system, counting it as the app's, would show the app
/// as running in the background and then close it itself, asking the user about it.
/// Elsewhere nothing is taken (a browser a file was opened in can be a child there).
fn leaving() -> Option<process::descendants::Descendants> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    Some(process::descendants::Descendants::of(std::process::id()))
}

impl AppWindows {
    /// The window `i` closing while others stay: what its project's programs started (an
    /// agent's helper detached into its own group would outlive it), taken while it still
    /// hangs together under them (macOS, as [`leaving`]). Nothing when the same project is
    /// open in another window, nor for a window with no project; a project open inside its
    /// folder keeps what ran there; the programs themselves end with the window as before.
    fn project_leaving(&self, i: usize) -> Option<process::descendants::Descendants> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let root = self.windows.get(i)?.1.project_root()?;
        let others: Vec<std::path::PathBuf> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .filter_map(|(_, (_, app))| app.project_root())
            .collect();
        if others.contains(&root) {
            return None;
        }
        let programs = crate::left_behind::project_programs(&root, &others);
        let left = process::descendants::Descendants::under(&programs);
        (!left.is_empty()).then_some(left)
    }
}

/// How long a closed project's helpers have, once asked, to end by themselves.
const PROJECT_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Stops what [`AppWindows::project_leaving`] took, on a thread of its own (the window
/// already gone), saying what in the log.
fn stop_project_leaving(left: process::descendants::Descendants) {
    let spawned = std::thread::Builder::new().name("project-leaving".into()).spawn(move || {
        let started = std::time::Instant::now();
        let forced = left.stop(PROJECT_GRACE);
        tracing::info!(
            programs = left.len(),
            forced,
            took = ?started.elapsed(),
            "a closed project's helpers stopped"
        );
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "can't stop a closed project's helpers");
    }
}

/// How long what the app started has, once asked, to end by itself.
const LEAVING_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// Stops what [`leaving`] took (each still the same program), saying what in the log.
fn stop_leaving(left: &process::descendants::Descendants) {
    if left.is_empty() {
        return;
    }
    let started = std::time::Instant::now();
    let forced = left.stop(LEAVING_GRACE);
    tracing::info!(
        programs = left.len(),
        forced,
        took = ?started.elapsed(),
        "the programs the app started stopped as it ends"
    );
}

/// The pause between a reopened window's first frame and the next one.
const REOPEN_PAUSE: std::time::Duration = std::time::Duration::from_millis(300);

impl AppWindows {
    /// A past session's reopening (plan 78): started when asked, stopped when asked, one
    /// window at a time (the asking window first, if it's blank), the progress on the
    /// asking window's status bar, the loop woken when the next step is due.
    fn step_reopen(&mut self) {
        use std::time::Instant;
        if let Some((session, asked)) = self.shared.reopen_ask.take() {
            // Another one asked meanwhile: that one ends here.
            self.finish_reopen();
            self.shared.reopen_stop.set(false);
            if let Some(session) = crate::sessions::past().get(session) {
                let windows = session.windows.clone();
                let (send, receive) = std::sync::mpsc::channel();
                let waker = self.waker.clone();
                // What still exists, looked at on a worker (a click reads no disk).
                kernel::work::run(crate::sessions::THREAD, move || {
                    let _ = send.send(existing(windows));
                    if let Some(waker) = waker {
                        waker();
                    }
                });
                self.reopen_found = Some((receive, asked));
            }
        }
        if self.shared.reopen_stop.take() {
            self.reopen_found = None;
            self.finish_reopen();
            return;
        }
        if let Some((receive, asked)) = &self.reopen_found
            && let Ok(windows) = receive.try_recv()
        {
            let asked = *asked;
            self.reopen_found = None;
            let limit = crate::config::settings().get(&crate::settings::SESSIONS_REOPEN);
            let limit = usize::try_from(limit).unwrap_or(0);
            let reopen =
                workspace::sessions::Reopen::new(windows, limit, REOPEN_PAUSE, Instant::now());
            self.reopen = Some((reopen, asked));
            self.reopen_shown = None;
        }
        let Some((reopen, asked)) = &mut self.reopen else { return };
        let asked = *asked;
        let now = Instant::now();
        if let Some(id) = reopen.waiting_for()
            && self.windows.iter().any(|(w, app)| w.0 == id && app.first_frame_drawn())
        {
            reopen.drawn(id, now);
        }
        while let Some(window) = reopen.next(now) {
            let paths = reopen_paths(&window, &self.shared.open_projects.borrow());
            let Some(paths) = paths else {
                reopen.skipped(now);
                continue;
            };
            // The asking window, still blank, takes the first one.
            match self.windows.iter_mut().find(|(w, _)| *w == asked) {
                Some((id, app)) if app.is_blank() => {
                    app.open_initial_paths(&paths);
                    reopen.appeared(id.0, now);
                    reopen.drawn(id.0, now);
                }
                _ => match self.windows.first_mut() {
                    Some((_, app)) => app.open_window_with(paths),
                    None => reopen.skipped(now),
                },
            }
            break;
        }
        if reopen.is_done(now) {
            self.finish_reopen();
            return;
        }
        let progress = reopen.progress();
        let deadline = reopen.deadline();
        // Shown when it changes (not on every event: a frame asks for none).
        if self.reopen_shown != Some(progress)
            && let Some((_, app)) = self.windows.iter_mut().find(|(w, _)| *w == asked)
        {
            app.show_reopen_progress(Some(progress));
            self.reopen_shown = Some(progress);
        }
        // Woken when the next step is due (a wait already ending sooner is kept).
        if let (Some(due), Some(waker)) = (deadline, &self.waker)
            && self.reopen_wake.as_ref().is_none_or(|w| w.due() > due || w.due() <= now)
        {
            let waker = Arc::clone(waker);
            self.reopen_wake = kernel::work::Later::run(
                "session-reopen",
                due.saturating_duration_since(now),
                move || waker(),
            );
        }
    }

    /// The reopening ended or stopped: what's open stays; its progress goes.
    fn finish_reopen(&mut self) {
        self.reopen_wake = None;
        self.reopen_shown = None;
        let Some((_, asked)) = self.reopen.take() else { return };
        if let Some((_, app)) = self.windows.iter_mut().find(|(w, _)| *w == asked) {
            app.show_reopen_progress(None);
        }
    }

    /// The session's windows recorded (at most once a second, unless `force`: a window
    /// opened or closed), front first.
    fn record_session(&mut self, force: bool) {
        if !self.sessions.due(force) {
            return;
        }
        let windows = self.windows.iter().map(|(_, app)| app.session_window()).collect();
        let kept = crate::config::settings().get(&crate::settings::SESSIONS_KEPT);
        self.sessions.record(windows, usize::try_from(kept).unwrap_or(1));
    }

    /// What later starts handed over (the jump list, the app started again): opened here.
    fn open_handed(&mut self) {
        let Some((_, first)) = self.windows.first_mut() else { return };
        for paths in crate::instance::take() {
            first.open_handed(paths);
        }
    }

    /// Each window's project noted (a recent one already open is brought to the front);
    /// the app icon's menu follows the recent projects.
    fn sync_projects(&mut self) {
        let open: Vec<(WindowId, std::path::PathBuf)> =
            self.windows.iter().filter_map(|(id, app)| Some((*id, app.project_root()?))).collect();
        if *self.shared.open_projects.borrow() != open {
            *self.shared.open_projects.borrow_mut() = open;
        }
        let names: Vec<(kernel::ProjectId, String)> =
            self.windows.iter().filter_map(|(_, app)| app.project_identity()).collect();
        if *self.shared.project_names.borrow() != names {
            *self.shared.project_names.borrow_mut() = names;
        }
        self.sync_project_windows();
        let generation = crate::recent::generation();
        if self.dock != Some(generation)
            && let Some((_, app)) = self.windows.first()
        {
            app.show_recent_in_dock();
            self.dock = Some(generation);
        }
    }
}

impl AppWindows {
    /// What an earlier run left running, asked about in the window of its project (those
    /// of no project in the first window): only while something is still to ask about.
    fn offer_left_behind(&mut self) {
        if !crate::left_behind::pending() {
            return;
        }
        for (i, (_, app)) in self.windows.iter_mut().enumerate() {
            app.offer_left_behind(i == 0);
        }
    }

    /// The windows' projects as modules see them (their numbers kept while open): told to
    /// every window when they change, each marking its own.
    fn sync_project_windows(&mut self) {
        let shown: Vec<(u64, String, std::path::PathBuf)> = self
            .windows
            .iter()
            .filter_map(|(id, app)| Some((id.0, app.project_identity()?.1, app.project_root()?)))
            .collect();
        let previous: Vec<(u64, u8)> = self
            .shared
            .project_windows
            .borrow()
            .iter()
            .filter_map(|p| Some((p.window, p.number?)))
            .collect();
        // Each project's preferred number (taken when it opens, if free); the ones just
        // chosen (they swap with whoever had them).
        let open: Vec<(u64, Option<u8>)> = shown
            .iter()
            .map(|(w, ..)| {
                let app = self.windows.iter().find(|(id, _)| id.0 == *w).map(|(_, a)| a);
                (*w, app.and_then(EditorApp::preferred_number))
            })
            .collect();
        let asks = std::mem::take(&mut *self.shared.number_asks.borrow_mut());
        let numbers = workbench_model::windows::numbered(&open, &previous, &asks);
        // A choice moved numbers: each moved project keeps its new one as its preference.
        if !asks.is_empty() {
            for (id, app) in &mut self.windows {
                let now = numbers.iter().find(|(w, _)| *w == id.0).map(|(_, n)| *n);
                let before = previous.iter().find(|(w, _)| *w == id.0).map(|(_, n)| *n);
                if let Some(now) = now.filter(|n| Some(*n) != before) {
                    app.save_preferred_number(now);
                }
            }
        }
        let mut all: Vec<workbench_model::ProjectWindow> = shown
            .into_iter()
            .map(|(window, name, path)| workbench_model::ProjectWindow {
                window,
                name,
                path,
                number: numbers.iter().find(|(w, _)| *w == window).map(|(_, n)| *n),
                current: false,
            })
            .collect();
        all.sort_by_key(|p| (p.number.is_none(), p.number));
        // The pictures of windows whose project closed go with it.
        let open: Vec<u64> = all.iter().map(|p| p.window).collect();
        workbench_model::pictures::keep_only(&open);
        if *self.shared.project_windows.borrow() == all {
            return;
        }
        *self.shared.project_windows.borrow_mut() = all.clone();
        for (id, app) in &mut self.windows {
            let own = all
                .iter()
                .cloned()
                .map(|p| workbench_model::ProjectWindow { current: p.window == id.0, ..p })
                .collect();
            app.set_project_windows(own);
        }
    }
}

/// A past session's windows that still have something to open (on a worker: it reads the
/// disk): a project whose folder exists, the files alone that still exist.
fn existing(
    windows: Vec<workspace::sessions::SessionWindow>,
) -> Vec<workspace::sessions::SessionWindow> {
    windows
        .into_iter()
        .filter_map(|mut window| match &window.project {
            Some(project) => project.is_dir().then_some(window),
            None => {
                window.files.retain(|f| f.is_file());
                (!window.files.is_empty()).then_some(window)
            }
        })
        .collect()
}

/// What a past session's window opens with (what still exists, [`existing`]): its project
/// (its own session brings back its tabs), or its files alone. `None`: nothing to open (the
/// project is already open, no files).
fn reopen_paths(
    window: &workspace::sessions::SessionWindow,
    open: &[(WindowId, std::path::PathBuf)],
) -> Option<Vec<std::path::PathBuf>> {
    match &window.project {
        Some(project) => (!open.iter().any(|(_, p)| p == project)).then(|| vec![project.clone()]),
        None => (!window.files.is_empty()).then(|| window.files.clone()),
    }
}

/// The event's name (for the watchdog: what was blocking the loop, if it hangs).
fn event_name(event: &PlatformEvent) -> &'static str {
    match event {
        PlatformEvent::Resized(_) => "resize",
        PlatformEvent::ScaleFactorChanged(_) => "scale",
        PlatformEvent::RedrawRequested => "redraw",
        PlatformEvent::CloseRequested => "close",
        PlatformEvent::Focused(_) => "focus",
        PlatformEvent::Occluded(_) => "occluded",
        PlatformEvent::Key(_) => "key",
        PlatformEvent::Scroll(_) => "scroll",
        PlatformEvent::Pinch(_) => "pinch",
        PlatformEvent::CursorMoved(_) => "cursor",
        PlatformEvent::CursorLeft => "cursor-left",
        PlatformEvent::ModifiersChanged(_) => "modifiers",
        PlatformEvent::MouseDown { .. } => "mouse-down",
        PlatformEvent::MouseUp { .. } => "mouse-up",
        PlatformEvent::Command(_) => "command",
        PlatformEvent::Ime(_) => "ime",
        PlatformEvent::Access(_) => "access",
        PlatformEvent::SystemAppearanceChanged(_) => "appearance",
        PlatformEvent::Wake => "wake",
        PlatformEvent::MenuOpening => "menu-opening",
        PlatformEvent::ScreenColor(_) => "screen-color",
        PlatformEvent::FilesHovered { .. } => "files-hovered",
        PlatformEvent::FilesHoverCancelled => "files-hover-cancelled",
        PlatformEvent::FilesDropped { .. } => "files-dropped",
    }
}

#[cfg(test)]
#[path = "../tests/unit/windows.rs"]
mod tests;
