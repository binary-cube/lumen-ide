//! The windowed app: wires together the platform, the renderer, the commands and the workbench.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use command_model::{ChordKey, CommandId, KeyChord, Keymap};
use document::{Motion, commands as edit};
use kernel::{PhysicalSize, Point};
use platform_api::{
    Control, CursorIcon, ImeEvent, Key, KeyEvent, MenuItem, MouseButton, NamedKey, PlatformEvent,
    ScrollDelta, SystemAppearance, Window,
};
use render_api::{RenderOptions, RenderTarget, Renderer, RendererFactory, Scene};
use render_wgpu::WgpuRendererFactory;
use text_layout::{FontFamily, TextSystem};
use theming::Appearance;
use ui_workbench::{DebugOverlay, SettingsAction, SettingsModel, Splash};

use crate::commands as cmd;
use crate::resources::ResourceMonitor;
use crate::themes::Themes;
use crate::workbench::{EditKey, FoldCommand, MouseResult, Split, Workbench};
use icons::WithIcons;
use syntax_ts::{SyntaxConfig, SyntaxWorker};

mod access;
mod actions;
mod alt_tap;
mod chords;
mod closing;
mod double_shift;
mod ending;
mod events;
mod file_drop;
mod floating;
mod frame;
mod input;
mod leftovers;
mod menu_states;
mod menus;
mod projects;
mod splash;
mod state;
mod system_sign;
mod theme;
mod unseen_motion;
mod web_views;

#[cfg(feature = "snapshot")]
pub(crate) use theme::settings_model;

const MAC: bool = cfg!(target_os = "macos");

/// The window's corner radius (logical pixels), as Windows 11's.
const WINDOW_CORNER_RADIUS: f32 = 8.0;

/// The maximum interval between clicks for a double/triple click.
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// How long a window stays hidden (minimized, covered, on another desktop) before the image
/// on screen goes too (plan 76f): a window hidden for a moment comes back as it was, with
/// nothing to draw first; one hidden longer is drawn again as it shows.
const HIDDEN_AFTER: Duration = Duration::from_secs(30);

/// The text system, available after the loading thread finishes.
enum Fonts {
    Loading(JoinHandle<(TextSystem, FontFamily)>),
    Ready(Box<TextSystem>),
    Failed,
}

/// What follows after the requested tabs are closed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Final {
    Nothing,
    /// The tab that stays active (after "close others" etc.).
    Activate(kernel::DocumentId),
    CloseWindow,
    Quit,
    OpenProject(PathBuf),
    /// The window's project closed (File › Close Project): the start screen.
    CloseProject,
}

/// A tab closing in progress (the Document context's use case): the remaining ones (may be
/// waiting for a dialog answer), then what follows.
type Closing = document::CloseDocuments<Final>;

/// The least time between two frames of an animation that showed nothing new (a display's
/// frame at 120 Hz): what moves unseen costs at most that rate, never an unpaced loop.
const ANIMATION_FLOOR: Duration = Duration::from_micros(8_333);

/// The open tabs, as the closing asks about them.
struct OpenTabs<'a>(&'a Workbench);

impl document::OpenDocuments for OpenTabs<'_> {
    fn is_open(&self, id: kernel::DocumentId) -> bool {
        self.0.tab_index(id).is_some()
    }
    fn is_modified(&self, id: kernel::DocumentId) -> bool {
        self.0.tab_index(id).is_some_and(|i| self.0.tab_dirty(i))
    }
}

/// The question the open dialog is waiting on.
#[derive(Debug)]
enum Ask {
    Save(kernel::DocumentId),
    OpenTarget(PathBuf),
    /// The project is open in another window: go there, or open it again.
    AlreadyOpen(PathBuf, platform_api::WindowId),
    Rename(kernel::DocumentId),
    GotoLine,
    /// A project taken out of the recent list (and, checked, its folder to the trash);
    /// `true`: it's open in a window (its folder stays).
    DeleteProject(PathBuf, bool),
    /// Work still running before a closing (the plain dialog, without the module's modal).
    Running,
    /// A program an earlier run left running (the first of `Pending::left_behind`).
    LeftBehind,
}

/// The resources shared by all windows (a single process): fonts and the text system
/// (with the shaping and glyph caches), themes, the syntax configuration (grammars),
/// render options. The GPU is shared through the renderer, the modules through
/// `LanguageSupport`. Each window has the rest: the project, tabs, indexer, terminals.
pub struct Shared {
    fonts: RefCell<Fonts>,
    family: RefCell<Option<FontFamily>>,
    themes: RefCell<Themes>,
    /// Incremented on every theme / language change; windows sync to them.
    theme_version: Cell<u64>,
    /// The themes' own settings' version last seen (`theme_options::version`).
    options_version: Cell<u64>,
    locale_version: Cell<u64>,
    /// Bumped when the keymap preset changes (every window rebuilds its keymap).
    keymap_version: Cell<u64>,
    /// The settings' changes by another program last taken in (`config::outside_changes`).
    outside_version: Cell<u64>,
    syntax: Arc<SyntaxConfig>,
    render_options: RenderOptions,
    started: Instant,
    /// The projects to open in the requested windows, in request order.
    pub(crate) pending: RefCell<VecDeque<Vec<PathBuf>>>,
    /// Each window's project (a recent project already open is brought to the front).
    pub(crate) open_projects: RefCell<Vec<(platform_api::WindowId, PathBuf)>>,
    /// Every window's project by identity and name (the question before quitting names
    /// them).
    pub(crate) project_names: RefCell<Vec<(kernel::ProjectId, String)>>,
    /// Every window's project as modules see it (its number kept while it's open; not
    /// marked as anyone's own: each window marks its own).
    pub(crate) project_windows: RefCell<Vec<workbench_model::ProjectWindow>>,
    /// Numbers just chosen for a window's project (its window, the number), applied once.
    pub(crate) number_asks: RefCell<Vec<(u64, u8)>>,
    /// The windows open (closing the last one ends the app, and its work with it).
    pub(crate) windows: Cell<usize>,
    /// Counts the windows' focusing: the window that took it last holds the latest (the
    /// frontmost, the only one that starts what modules start when idle).
    focus_turn: Cell<u64>,
    /// The view over every window (a module's `open_overlay_everywhere`): what it is
    /// (`None`: none), and how many times it changed (each window follows it once).
    pub(crate) everywhere: RefCell<(u64, Option<crate::over::Over>)>,
    /// A past session asked to reopen (plan 78): which one, from which window; and its
    /// reopening asked to stop.
    pub(crate) reopen_ask: Cell<Option<(u128, platform_api::WindowId)>>,
    pub(crate) reopen_stop: Cell<bool>,
}

impl Shared {
    pub fn new(
        themes: Themes,
        syntax: Arc<SyntaxConfig>,
        fonts: JoinHandle<(TextSystem, FontFamily)>,
        render_options: RenderOptions,
        started: Instant,
    ) -> Self {
        Self {
            fonts: RefCell::new(Fonts::Loading(fonts)),
            family: RefCell::new(None),
            themes: RefCell::new(themes),
            theme_version: Cell::new(0),
            options_version: Cell::new(0),
            locale_version: Cell::new(0),
            keymap_version: Cell::new(0),
            outside_version: Cell::new(crate::config::outside_changes()),
            syntax,
            render_options,
            started,
            pending: RefCell::new(VecDeque::new()),
            open_projects: RefCell::new(Vec::new()),
            project_names: RefCell::new(Vec::new()),
            project_windows: RefCell::new(Vec::new()),
            number_asks: RefCell::new(Vec::new()),
            windows: Cell::new(0),
            focus_turn: Cell::new(0),
            everywhere: RefCell::default(),
            reopen_ask: Cell::new(None),
            reopen_stop: Cell::new(false),
        }
    }

    /// Waits (once, for all windows) for the fonts to load.
    fn ensure_fonts(&self) {
        let mut fonts = self.fonts.borrow_mut();
        if !matches!(*fonts, Fonts::Loading(_)) {
            return;
        }
        let Fonts::Loading(handle) = std::mem::replace(&mut *fonts, Fonts::Failed) else {
            return;
        };
        let waited = Instant::now();
        let joined = handle.join();
        tracing::info!(waited = ?waited.elapsed(), elapsed = ?crate::startup::elapsed(), "startup phase: fonts taken");
        match joined {
            Ok((text, family)) => {
                *self.family.borrow_mut() = Some(family);
                *fonts = Fonts::Ready(Box::new(text));
            }
            Err(_) => tracing::error!("loading the fonts failed"),
        }
    }

    /// What opens in the next new window (the project, then files), in order.
    pub fn take_pending(&self) -> Vec<PathBuf> {
        self.pending.borrow_mut().pop_front().unwrap_or_default()
    }
}

pub struct EditorApp {
    window: Option<Arc<dyn Window>>,
    /// The window as shown: hidden, focused, sampling the screen, its title.
    window_state: state::WindowState,
    /// The mouse.
    pointer: state::Pointer,
    /// The renderer, the scene and what the frames measure (the renderer is let go before
    /// the workbench, as the fields' order says).
    frames: state::Frames,
    shared: Rc<Shared>,
    /// The shared state's versions applied here.
    applied: state::Applied,
    workbench: Workbench,
    /// What waits on an answer (a closing, a question, the platform's exit).
    pending: state::Pending,
    /// Detects a double Shift (opens the project search).
    double_shift: double_shift::DoubleShift,
    /// Alt alone opens the menu drawer (Windows, Linux).
    alt_tap: alt_tap::AltTap,
    keymap: Keymap,
    resources: Option<ResourceMonitor>,
    /// The splash and the first frame.
    startup: state::Startup,
    /// The pages the system's web engine shows over the views (a response's preview).
    web_views: web_views::WebViews,
}

impl EditorApp {
    pub fn new(workbench: Workbench, shared: Rc<Shared>) -> anyhow::Result<Self> {
        // A new window opens in front.
        let turn = shared.focus_turn.get() + 1;
        shared.focus_turn.set(turn);
        Ok(Self {
            window: None,
            window_state: state::WindowState {
                focused: true,
                focus_turn: turn,
                pictured: None,
                ..Default::default()
            },
            applied: state::Applied {
                theme_version: shared.theme_version.get(),
                locale_version: shared.locale_version.get(),
                keymap_version: shared.keymap_version.get(),
                font_applied: false,
                // A window opened while a view is over every window: it gets it too.
                everywhere_turn: 0,
            },
            shared,
            workbench,
            pending: state::Pending::default(),
            double_shift: Default::default(),
            alt_tap: Default::default(),
            keymap: crate::keymaps::keymap(MAC)?,
            resources: None,
            frames: state::Frames::new(),
            startup: state::Startup::new(),
            web_views: web_views::WebViews::default(),
            pointer: state::Pointer::default(),
        })
    }

    /// The text system; on the first call, waits for the font loading thread.
    fn ensure_text(&mut self) {
        self.shared.ensure_fonts();
        if !self.applied.font_applied
            && let Some(family) = self.shared.family.borrow().clone()
        {
            self.workbench.set_font_family(family);
            self.applied.font_applied = true;
        }
    }

    /// Runs `f` with the workbench and the text system. `false` if the fonts failed.
    fn with_text<R>(&mut self, f: impl FnOnce(&mut Workbench, &mut TextSystem) -> R) -> Option<R> {
        self.ensure_text();
        let mut fonts = self.shared.fonts.borrow_mut();
        match &mut *fonts {
            Fonts::Ready(text) => Some(f(&mut self.workbench, text)),
            _ => None,
        }
    }

    /// "Clear and Rebuild Caches": the window's (workbench), the modules' data, the shaped
    /// text and the glyphs (shared by windows) and, in the same step, the GPU atlas and
    /// textures — glyph keys are reused after clearing, so the two can't be left out of sync.
    pub(super) fn clear_caches(&mut self) {
        use kernel::Purge;
        self.workbench.purge();
        self.workbench.support().manager.purge();
        if let Fonts::Ready(text) = &mut *self.shared.fonts.borrow_mut() {
            text.purge();
        }
        if let Some(renderer) = &mut self.frames.renderer {
            renderer.purge_caches();
        }
        crate::resources::release_splash_art();
        self.relayout();
    }

    /// Requests a new frame and records the input moment for latency measurement.
    fn redraw(&mut self) {
        self.frames.dirty = true;
        self.frames.pending_input.get_or_insert_with(Instant::now);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.update_title();
    }

    fn update_title(&mut self) {
        let title = self.workbench.title();
        if title != self.window_state.title {
            if let Some(window) = &self.window {
                window.set_title(&title);
            }
            self.window_state.title = title;
        }
    }

    fn relayout(&mut self) {
        if let Some(window) = self.window.clone() {
            self.workbench.resize(window.inner_size(), window.scale_factor() as f32);
            self.redraw();
        }
    }
}

fn theme_appearance(appearance: SystemAppearance) -> Appearance {
    match appearance {
        SystemAppearance::Light => Appearance::Light,
        SystemAppearance::Dark => Appearance::Dark,
    }
}

/// What was being done, when it fails: logged, and told to the user.
#[derive(Debug, Clone, Copy)]
enum Doing<'a> {
    Saving(&'a str),
    Opening(&'a Path),
}

impl EditorApp {
    /// An operation's outcome: on failure, logged and told to the user (in the window's
    /// message line). `true` if it succeeded.
    fn succeeded(&mut self, result: std::io::Result<()>, doing: Doing<'_>) -> bool {
        let Err(e) = result else { return true };
        let reason = e.to_string();
        let message = match doing {
            Doing::Saving(name) => {
                tracing::error!(name, error = %e, "saving failed");
                i18n::tr!("files.save_failed", name = name.to_owned(), reason = reason)
            }
            Doing::Opening(path) => {
                tracing::error!(path = %path.display(), error = %e, "opening failed");
                let name = path.file_name().unwrap_or(path.as_os_str());
                let name = name.to_string_lossy().into_owned();
                i18n::tr!("files.open_failed", name = name, reason = reason)
            }
        };
        workbench_model::WorkbenchHost::notify(
            &mut self.workbench,
            workbench_model::Notification::error(message),
        );
        false
    }
}

impl EditorApp {
    /// The window was created: the renderer (on the shared GPU), the theme, its threads.
    pub fn window_opened(&mut self, window: Arc<dyn Window>) {
        ui_workbench::ColorPicker::enable_eyedropper(window.can_sample_screen_color());
        tracing::info!(elapsed = ?self.shared.started.elapsed(), "window created");
        let size = window.inner_size();
        let target: Arc<dyn RenderTarget> = Arc::new(window.handles());
        let factories: [&dyn RendererFactory; 1] = [&WgpuRendererFactory];
        for factory in factories {
            match factory.create(Arc::clone(&target), size, &self.shared.render_options) {
                Ok(renderer) => {
                    let description = renderer.description();
                    tracing::info!(renderer = %description, elapsed = ?self.shared.started.elapsed(), "rendering driver active");
                    self.workbench.set_renderer(description.clone());
                    self.frames.overlay = DebugOverlay::new(description);
                    self.workbench.set_backdrop_blur(renderer.backdrop_blur());
                    self.frames.renderer = Some(renderer);
                    break;
                }
                Err(e) => {
                    tracing::warn!(driver = factory.name(), error = %e, "the driver didn't start")
                }
            }
        }
        if self.frames.renderer.is_none() {
            tracing::error!("no rendering driver could start");
        }
        if let Some(appearance) = window.system_appearance()
            && self.shared.themes.borrow_mut().follow_appearance(theme_appearance(appearance))
        {
            self.shared.theme_version.set(self.shared.theme_version.get() + 1);
        }
        self.apply_theme();
        crate::startup::mark("theme applied");
        self.workbench.set_waker(window.waker());
        // Weak: the workbench doesn't keep a closed window alive.
        let clipboard = Arc::downgrade(&window);
        self.workbench.set_clipboard_reader(Rc::new(move || clipboard.upgrade()?.clipboard_text()));
        let shared = Rc::downgrade(&self.shared);
        self.workbench.set_theme_source(Rc::new(move |project_theme| {
            shared
                .upgrade()
                .map(|shared| theme::studio_input(&shared, project_theme))
                .unwrap_or_else(|| theming::studio::StudioInput {
                    themes: Vec::new(),
                    current: String::new(),
                })
        }));
        let (read, write) = (Arc::downgrade(&window), Arc::downgrade(&window));
        self.workbench.set_files_clipboard(crate::workbench::FilesClipboard {
            read: Rc::new(move || read.upgrade().map(|w| w.clipboard_files()).unwrap_or_default()),
            write: Rc::new(move |paths: &[PathBuf]| {
                if let Some(window) = write.upgrade() {
                    window.set_clipboard_files(paths);
                }
            }),
        });
        self.workbench.set_titlebar_inset(window.titlebar_inset());
        match SyntaxWorker::spawn(Arc::clone(&self.shared.syntax), window.waker()) {
            Ok(worker) => self.workbench.set_worker(worker),
            Err(e) => tracing::error!(error = %e, "the parsing thread didn't start"),
        }
        if std::env::var_os("EDITOR_NO_RESOURCES").is_none() {
            match ResourceMonitor::shared(window.app_waker()) {
                Ok(monitor) => self.resources = Some(monitor),
                Err(e) => tracing::warn!(error = %e, "the resource monitor didn't start"),
            }
        }
        self.window = Some(window);
        self.relayout();
        // Startup isn't user input: we don't measure it as latency.
        self.frames.pending_input = None;
        crate::startup::mark("window ready");
    }

    pub fn event(&mut self, event: PlatformEvent) -> Control {
        // Anything but a frame asked for: the next frame is whole.
        if !matches!(event, PlatformEvent::RedrawRequested) {
            self.frames.dirty = true;
        }
        // Input: the theme's decorations move again (they stop after a while without).
        if matches!(
            event,
            PlatformEvent::Key(_)
                | PlatformEvent::MouseDown { .. }
                | PlatformEvent::MouseUp { .. }
                | PlatformEvent::CursorMoved(_)
                | PlatformEvent::Scroll(_)
                | PlatformEvent::Pinch(_)
                | PlatformEvent::Ime(_)
        ) {
            let resumed = self.workbench.decorations_deadline(Instant::now()).is_none();
            self.workbench.decorations_input(Instant::now());
            // What starts after a while without input waits again.
            self.workbench.idle_input(Instant::now());
            tracing::trace!(?event, "input for the decorations");
            // Stopped while idle: they move on from here.
            if resumed
                && let Some(at) = self.workbench.decorations_deadline(Instant::now())
                && let Some(window) = &self.window
            {
                window.request_redraw_at(at);
            }
        }
        // A key: the region emphasized follows the keyboard again (Focus mode).
        if matches!(
            event,
            PlatformEvent::Key(KeyEvent { pressed: true, .. }) | PlatformEvent::Ime(_)
        ) {
            self.workbench.emphasis_key();
        }
        match &event {
            PlatformEvent::Focused(focused) => {
                self.window_state.focused = *focused;
                // Focused last: the frontmost window now.
                if *focused {
                    let turn = self.shared.focus_turn.get() + 1;
                    self.shared.focus_turn.set(turn);
                    self.window_state.focus_turn = turn;
                }
                // The menu bar is the focused window's: its rows as this one has them.
                self.window_state.menu_context = None;
                self.window_state.recent_menu = None;
                // Alt held while switching windows (Alt + Tab) isn't the menu's key.
                self.alt_tap.focus_changed();
                // Back from elsewhere: `settings.toml` may have been edited by hand (read
                // again in the background; the UI answers from the cache meanwhile).
                if *focused && let Some(window) = &self.window {
                    crate::config::reload_settings(window.app_waker());
                }
                // The caret stops blinking in a background window: it stays shown (not
                // frozen in its "off" half), and blinks again from "on" when focus returns.
                ui_kit::caret::reset();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                // Leaving the window saves what changed (automatic saving).
                if !*focused
                    && self.workbench.autosave(true)
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            // Input: the caret shows solid and its blink restarts.
            PlatformEvent::Key(KeyEvent { pressed: true, .. })
            | PlatformEvent::MouseDown { .. }
            | PlatformEvent::Ime(_) => ui_kit::caret::reset(),
            // The user's back: a caret at rest blinks again.
            PlatformEvent::CursorMoved(_) | PlatformEvent::Scroll(_) => {
                if ui_kit::caret::wake()
                    && let Some(window) = &self.window
                {
                    window.request_redraw();
                }
            }
            _ => {}
        }
        if self.window_state.sampling
            && matches!(
                event,
                PlatformEvent::CursorMoved(_)
                    | PlatformEvent::Scroll(_)
                    | PlatformEvent::ModifiersChanged(_)
                    | PlatformEvent::RedrawRequested
                    | PlatformEvent::Pinch(_)
            )
        {
            return Control::Continue;
        }
        let control = self.event_inner(event);
        // An edit refused in a locked tab: a word to the user.
        if self.workbench.report_refused_edits() {
            self.redraw();
        }
        // Whatever the workbench asked for while handling the event (a click on the
        // eyedropper, Enter on a list…) runs now, not at the next event.
        if let Some(request) = self.workbench.take_request() {
            self.mouse_result(request);
        }
        // A dialog's answer continues the action that requested it (may close the window).
        if let Some(answer) = self.workbench.take_dialog_answer() {
            self.on_dialog(answer);
            self.redraw();
        }
        // A module asked about its work still running before restarting it (a reconnect).
        if let Some(request) = self.workbench.take_running_request() {
            self.ask_running_request(request);
            self.redraw();
        }
        // A closing waiting on work still running: its question closed without an answer,
        // or the work done.
        self.follow_running(Instant::now());
        // The app ending: what it started stopped, then the window closes (or the app quits).
        self.follow_ending(Instant::now());
        self.sync_recent_menu();
        self.sync_menu_states();
        self.sync_system_sign();
        match self.pending.exit_request.take() {
            Some(request) => request,
            None => control,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/mod.rs"]
mod tests;
