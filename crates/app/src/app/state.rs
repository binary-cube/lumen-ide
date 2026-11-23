//! The window's state, grouped by purpose: the window as shown, the mouse, the frames,
//! the start (splash), what was applied from the shared state, and what waits on an answer.

use super::*;

/// The window as the system shows it.
#[derive(Debug, Default)]
pub(super) struct WindowState {
    /// The window is hidden (another desktop, minimized, the screen locked or asleep):
    /// nothing is drawn — macOS hands out no drawables then, and each frame asked for would
    /// wait up to a second on the main thread. Drawn again once visible.
    pub occluded: bool,
    /// The window went full screen for a view over it: left again when that view closes.
    pub full_screen_for_view: bool,
    /// The view over every window this window started on the whole screen: its ticket
    /// (another instance's on the same screen, started first, makes it give way).
    pub ticket: Option<crate::over::Ticket>,
    /// The window has keyboard focus (its caret blinks).
    pub focused: bool,
    /// The screen eyedropper is running: the app stays idle (no hover, no frames) so the
    /// main thread, which also runs the system's loupe, is all the loupe's.
    pub sampling: bool,
    pub title: String,
    /// Where the system was last told the text caret is (its input methods' popups go
    /// there): told again only when it moves.
    pub ime_area: Option<kernel::Rect>,
    /// The state the menu bar's rows were last greyed out for (`None`: to push again).
    pub menu_context: Option<command_model::MenuContext>,
    /// What File › Open Recent was last made from (the recent projects' generation, the
    /// project, its recent files' count and latest; `None`: to make again).
    pub recent_menu: Option<RecentMenuKey>,
    /// What the system says of motion (reduced motion, saving power), and when it was asked
    /// (asked again after a few seconds, never on every frame).
    pub system_still: Option<(bool, Instant)>,
    /// When it last took the focus, among the windows (`Shared::focus_turn`).
    pub focus_turn: u64,
    /// When its picture was last taken (the open projects' list): taken again as it loses
    /// the focus only after [`super::projects::PICTURE_EVERY`].
    pub pictured: Option<Instant>,
    /// Hidden since then: its wait to give back the image on screen ([`super::HIDDEN_AFTER`]).
    pub hidden: Option<(Instant, kernel::work::Later)>,
}

/// What File › Open Recent is made from: the recent projects' generation, the project, its
/// recent files' count and latest.
pub(super) type RecentMenuKey =
    (u64, Option<std::path::PathBuf>, usize, Option<std::path::PathBuf>);

/// The mouse: where it is, its cursor, a drag, the last click (for multi-clicks), and where
/// the last context menu was opened.
#[derive(Debug)]
pub(super) struct Pointer {
    pub cursor: Point,
    pub cursor_icon: CursorIcon,
    pub dragging: bool,
    pub last_click: Option<(Instant, Point, u32)>,
    /// The tree node the context menu was opened on.
    pub tree_menu_path: Option<std::path::PathBuf>,
    /// Where the last context menu was shown (the "Copy" popover opens there).
    pub menu_position: Option<Point>,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            cursor: Point::default(),
            cursor_icon: CursorIcon::Default,
            dragging: false,
            last_click: None,
            tree_menu_path: None,
            menu_position: None,
        }
    }
}

/// The window's layer over the pages ([`platform_api::FloatingLayer`]): what floats (the
/// overlays, the tooltips) drawn into its scene by its own renderer, over the pages — whole,
/// nothing cut out of them.
pub(super) struct Floating {
    /// Let go before its layer (the fields' order).
    pub renderer: Box<dyn Renderer>,
    pub layer: Box<dyn platform_api::FloatingLayer>,
    pub scene: Scene,
}

/// What painting a frame uses and measures: the renderer, the scene, the debug overlay,
/// the input latency and the resize diagnostic.
pub(super) struct Frames {
    pub renderer: Option<Box<dyn Renderer>>,
    /// While a profile records: the times handed to the window for a frame, each with whose
    /// it is — counted once, by the frame that comes for them.
    pub asked: Vec<(&'static str, Instant)>,
    /// The layer over the pages, what floats drawn there (only while a page is on screen).
    pub floating: Option<Floating>,
    /// No floating layer could be made here (none, or not transparent): holes are cut out
    /// of the pages instead, not tried again.
    pub floating_failed: bool,
    pub overlay: DebugOverlay,
    pub scene: Scene,
    /// The moment of the first input not yet painted (for input → frame latency).
    pub pending_input: Option<Instant>,
    /// The redraw asked for when the kept frame can go.
    pub kept_frame_check: Option<Instant>,
    /// `EDITOR_DEBUG_RESIZE` diagnostic: how many resizes are left and the frame durations.
    pub resize_test: Option<ResizeTest>,
    /// Something happened since the last whole frame (an event, a redraw asked): the next
    /// frame is whole. Else, until [`Self::full_due`], a frame may draw only the theme's
    /// decorations again over the kept scene.
    pub dirty: bool,
    /// When the next whole frame is due anyway (the earliest deadline the last one asked
    /// for: the caret, a hover, a notification…; at most a second away).
    pub full_due: Option<Instant>,
    /// The earliest frame something asked for, the kept frame's release aside (`None`:
    /// nothing within the second [`Self::full_due`] looks ahead).
    pub other_due: Option<Instant>,
    /// The frames that drew only the decorations, and the whole ones (diagnostics).
    pub decoration_frames: u64,
    pub whole_frames: u64,
    /// The accessibility tree the last frame made (while a screen reader runs): told to the
    /// system again only when a frame changes it, and where a node it acts on is.
    pub access: Option<super::access::Told>,
}

impl Frames {
    pub fn new() -> Self {
        Self {
            renderer: None,
            access: None,
            floating: None,
            floating_failed: false,
            overlay: DebugOverlay::new(String::new()),
            scene: Scene::default(),
            pending_input: None,
            kept_frame_check: None,
            resize_test: std::env::var("EDITOR_DEBUG_RESIZE")
                .ok()
                .and_then(|n| n.parse().ok())
                .map(|steps| ResizeTest { steps, frames: Vec::new(), skipped: 0 }),
            dirty: true,
            full_due: None,
            asked: Vec::new(),
            other_due: None,
            decoration_frames: 0,
            whole_frames: 0,
        }
    }
}

/// The start: the splash and the first frame shown.
#[derive(Debug)]
pub(super) struct Startup {
    /// When the splash shows (only while the window isn't ready).
    pub splash: super::splash::SplashClock,
    pub first_frame_logged: bool,
    /// The first frame showing everything (the active tab's colors) was logged.
    pub ready_logged: bool,
}

impl Startup {
    pub fn new() -> Self {
        Self {
            splash: super::splash::SplashClock::new(
                std::env::var_os("EDITOR_NO_SPLASH").is_none()
                    && std::env::var_os("EDITOR_DEBUG_RESIZE").is_none(),
            ),
            first_frame_logged: false,
            ready_logged: false,
        }
    }

    /// What the frame at `frame_start` shows: the splash (alone, or fading out over the
    /// workbench), only the background, or the workbench; `ready`: the workbench has
    /// everything to show. The splash's art is let go once it's over.
    pub fn splash_at(&mut self, frame_start: Instant, ready: bool) -> super::splash::SplashShow {
        let (show, ended) = self.splash.frame(frame_start, ready);
        if let Some(shown) = ended {
            tracing::info!(phase = "splash over", shown, elapsed = ?crate::startup::elapsed(), "startup phase");
            if shown {
                crate::resources::release_splash_art();
            }
        }
        show
    }
}

/// The shared state's versions the window has applied (see [`Shared`]).
#[derive(Debug, Default)]
pub(super) struct Applied {
    /// The theme / language versions applied in the window.
    pub theme_version: u64,
    pub locale_version: u64,
    pub keymap_version: u64,
    /// The code font family was given to the workbench.
    pub font_applied: bool,
    /// The change of the view over every window followed last.
    pub everywhere_turn: u64,
}

/// What waits on an answer: the tab closing in progress, the dialog's question and what
/// must be returned to the platform after the answer (closing the window, quitting).
#[derive(Default)]
pub(super) struct Pending {
    pub closing: Option<Closing>,
    pub asking: Option<Ask>,
    pub exit_request: Option<Control>,
    /// A closing asked about the work still running, waiting on the answer (or, told to
    /// wait, on the work).
    pub running: Option<RunningAsk>,
    /// What an earlier run left running, asked about one by one (the first asked now),
    /// and how many there were in all.
    pub left_behind: std::collections::VecDeque<process_registry::Found>,
    pub left_behind_total: usize,
    /// An update to put in place once the app has quit (asked by the updates' module): its
    /// staged folder. A Quit of the user's own forgets it.
    pub update: Option<std::path::PathBuf>,
    /// The app ending, what it started being stopped before the window goes.
    pub ending: Option<super::ending::Ending>,
}

/// What follows once the work still running is let go: a closing (its tabs, then what it
/// does), or what a module asked to run (its restart).
pub(super) enum AfterRunning {
    Close(Final),
    Run(workbench_model::OnYes),
}

impl std::fmt::Debug for AfterRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Close(then) => f.debug_tuple("Close").field(then).finish(),
            Self::Run(_) => f.write_str("Run"),
        }
    }
}

/// A closing held by the question about the work still running.
#[derive(Debug)]
pub(super) struct RunningAsk {
    /// What closes: what's asked about.
    pub scope: workbench_model::CloseScope,
    pub closing: workbench_model::running::Closing,
    /// Only this module's work (a restart asked by it; `None`: a closing, everyone's).
    pub owner: Option<String>,
    /// What the closing does once let go.
    pub then: AfterRunning,
    /// The user chose to wait for the work to end.
    pub waiting: bool,
    /// Asked in the module's modal (else in the plain dialog).
    pub modal: bool,
    /// When the work was last looked at (while waiting).
    pub looked: Instant,
}
