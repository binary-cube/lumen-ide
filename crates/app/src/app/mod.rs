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

mod actions;
mod alt_tap;
mod closing;
mod double_shift;
mod ending;
mod events;
mod file_drop;
mod input;
mod leftovers;
mod menu_states;
mod menus;
mod projects;
mod splash;
mod state;
mod system_sign;
mod theme;
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

    fn render(&mut self) {
        let Some(window) = self.window.clone() else { return };
        // Typing paused long enough: the changed files are saved.
        self.workbench.autosave(false);
        // The view that took the whole screen closed, however it did.
        if self.window_state.full_screen_for_view && !self.workbench.modal_open() {
            self.window_state.full_screen_for_view = false;
            window.toggle_fullscreen();
        }
        if self.window_state.occluded {
            return;
        }
        // A module asked for this window's picture: taken from the frame on screen, before
        // this one (a view just opened over it) is drawn.
        if self.workbench.take_capture_wanted() {
            self.capture_window(true);
        }
        let frame_start = Instant::now();
        // A view's own deadline came (a save due a moment after typing): its work is done
        // now, by its poll, not only at the next wake — or the frames it asked for would
        // follow one another until something else woke the loop.
        if self.workbench.views_deadline().is_some_and(|due| due <= frame_start) {
            self.workbench.poll_viewers();
        }
        // What the app pages told their views, before those paint.
        let told = self.web_views.take_events();
        if !told.is_empty() {
            for (page, event) in told {
                self.workbench.web_event(page, event);
            }
            // What they asked for (a tab closed, a link, a Save dialog), now: not at the next
            // unrelated wake.
            self.with_text(|wb, text| wb.run_panel_actions(text));
            self.frames.dirty = true;
        }
        self.workbench.paint_phases.begin();
        // A while without input: what modules start then (Relax mode), drawn in this frame.
        let frontmost = self.window_state.focus_turn == self.shared.focus_turn.get();
        // One due: asked of the system first, the user may be busy in another program.
        if self.workbench.idle_deadline().is_some_and(|due| due <= frame_start)
            && let Some(at) = window.system_idle().and_then(|idle| frame_start.checked_sub(idle))
        {
            self.workbench.active_elsewhere(at);
        }
        if self.workbench.run_idle_actions(frame_start, frontmost) {
            self.frames.dirty = true;
        }
        let viewport = window.inner_size().to_size();
        let scale = window.scale_factor() as f32;
        self.sync_decorations_window(&window, frame_start);
        if self.release_without_frame(frame_start) {
            return;
        }
        // Nothing happened since the last whole frame and nothing else is due: only the
        // theme's decorations are drawn again, over the scene kept.
        let only_decorations = !self.frames.dirty
            && self.frames.full_due.is_some_and(|due| frame_start < due)
            && self.startup.splash.is_gone()
            && self.startup.first_frame_logged
            && self.workbench.decorations_drawn();

        self.ensure_text();
        self.workbench.paint_phases.mark("text_system");
        // A page on screen: what floats is drawn on the layer over it.
        if !only_decorations {
            self.sync_floating(&window);
        }
        let shared = Rc::clone(&self.shared);
        let mut fonts = shared.fonts.borrow_mut();
        let (Fonts::Ready(text), Some(renderer)) = (&mut *fonts, &mut self.frames.renderer) else {
            return;
        };
        let only_decorations =
            only_decorations && self.workbench.repaint_decorations(&mut self.frames.scene);
        // The earliest moment the next whole frame is needed, whatever happens meanwhile.
        let looks_ahead = frame_start + Duration::from_secs(1);
        let mut full_due = looks_ahead;
        if !only_decorations {
            self.frames.whole_frames += 1;
            // The glyph keys only grow (sizes, zoom, scripts seen): past a bound they're
            // reset, with the atlas that refers to them, in the same step.
            const MAX_GLYPH_KEYS: usize = 200_000;
            if text.glyph_key_count() > MAX_GLYPH_KEYS {
                use kernel::Purge;
                tracing::debug!("too many glyph keys; clearing the shaped text and the atlas");
                text.purge();
                renderer.purge_caches();
            }
            self.frames.scene.reset(self.workbench.background());
            // The window's corners (Windows' radius): rounded unless it fills the screen;
            // cut where the surface is transparent (the splash's frames too).
            if window.window_controls() && !window.is_fullscreen() && !window.is_maximized() {
                self.frames.scene.window_corners = (WINDOW_CORNER_RADIUS * scale).round();
            }
            // The splash, only while the workbench isn't ready: alone, then faded over it.
            // (The active tab is sent to parsing even while nothing of the workbench is drawn.)
            let ready = self.startup.splash.is_gone() || {
                self.workbench.sync_syntax();
                self.workbench.poll_syntax();
                self.workbench.first_paint_ready()
            };
            let show = self.startup.splash_at(frame_start, ready);
            let workbench = match show {
                splash::SplashShow::Workbench => true,
                splash::SplashShow::Splash { opacity, .. } => opacity < 1.0,
                splash::SplashShow::Background => false,
            };
            self.workbench.paint_phases.mark("before_workbench");
            if workbench {
                // The window's own buttons in the top bar (Windows), as it is now.
                self.workbench
                    .set_window_controls(window.window_controls().then(|| window.is_maximized()));
                let full_screen = window.is_fullscreen();
                self.workbench.set_full_screen(full_screen);
                self.workbench.set_maximize_hovered(window.maximize_hovered());
                self.workbench.set_page_holes(window.web_view_holes());
                let floating = self.frames.floating.as_mut().map(|f| {
                    f.scene.reset(kernel::Rgba::TRANSPARENT);
                    &mut f.scene
                });
                self.workbench.paint_with_floating(text, &mut self.frames.scene, floating);
                // The pages over the views, where the views just drew themselves; a page
                // something opened over, its picture in its place from the next frame.
                let pictured = self.web_views.sync(
                    self.workbench.web_pages(),
                    self.workbench.overlay_open(),
                    frame_start,
                    |source, area, inbox| {
                        let source = match source {
                            ui_workbench::WebSource::Html(html) => {
                                platform_api::WebSource::Html(html)
                            }
                            ui_workbench::WebSource::App { root, entry } => {
                                platform_api::WebSource::App { root, entry }
                            }
                        };
                        window.web_view(source, area, &web_views::data_dir(), inbox)
                    },
                );
                if pictured {
                    self.workbench.set_page_pictures(self.web_views.pictures());
                    window.request_redraw();
                }
                // What floats over the pages: the mouse there the app's; drawn over them on
                // the floating layer, else cut out of them (shows through).
                let holes = self.workbench.page_holes();
                self.web_views.set_holes(&holes, self.frames.floating.is_none());
                // Their corners as the cards they're in.
                let workbench = &self.workbench;
                self.web_views.set_corners(|area| workbench.page_corners(area));
                if let Some(floating) = &self.frames.floating {
                    // Something open over the views (a menu, a dialog; the pages hidden
                    // under it): all of the layer; else only where something floats.
                    let shapes = if self.workbench.overlay_open() {
                        vec![(kernel::Rect::from_size(viewport), 0.0)]
                    } else {
                        holes
                    };
                    floating.layer.set_shapes(&shapes, (FLOATING_SHADOW * scale).ceil());
                    floating.layer.raise();
                }
                // What the views have for their app pages, once those are placed.
                self.web_views.send(self.workbench.take_web_sends());
                // Its maximize button is the system's (Windows' snap layouts on hover); full
                // screen, it's ours (it leaves full screen).
                if window.window_controls() {
                    let button = self.workbench.maximize_button().filter(|_| !full_screen);
                    window.set_maximize_button(button);
                }
                // Transitions (hover on separators) request frames only while they last.
                if self.workbench.animating() {
                    window.request_redraw();
                    full_due = frame_start;
                } else if self.workbench.loading() {
                    // A loader turning, a progress bar sweeping: about 30 frames a second
                    // (smooth to the eye; only while work goes on).
                    const LOADER_FRAME: std::time::Duration = std::time::Duration::from_millis(33);
                    let at = std::time::Instant::now() + LOADER_FRAME;
                    window.request_redraw_at(at);
                    full_due = full_due.min(at);
                }
            }
            if let splash::SplashShow::Splash { elapsed, opacity } = show {
                let version = concat!("v", env!("CARGO_PKG_VERSION"));
                let frame = ui_workbench::SplashFrame {
                    bounds: kernel::Rect::from_size(viewport),
                    scale,
                    phase: elapsed.as_secs_f32(),
                    opacity,
                };
                let painter = ui_workbench::Painter {
                    text,
                    scene: &mut self.frames.scene,
                    palette: self.workbench.palette(),
                    icons: self.workbench.icons(),
                };
                let art = crate::resources::splash_art();
                Splash.paint(frame, version, &i18n::t("splash.preparing"), art.as_ref(), painter);
                // The splash animation requests frames until it's gone.
                window.request_redraw();
                full_due = frame_start;
            } else if show == splash::SplashShow::Background {
                // Waiting to be ready (or for the splash's turn): the next frame looks again.
                window.request_redraw();
                full_due = frame_start;
            }
            let area = self.workbench.overlay_area();
            let (palette, icons) = (self.workbench.palette(), self.workbench.icons());
            let scene = &mut self.frames.scene;
            let painter = ui_workbench::Painter { text, scene, palette, icons };
            self.frames.overlay.paint(painter, area, viewport, scale);
        } else {
            self.frames.decoration_frames += 1;
        }

        let mut glyphs = WithIcons { text: &mut **text, icons: self.workbench.icons() };
        let painted = frame_start.elapsed();
        let result = renderer.render(&self.frames.scene, &mut glyphs);
        // What floats, on its layer over the pages (shown once it has a frame).
        if !only_decorations && let Some(floating) = &mut self.frames.floating {
            match floating.renderer.render(&floating.scene, &mut glyphs) {
                Ok(stats) if stats.retry => window.request_redraw(),
                Ok(_) => floating.layer.set_visible(true),
                Err(e) => tracing::warn!(error = %e, "the floating layer's frame"),
            }
        }
        // A long frame says which half took the time (the scene, the GPU).
        if frame_start.elapsed() >= Duration::from_millis(500) {
            let phases = self.workbench.paint_phases.text();
            tracing::info!(?painted, total = ?frame_start.elapsed(), phases, "a slow frame");
        }
        // A window left standing gives its kept frame back: one redraw at that moment.
        if let Some(at) = renderer.kept_frame_release_at()
            && self.frames.kept_frame_check.is_none_or(|check| check <= frame_start)
        {
            self.frames.kept_frame_check = Some(at);
            window.request_redraw_at(at);
        }
        if let (Some(test), Ok(stats)) = (&mut self.frames.resize_test, &result) {
            test.record(frame_start.elapsed(), stats.skipped);
        }
        match result {
            Ok(stats) => {
                self.frames.overlay.record(frame_start.elapsed(), stats);
                if stats.retry {
                    window.request_redraw();
                    full_due = frame_start;
                } else if !stats.skipped
                    && let Some(input) = self.frames.pending_input.take()
                {
                    self.frames.overlay.record_latency(input.elapsed());
                }
                if !self.startup.ready_logged
                    && !stats.skipped
                    && self.workbench.first_paint_ready()
                {
                    self.startup.ready_logged = true;
                    crate::startup::mark("editor ready");
                }
                if !self.startup.first_frame_logged && !stats.skipped {
                    self.startup.first_frame_logged = true;
                    tracing::info!(elapsed = ?self.shared.started.elapsed(), "first frame shown");
                    // Diagnostics: `EDITOR_DEBUG_EYEDROPPER=1` starts the screen eyedropper.
                    if std::env::var_os("EDITOR_DEBUG_EYEDROPPER").is_some() {
                        let started = window.sample_screen_color();
                        self.window_state.sampling = started;
                        tracing::debug!(started, "eyedropper (diagnostic)");
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "render error"),
        }
        // Now and then, how the decorations' frames went (`RUST_LOG=debug`).
        if only_decorations && self.frames.decoration_frames.is_multiple_of(75) {
            tracing::debug!(
                whole = self.frames.whole_frames,
                decorations_only = self.frames.decoration_frames,
                took = ?frame_start.elapsed(),
                "frames"
            );
        }
        // The caret's place for the system's input methods: told when it moves, not each
        // frame (a frame per hover would bring back their popups, macOS's "US").
        let area = self.workbench.caret_rect(text);
        if let Some(area) = area
            && self.window_state.ime_area != Some(area)
        {
            window.set_ime_cursor_area(area);
        }
        self.window_state.ime_area = area;
        // A caret on screen blinks: one redraw when it next turns on / off (only in the
        // focused window; background windows don't wake up for it).
        let mut due = |at: Instant| {
            window.request_redraw_at(at);
            full_due = full_due.min(at);
        };
        if let Some(at) = ui_kit::caret::take_next_frame()
            && self.window_state.focused
        {
            due(at);
        }
        // The code hover card / a tooltip appears after the mouse rests: one redraw then.
        if let Some(at) = self.workbench.hover_deadline() {
            due(at);
        }
        if let Some(at) = self.workbench.autosave_deadline() {
            due(at);
        }
        // A language server's answer waited for: the built-in engine's when its grace ends.
        if let Some(at) = self.workbench.served_deadline() {
            due(at);
        }
        // A notification's ring or spinner: a frame when it next changes (no faster than
        // about 30 a second), none while nothing counts down.
        if let Some(at) = self.workbench.toast_deadline() {
            due(at);
        }
        // The status bar's meter gliding to new heights.
        if let Some(at) = self.workbench.status_deadline() {
            due(at);
        }
        // A view over the window animating at its own pace (a full-window scene).
        if let Some(at) = self.workbench.modal_view_deadline() {
            due(at);
        }
        if let Some(at) = self.workbench.idle_deadline() {
            due(at);
        }
        // The views at their pace (a scrollbar fading in or out).
        if let Some(at) = self.workbench.views_deadline() {
            due(at);
        }
        // A hidden page let go when its time's up (its engine holds hundreds of MB).
        if let Some(at) = self.web_views.deadline() {
            due(at);
        }
        // The windows over the editor at theirs (a popover's scrollbar fading).
        if let Some(at) = self.workbench.overlays_deadline() {
            due(at);
        }
        // A settings card playing its picture (Relax mode's scenes), at its pace.
        if let Some(at) = self.workbench.settings_card_deadline() {
            due(at);
        }
        // The overlay shows what the frames drawn anyway measured (never a frame to measure
        // itself: idle stays idle); only its ×'s tooltip asks for one.
        if let Some(at) = self.frames.overlay.frame_deadline() {
            due(at);
        }
        if only_decorations {
            // The deadlines the last whole frame set still hold.
            full_due = full_due.min(self.frames.full_due.unwrap_or(full_due));
        } else {
            self.frames.dirty = false;
        }
        self.frames.full_due = Some(full_due);
        self.frames.other_due = (full_due < looks_ahead).then_some(full_due);
        // The theme's decorations moving: their next frame (a low rate; none while the
        // window is hidden, unfocused or idle).
        if let Some(at) = self.workbench.decorations_deadline(Instant::now()) {
            window.request_redraw_at(at);
        }
    }

    /// The frame asked for only so that the kept frame and the surface's spare image go back
    /// (nothing happened since, nothing else is due, nothing moves): they go without drawing
    /// one, and what's on screen stays. Drawing it would show a frame anew, which starts the
    /// wait for the spare image again — a whole frame every two seconds in a window standing
    /// still, for ever, and two images of it held. `true`: released, no frame.
    fn release_without_frame(&mut self, now: Instant) -> bool {
        let only_release = !self.frames.dirty
            && self.frames.kept_frame_check.is_some_and(|at| at <= now)
            && self.frames.other_due.is_none_or(|at| now < at)
            && self.startup.splash.is_gone()
            && self.startup.first_frame_logged
            && !self.workbench.animating()
            && !self.workbench.loading()
            && self.workbench.decorations_deadline(now).is_none();
        let Some(renderer) = self.frames.renderer.as_mut().filter(|_| only_release) else {
            return false;
        };
        if renderer.capture_waits_for_frame() {
            return false;
        }
        renderer.release_idle();
        self.frames.kept_frame_check = None;
        true
    }

    /// Tells the decorations how the window is: focused, shown, and whether the system asks
    /// for less motion or saves power (asked of the system every few seconds at most).
    fn sync_decorations_window(&mut self, window: &Arc<dyn Window>, now: Instant) {
        const ASK_EVERY: Duration = Duration::from_secs(5);
        let still = match self.window_state.system_still {
            Some((still, asked)) if now.saturating_duration_since(asked) < ASK_EVERY => still,
            _ => {
                let reduce = window.reduce_motion();
                // The views animating on their own read it too (Relax mode).
                ui_workbench::decorations::set_reduce_motion(reduce);
                let still = reduce || window.power_saving();
                self.window_state.system_still = Some((still, now));
                still
            }
        };
        let (focused, visible) = (self.window_state.focused, !self.window_state.occluded);
        self.workbench.set_decorations_window(focused, visible, still);
    }

    /// The next step of the resize diagnostic. `true` when the test is finished.
    fn step_resize_test(&mut self) -> bool {
        let (Some(test), Some(window)) = (&mut self.frames.resize_test, &self.window) else {
            return false;
        };
        if test.steps == 0 {
            test.report();
            self.frames.resize_test = None;
            return true;
        }
        test.steps -= 1;
        let base = PhysicalSize::new(1600, 1000);
        let grow = (test.steps % 10) * 60;
        window.request_inner_size(PhysicalSize::new(base.width + grow, base.height + grow / 2));
        false
    }
}

/// The results of the `EDITOR_DEBUG_RESIZE` diagnostic.
#[derive(Debug)]
struct ResizeTest {
    steps: u32,
    frames: Vec<Duration>,
    skipped: u32,
}

impl ResizeTest {
    fn record(&mut self, duration: Duration, skipped: bool) {
        self.frames.push(duration);
        self.skipped += u32::from(skipped);
    }

    fn report(&self) {
        let mut sorted = self.frames.clone();
        sorted.sort();
        let pick = |q: f32| {
            sorted
                .get(((sorted.len() as f32 * q) as usize).min(sorted.len().saturating_sub(1)))
                .copied()
                .unwrap_or_default()
        };
        tracing::debug!(
            frames = sorted.len(),
            skipped = self.skipped,
            p50 = ?pick(0.5),
            p95 = ?pick(0.95),
            max = ?sorted.last().copied().unwrap_or_default(),
            "diagnostic resize"
        );
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

/// The key combination of an event, for the keymap lookup.
fn chord(event: &KeyEvent) -> Option<KeyChord> {
    if altgr_text(event) {
        return None;
    }
    let key = match &event.key {
        // The keypad's + and − (Expand All / Collapse All), apart from the main row's.
        Key::Character(c) if event.numpad && (c == "+" || c == "-") => {
            ChordKey::Named(if c == "+" { "Numpadadd" } else { "Numpadsubtract" }.to_owned())
        }
        Key::Character(c) => {
            let c = c.chars().next()?.to_lowercase().next()?;
            // With Shift, the US layout reports `{`/`}` for the `[`/`]` keys.
            ChordKey::Char(match c {
                '{' => '[',
                '}' => ']',
                '+' => '=',
                '_' => '-',
                c => c,
            })
        }
        Key::Named(named) => ChordKey::Named(
            match named {
                NamedKey::ArrowUp => "Up",
                NamedKey::ArrowDown => "Down",
                NamedKey::ArrowLeft => "Left",
                NamedKey::ArrowRight => "Right",
                NamedKey::PageUp => "Pageup",
                NamedKey::PageDown => "Pagedown",
                NamedKey::Home => "Home",
                NamedKey::End => "End",
                NamedKey::Enter => "Enter",
                NamedKey::Escape => "Escape",
                NamedKey::Tab => "Tab",
                NamedKey::Backspace => "Backspace",
                NamedKey::Delete => "Delete",
                NamedKey::Insert => "Insert",
                NamedKey::Space => return Some(chord_with(event, ChordKey::Char(' '))),
                NamedKey::F(n) => return Some(chord_with(event, ChordKey::Named(format!("F{n}")))),
            }
            .to_owned(),
        ),
        Key::Unidentified => return None,
    };
    Some(chord_with(event, key))
}

/// A character typed with AltGr (Windows and Linux report it as Ctrl + Alt): `{`, `@`, `\\`,
/// `ș` on German, Polish, Romanian keyboards… It's text, never a shortcut. A real
/// Ctrl + Alt shortcut types nothing.
fn altgr_text(event: &KeyEvent) -> bool {
    let m = event.modifiers;
    !MAC && m.ctrl
        && m.alt
        && !m.meta
        && event.text.as_deref().is_some_and(|t| !t.is_empty() && !t.chars().any(char::is_control))
}

fn chord_with(event: &KeyEvent, key: ChordKey) -> KeyChord {
    let m = event.modifiers;
    KeyChord { primary: m.primary(), shift: m.shift, alt: m.alt, control: MAC && m.ctrl, key }
}

/// The room around what floats for its shadow (logical pixels): the floating layer draws
/// and takes the mouse there too.
const FLOATING_SHADOW: f32 = 28.0;

impl EditorApp {
    /// The layer over the pages made while a page is on screen (what floats drawn there,
    /// over it), let go once none is; its renderer at the window's size. Where none can be
    /// made (or it can't be transparent), holes are cut out of the pages instead.
    fn sync_floating(&mut self, window: &Arc<dyn Window>) {
        let wanted = self.web_views.any_shown()
            && !self.frames.floating_failed
            && self.frames.renderer.is_some();
        if !wanted {
            if let Some(floating) = self.frames.floating.take() {
                floating.layer.set_visible(false);
            }
            return;
        }
        let size = window.inner_size();
        if self.frames.floating.is_none() {
            let Some(layer) = window.floating_layer() else {
                self.frames.floating_failed = true;
                return;
            };
            let options = RenderOptions { transparent: true, ..self.shared.render_options };
            let target: Arc<dyn RenderTarget> = Arc::new(layer.handles());
            match WgpuRendererFactory.create(target, size, &options) {
                Ok(renderer) if renderer.transparent() => {
                    tracing::info!("what floats is drawn over the pages, on its own layer");
                    self.frames.floating =
                        Some(state::Floating { renderer, layer, scene: Scene::default() });
                }
                Ok(_) => {
                    tracing::info!("the floating layer can't be transparent: holes instead");
                    self.frames.floating_failed = true;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the floating layer's renderer didn't start");
                    self.frames.floating_failed = true;
                }
            }
        }
        if let Some(floating) = &mut self.frames.floating {
            floating.renderer.resize(size);
        }
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
