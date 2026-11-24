//! The open projects across the windows, and the app icon's menu (the Dock's on macOS):
//! the recent projects; one already open brings its window to the front, another opens in a
//! new window.

use super::*;

/// The icon menu's commands (File › Open Recent's too): this prefix and the project's path.
pub(crate) const OPEN_RECENT: &str = cmd::OPEN_RECENT_PROJECT;

/// Recent projects shown in the icon's menu at most.
const DOCK_RECENT: usize = 12;

/// The status bar's progress while a past session reopens.
const REOPEN_PROGRESS: &str = "session-reopen";

/// A window's picture's width at most, in pixels (a card of the open projects' list, on a
/// screen of twice the density): about 500 KB each.
const PICTURE_WIDTH: u32 = 480;

/// How long a window's picture is good for when it loses the focus: reading a frame back
/// takes a buffer as large as the window, not worth at every switch.
pub(super) const PICTURE_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

impl EditorApp {
    /// "Delete…" on a recent project: taken out of the list and, if its checkbox is checked
    /// (unchecked on open: a danger), its folder moved to the system's trash. A project open
    /// in a window keeps its folder (no checkbox).
    pub(super) fn ask_delete_project(&mut self) {
        let Some(path) = self.workbench.recent_menu_project() else { return };
        let open = self.shared.open_projects.borrow().iter().any(|(_, p)| *p == path);
        self.workbench.open_dialog(crate::workbench::delete_project_dialog(&path, open));
        self.pending.asking = Some(Ask::DeleteProject(path, open));
    }

    /// Takes `path` out of the recent list; `to_trash`: its folder goes to the system's
    /// trash too (restorable from there), off the UI thread (a large folder takes a while
    /// on Windows). A project open in a window by now keeps its folder.
    pub(super) fn delete_project(&mut self, path: &Path, to_trash: bool) {
        crate::recent::update(|list| list.forget(path));
        let open = self.shared.open_projects.borrow().iter().any(|(_, p)| p == path);
        if !to_trash || open || !path.is_dir() {
            return;
        }
        let paths = vec![path.to_path_buf()];
        let spawned = std::thread::Builder::new().name("trash-project".into()).spawn(move || {
            match fs_local::move_to_trash(&paths) {
                Ok(()) => tracing::info!(path = %paths[0].display(), "project folder moved to the trash"),
                Err(e) => tracing::warn!(path = %paths[0].display(), error = %e, "cannot move the project folder to the trash"),
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "cannot start moving the project folder to the trash");
        }
    }

    /// The windows' projects (this one's marked): the modules' list, the top bar's number.
    pub fn set_project_windows(&mut self, windows: Vec<workbench_model::ProjectWindow>) {
        self.workbench.set_project_windows(windows);
        self.redraw();
    }

    /// The number this project prefers among the open ones (its own settings).
    pub fn preferred_number(&self) -> Option<u8> {
        self.workbench.preferred_number()
    }

    /// Keeps `number` as this project's preferred one.
    pub fn save_preferred_number(&mut self, number: u8) {
        self.workbench.save_preferred_number(number);
    }

    /// A picture of this window's last frame (what the open projects' list shows of it),
    /// read and made small in the background, then kept with the others'. Only a window
    /// with a project: the list shows no other. `asked`: a module wants it now (the list
    /// opening); else (the focus lost) only when the last one is older than
    /// [`PICTURE_EVERY`].
    pub fn capture_window(&mut self, asked: bool) {
        let recent = self.window_state.pictured.is_some_and(|at| at.elapsed() < PICTURE_EVERY);
        if recent && !asked {
            return;
        }
        let Some(window) = self.workbench.this_window() else { return };
        let Some(renderer) = &mut self.frames.renderer else { return };
        let started = renderer.capture(
            PICTURE_WIDTH,
            Box::new(move |c| workbench_model::pictures::keep(window, c.width, c.height, c.rgba)),
        );
        if started {
            self.window_state.pictured = Some(Instant::now());
        }
        // A quiet window keeps no frame: what's on screen drawn again for it.
        if renderer.capture_waits_for_frame()
            && let Fonts::Ready(text) = &mut *Rc::clone(&self.shared).fonts.borrow_mut()
        {
            let mut glyphs = WithIcons { text: &mut **text, icons: self.workbench.icons() };
            if let Err(e) = renderer.render(&self.frames.scene, &mut glyphs) {
                tracing::debug!(error = %e, "the window's picture wasn't drawn");
            }
        }
    }

    pub fn project_root(&self) -> Option<PathBuf> {
        self.workbench.project_root()
    }

    /// What the app's session keeps of this window: its project, its open files.
    /// Nothing open here (no project, no tab): a session reopened starts in it.
    pub fn is_blank(&self) -> bool {
        self.workbench.project_root().is_none() && self.workbench.tab_count() == 0
    }

    /// The window drew its first frame.
    pub fn first_frame_drawn(&self) -> bool {
        self.startup.first_frame_logged
    }

    /// The status bar's line while a session reopens (`done` of `total`, with "Stop"), or
    /// none.
    pub fn show_reopen_progress(&mut self, progress: Option<(usize, usize)>) {
        let progress = progress.map(|(done, total)| workbench_model::Progress {
            label: i18n::tr!("home.session_reopening", done = done as i64, total = total as i64),
            icon: "rotate-ccw",
            stop: Some(crate::commands::STOP_REOPEN),
        });
        workbench_model::WorkbenchHost::set_progress(
            &mut self.workbench,
            REOPEN_PROGRESS,
            progress,
        );
        self.redraw();
    }

    pub fn session_window(&self) -> workspace::sessions::SessionWindow {
        workspace::sessions::SessionWindow {
            project: self.workbench.project_root(),
            files: self.workbench.open_file_paths(),
        }
    }

    /// The window's project: its identity and its name.
    pub fn project_identity(&self) -> Option<(kernel::ProjectId, String)> {
        self.workbench.project_identity()
    }

    /// The app icon's menu (the Dock's, the taskbar's jump list): the recent projects (only
    /// existing folders).
    pub fn show_recent_in_dock(&self) {
        let Some(window) = &self.window else { return };
        let projects = crate::recent::projects()
            .into_iter()
            .take(DOCK_RECENT)
            .map(|p| platform_api::IconProject {
                command: format!("{OPEN_RECENT}{}", p.path.display()),
                label: p.name,
                path: p.path,
            })
            .collect();
        window.set_icon_menu(platform_api::IconMenu {
            title: i18n::t("menu.recent_projects"),
            projects,
        });
    }

    /// Paths handed by a later start: a project already open brings its window to the
    /// front; others open in a new window; nothing: the app to the front.
    pub fn open_handed(&mut self, handed: Vec<crate::instance::Handed>) {
        match handed.as_slice() {
            [] => self.bring_to_front(),
            [one] if one.is_dir => self.open_recent(&one.real),
            _ => self.open_window_with(handed.into_iter().map(|h| h.path).collect()),
        }
    }

    /// Files handed by a later start (or the system's "Open With"), opened here as tabs, the
    /// last one's chosen (one already open: its tab).
    pub fn open_handed_files(&mut self, files: &[PathBuf]) {
        for path in files {
            let opened = self.workbench.open_path(path);
            self.succeeded(opened, Doing::Opening(path));
        }
        self.relayout();
    }

    /// This window in front, the app's focus in it (out of the Dock if minimized).
    pub fn bring_to_front(&self) {
        if let Some(window) = &self.window {
            window.focus_window(window.id());
        }
    }

    /// When this window last took the focus, among the windows (the frontmost: the highest).
    pub fn focus_turn(&self) -> u64 {
        self.window_state.focus_turn
    }

    /// A recent project chosen from the icon's menu: its window to the front if it's open,
    /// else a new window with it.
    pub(super) fn open_recent(&mut self, path: &Path) {
        let Some(window) = self.window.clone() else { return };
        let open =
            self.shared.open_projects.borrow().iter().find(|(_, p)| p == path).map(|(w, _)| *w);
        match open {
            Some(id) => window.focus_window(id),
            None => self.open_window(Some(path)),
        }
    }
}
