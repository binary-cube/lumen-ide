//! Platform contracts: window, input, application lifecycle.
//!
//! The app implements [`Application`]; a concrete platform (winit or another)
//! implements [`Platform`] and translates native events into [`PlatformEvent`].

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod access;

use kernel::{PhysicalSize, Point, Rect, Size};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// The window's native handles, for the render driver.
pub trait WindowHandles: HasWindowHandle + HasDisplayHandle + Send + Sync + fmt::Debug {}

impl<T: HasWindowHandle + HasDisplayHandle + Send + Sync + fmt::Debug + ?Sized> WindowHandles
    for T
{
}

/// A page shown by the system's web engine over a window ([`Window::web_view`]); gone when
/// dropped. Lives on the window's thread.
pub trait WebView: fmt::Debug {
    /// Where it is (physical pixels, in the window).
    fn set_bounds(&self, bounds: Rect);
    fn set_visible(&self, visible: bool);
    /// Another page in its place (an HTML page's; an app's keeps its own).
    fn load_html(&self, html: &str);
    /// The keyboard given back to the window (a click elsewhere in it).
    fn release_focus(&self) {}
    /// A message for an app's page: handed to the function it defines,
    /// `window.__hostReceive(message)` (nothing if it defines none). An HTML page runs no
    /// scripts: nothing reaches it.
    fn send(&self, message: &str) {
        let _ = message;
    }
    /// A picture of what it shows now, given to `done` later on the window's thread (the
    /// window asked for a frame then; `None`: none could be taken). `false`: this engine
    /// takes none, and `done` is never called.
    fn snapshot(&self, done: Box<dyn FnOnce(Option<WebPicture>)>) -> bool {
        drop(done);
        false
    }
    /// The places cut out of it ([`Window::web_view_holes`]): there the app's own drawing
    /// shows through and the mouse reaches the app, not the page (what floats over the page:
    /// a pinned window, a tooltip). Each a rectangle (physical pixels, in the window) with
    /// its corners' radius; none: the page whole. `cut`: the page isn't drawn there either
    /// (else only the mouse goes to the app: what floats is drawn over the page, on the
    /// window's [`FloatingLayer`]).
    fn set_holes(&self, holes: &[(Rect, f32)], cut: bool) {
        let _ = (holes, cut);
    }
    /// Its `corners` ([`Corners`]' bits) rounded by `radius` (physical pixels): as the card
    /// it's in, where it reaches that card's corners. None: square.
    fn set_corners(&self, radius: f32, corners: u8) {
        let _ = (radius, corners);
    }
}

/// A page's corners ([`WebView::set_corners`]), as bits.
#[derive(Debug, Clone, Copy)]
pub struct Corners;

impl Corners {
    pub const TOP_LEFT: u8 = 1;
    pub const TOP_RIGHT: u8 = 2;
    pub const BOTTOM_LEFT: u8 = 4;
    pub const BOTTOM_RIGHT: u8 = 8;
}

/// A transparent layer over the window's pages ([`Window::floating_layer`]): what the app
/// draws there (what floats: a window over the bar, notifications, menus, tooltips) is over
/// the pages too, with its shadows and its translucency — nothing cut out of them. The app
/// draws it with its own renderer (its handles); where nothing is drawn, the page or the
/// window under it shows. It never takes the mouse itself: the mouse over what floats
/// reaches the app (the pages' holes, [`WebView::set_holes`], or `set_shapes`). Lives on
/// the window's thread; gone when dropped.
pub trait FloatingLayer: fmt::Debug {
    /// Its native handles, for a renderer (a transparent surface).
    fn handles(&self) -> Arc<dyn WindowHandles>;
    /// Shown or hidden (hidden: nothing of it on screen, nothing taken).
    fn set_visible(&self, visible: bool);
    /// Where something floats now (physical pixels, in the window; each with its corners'
    /// radius, and with `margin` around it for its shadow): where it may draw and where the
    /// mouse is the app's. Where a platform needs it (Windows).
    fn set_shapes(&self, shapes: &[(Rect, f32)], margin: f32) {
        let _ = (shapes, margin);
    }
    /// Put back over every page (a page made after it goes on top).
    fn raise(&self) {}
}

/// A page's picture ([`WebView::snapshot`]): `width` × `height` pixels, RGBA, rows top
/// down.
#[derive(Clone, PartialEq, Eq)]
pub struct WebPicture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl fmt::Debug for WebPicture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebPicture({}x{})", self.width, self.height)
    }
}

/// What a page shows ([`Window::web_view`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSource<'a> {
    /// HTML given (a response's): shown as it is, its scripts don't run, nothing it links to
    /// is followed, it reaches no network.
    Html(&'a str),
    /// A trusted app on this computer: the files of `root` served to it through a scheme of
    /// the app's own (nothing outside the folder), its scripts running, no network; it
    /// talks to the host through messages. `entry`: the page opened first, relative to
    /// `root`, with its query (`index.html?embed=1`).
    App { root: &'a Path, entry: &'a str },
}

/// What an app's page tells the host ([`WebInbox`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebEvent {
    /// A message it sent (`window.ipc.postMessage(text)`).
    Message(String),
    /// A link it wanted opened in a new window: never opened by the engine, the host
    /// decides.
    OpenLink(String),
}

/// Where an app page's events go: called on the window's thread as they arrive (the window
/// is asked for a frame after each). A message longer than [`MAX_WEB_MESSAGE`] never
/// reaches it.
pub type WebInbox = Box<dyn Fn(WebEvent)>;

/// The longest message a page may send the host (bytes): a longer one is dropped (logged).
pub const MAX_WEB_MESSAGE: usize = 64 << 20;

/// Identifier of a window (unique within the process).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// The app window, seen independently of the windowing library.
pub trait Window: fmt::Debug {
    fn id(&self) -> WindowId;
    /// The system wants this window's accessibility tree (a screen reader runs): the app
    /// makes one with each frame ([`access`]). The number changes each time the system asks
    /// anew (the whole tree is told then); `None`: not wanted, or no such system.
    fn access_asked(&self) -> Option<u64> {
        None
    }
    /// The window's accessibility tree, as the last frame made it (told only when it
    /// changed): only what changed since `previous` (the tree told last), or whole (`None`).
    fn update_access(&self, frame: &access::Frame, previous: Option<&access::Frame>) {
        let _ = (frame, previous);
    }
    /// Opens another window in the same process (with the first window's options); it
    /// reaches the app via [`Application::window_opened`].
    fn open_window(&self);
    fn inner_size(&self) -> PhysicalSize;
    fn scale_factor(&self) -> f64;
    fn request_redraw(&self);
    /// How long the computer has gone without input in any program (`None`: not known).
    fn system_idle(&self) -> Option<std::time::Duration> {
        None
    }
    /// Requests a redraw at `at` (e.g. the next caret blink), without a thread and without
    /// frames in between; the earliest pending request per window wins.
    fn request_redraw_at(&self, at: std::time::Instant);
    /// Requests a new size (physical pixels); the result arrives as [`PlatformEvent::Resized`].
    fn request_inner_size(&self, size: PhysicalSize);
    fn set_title(&self, title: &str);
    fn handles(&self) -> Arc<dyn WindowHandles>;

    /// `true` if the shortcuts in [`WindowOptions::menus`] are handled by a native menu (they
    /// arrive as [`PlatformEvent::Command`]); otherwise the app handles them from the keyboard.
    fn has_native_menu(&self) -> bool;

    /// Native file picker dialog. Blocks until the dialog closes.
    fn pick_file(&self, start_dir: Option<&Path>) -> Option<PathBuf>;

    /// Native folder picker dialog. Blocks until the dialog closes.
    fn pick_folder(&self, start_dir: Option<&Path>) -> Option<PathBuf>;

    /// Native "Save As" dialog.
    fn pick_save_path(&self, start_dir: Option<&Path>, file_name: &str) -> Option<PathBuf>;

    fn clipboard_text(&self) -> Option<String>;
    fn set_clipboard_text(&self, text: &str);
    /// Puts an image on the system clipboard: `width` × `height` straight-alpha RGBA.
    fn set_clipboard_image(&self, _width: usize, _height: usize, _rgba: &[u8]) {}
    /// Files / folders on the system clipboard (copied in the file manager).
    fn clipboard_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    /// Puts files / folders on the system clipboard (pasteable in the file manager).
    fn set_clipboard_files(&self, _paths: &[PathBuf]) {}

    /// Area (physical pixels) next to which the system shows the IME composition window.
    fn set_ime_cursor_area(&self, area: Rect);

    fn set_cursor_icon(&self, icon: CursorIcon);

    /// Shows a native context menu at `position` (physical pixels, relative to the
    /// window). The selection arrives as [`PlatformEvent::Command`]. `false` if the platform
    /// has no native context menus (the app can draw its own).
    fn show_context_menu(&self, items: &[MenuItem], position: Point) -> bool;

    /// Starts moving the window with the mouse (click on an empty area of the top bar).
    fn drag_window(&self);

    /// Maximizes the window or restores it to its previous size.
    fn toggle_maximize(&self);

    /// Space (logical pixels) taken by the window buttons on the left of the top bar,
    /// when the title bar is unified (macOS); otherwise 0.
    fn titlebar_inset(&self) -> f32;

    /// The app draws the minimize / maximize / close buttons in its top bar (Windows, the
    /// system's title bar gone); `false` where the system draws them.
    fn window_controls(&self) -> bool {
        false
    }

    fn minimize(&self) {}

    /// Full screen (the whole screen, no system bars) or back to the window as it was.
    fn toggle_fullscreen(&self) {}

    /// The mouse is over the app's maximize button while the system has it there (Windows'
    /// snap layouts): the app draws its hover from this.
    fn maximize_hovered(&self) -> bool {
        false
    }

    fn is_fullscreen(&self) -> bool {
        false
    }

    /// The screen the window is on, as every process names it (its name and where it is
    /// among the screens); `None`: not known.
    fn display_id(&self) -> Option<String> {
        None
    }

    fn is_maximized(&self) -> bool {
        false
    }

    /// Where the app's maximize button is (physical pixels, in the window), so the system
    /// treats it as its own (Windows: the snap layouts on hover); `None`: none drawn.
    fn set_maximize_button(&self, rect: Option<Rect>) {
        let _ = rect;
    }

    /// Reveals the file in the system file manager (Finder, Explorer…).
    fn reveal_in_file_manager(&self, path: &Path);

    /// Opens a URL (`https://…`, `mailto:…`) in the system's default application.
    fn open_url(&self, url: &str);

    /// Opens a page the app wrote (an `.html` file; nothing else) in the system's browser.
    fn open_in_browser(&self, path: &Path) {
        let _ = path;
    }

    /// Whether its pages can have holes ([`WebView::set_holes`]): what floats over a page
    /// then shows through it; else the page is made shorter, or hidden, under it.
    fn web_view_holes(&self) -> bool {
        false
    }

    /// A page shown by the system's web engine over the window, at `bounds` (physical
    /// pixels, in the window): its own surface, above the app's (nothing the app draws goes
    /// over it — the app hides it while something is). It goes when it's dropped. `data`:
    /// the folder the engine keeps its own files in (the app's, writable: never beside the
    /// program). `inbox`: where an app page's events go (an HTML page sends none). `None`:
    /// the system has none here (the page is opened in the browser instead).
    fn web_view(
        &self,
        source: WebSource<'_>,
        bounds: Rect,
        data: &Path,
        inbox: WebInbox,
    ) -> Option<Box<dyn WebView>> {
        let _ = (source, bounds, data, inbox);
        None
    }

    /// A transparent layer over the pages ([`FloatingLayer`]); `None` where there's none
    /// (what floats then goes in holes cut out of the pages).
    fn floating_layer(&self) -> Option<Box<dyn FloatingLayer>> {
        None
    }

    /// The system's file manager, for the action above's label.
    fn file_manager(&self) -> FileManager;

    /// Starts picking a color anywhere on screen (an eyedropper with a magnifier). The
    /// result arrives as [`PlatformEvent::ScreenColor`] (`None`: cancelled). `false` if the
    /// platform can't do it (the app then hides the eyedropper).
    fn sample_screen_color(&self) -> bool {
        false
    }

    /// Replaces the app's menus (e.g. the shortcuts changed with the keymap preset). The
    /// native menu bar (macOS) is rebuilt; elsewhere the app handles shortcuts itself.
    fn set_menus(&self, _menus: Vec<Menu>) {}

    /// The menus' rows that don't apply now (their ids): greyed out in the native menu bar
    /// (macOS), the others enabled; kept over a [`Window::set_menus`].
    fn set_menu_states(&self, _disabled: Vec<String>) {}

    /// The app icon's menu: the recent projects (macOS: a submenu of the Dock's, under the
    /// open windows, sending each one's command as [`PlatformEvent::Command`]; Windows: the
    /// taskbar's jump list, starting the app with the project's path).
    fn set_icon_menu(&self, _menu: IconMenu) {}

    /// The app's sign outside its windows (`None`: gone) — see [`SystemSign`]. One for the
    /// whole app, whichever window sets it.
    fn set_system_sign(&self, _sign: Option<SystemSign>) {}

    /// Brings window `id` of this app to the front (unminimized).
    fn focus_window(&self, _id: WindowId) {}

    /// Whether [`Window::sample_screen_color`] is available.
    fn can_sample_screen_color(&self) -> bool {
        false
    }

    /// The system appearance (light / dark), if the platform reports it.
    fn system_appearance(&self) -> Option<SystemAppearance>;

    /// The user asked the system for less motion (accessibility): what only decorates
    /// stays still. Asked now and then, not on every frame.
    fn reduce_motion(&self) -> bool {
        false
    }

    /// The system is saving power (battery saver, Low Power Mode): what only decorates
    /// stays still.
    fn power_saving(&self) -> bool {
        false
    }

    /// Function that, called from any thread, wakes this window with
    /// [`PlatformEvent::Wake`] (e.g. a result from background parsing): only it looks at
    /// what finished — another window's project never makes it work.
    fn waker(&self) -> Arc<dyn Fn() + Send + Sync>;

    /// Like [`Self::waker`], for what every window shows (the settings, the resources):
    /// wakes them all.
    fn app_waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        self.waker()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAppearance {
    Light,
    Dark,
}

/// The system's file manager (the app names the "reveal" action after it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileManager {
    Finder,
    Explorer,
    /// Linux and others: "open the containing folder".
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Default,
    Text,
    Pointer,
    /// Horizontal resize (the separator between panels).
    ResizeHorizontal,
    /// Vertical resize (the separator above the terminal).
    ResizeVertical,
}

/// Input method (IME) events: character composition (diacritics via
/// dead keys, Chinese, Japanese, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    /// Text being composed; `cursor` is the highlighted byte range within it.
    Preedit { text: String, cursor: Option<(usize, usize)> },
    /// Final text, to insert.
    Commit(String),
}

/// A top-level menu (e.g. "File"). On macOS, the first menu becomes the app menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    /// The app's window list (macOS adds the open windows to it, and to the Dock's menu).
    pub windows: bool,
    /// Its icon (a name from the app's set), where the app draws its menus (the menu
    /// drawer); the native menu bar ignores it.
    pub icon: Option<&'static str>,
    /// The Help menu (macOS puts its search over every menu's rows in it).
    pub help: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Command {
        /// Identifier sent back in [`PlatformEvent::Command`].
        id: String,
        label: String,
        /// Shortcut, e.g. `"CmdOrCtrl+Shift+O"`.
        shortcut: Option<String>,
        /// Icon on the left (context menus).
        icon: Option<MenuIcon>,
    },
    Separator,
    /// A row opening more rows.
    Submenu {
        title: String,
        /// Its icon (a name from the app's set), where the app draws its menus (the menu
        /// drawer); the native menu bar ignores it.
        icon: Option<&'static str>,
        items: Vec<MenuItem>,
    },
    /// A row the system makes and runs itself (its keys the system's), labeled `label`:
    /// only in the native menu bar (macOS); where the app draws its menus it's left out.
    Native {
        item: NativeItem,
        label: String,
    },
}

/// The rows the system makes (macOS' application and Window menus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeItem {
    /// The Services submenu.
    Services,
    /// Hide the app (⌘H).
    Hide,
    /// Hide the other apps (⌥⌘H).
    HideOthers,
    /// Show every app again.
    ShowAll,
    /// Minimize the window (⌘M).
    Minimize,
    /// Zoom the window (as its title bar's green button does with ⌥).
    Zoom,
    /// Every window of the app to the front.
    BringAllToFront,
}

/// The app icon's menu: its recent projects, under a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconMenu {
    pub title: String,
    pub projects: Vec<IconProject>,
}

/// The app's sign outside its windows while something holds (the computer kept awake): an
/// icon by the system's clock (macOS' menu bar, Windows' notification area) with its menu,
/// and a mark on the app's icon (macOS' Dock badge, Windows' taskbar overlay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSign {
    pub tooltip: String,
    /// Square. macOS draws it as a template (in the menu bar's color, from its alpha);
    /// Windows as it is.
    pub icon: MenuIcon,
    /// The mark on the app's icon (macOS' Dock, Windows' taskbar buttons): square, drawn as
    /// it is (a round mark, the sign's icon in it).
    pub badge: MenuIcon,
    /// Its menu's rows ([`MenuItem::command`]: the ids come back as
    /// [`PlatformEvent::Command`]); a row with the id [`SHOW_APP`] brings the app to the front.
    pub menu: Vec<MenuItem>,
}

/// The id of [`SystemSign::menu`]'s row that brings the app's windows to the front (the
/// platform answers it).
pub const SHOW_APP: &str = "system-sign.show";

/// A project in the app icon's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconProject {
    pub label: String,
    pub path: PathBuf,
    /// Sent as [`PlatformEvent::Command`] when chosen (where the menu is the app's own).
    pub command: String,
}

/// Icon of a menu item: RGBA pixels (non-premultiplied), `width`×`height`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl MenuItem {
    pub fn command(id: &str, label: &str, shortcut: Option<&str>) -> Self {
        Self::Command {
            id: id.into(),
            label: label.into(),
            shortcut: shortcut.map(Into::into),
            icon: None,
        }
    }

    /// The same item, with an icon.
    pub fn with_icon(mut self, new: Option<MenuIcon>) -> Self {
        if let Self::Command { icon, .. } = &mut self {
            *icon = new;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowOptions {
    pub title: String,
    /// Initial size, in logical pixels.
    pub size: Size,
    pub min_size: Size,
    pub menus: Vec<Menu>,
    /// Title bar unified with the app bar, of the given height (logical pixels):
    /// on macOS the content extends under the title bar, and the window buttons are centered
    /// on it; on Windows the system's title bar goes (the app draws the window's buttons:
    /// [`Window::window_controls`]). On other platforms the native bar remains.
    pub unified_titlebar: Option<f32>,
    /// The window shows what's behind it where the app leaves it transparent (Windows: its
    /// corners, cut by the app; drawn through DirectComposition, no system shadow).
    pub transparent: bool,
    /// The app's icon as a PNG, for the windows on Linux and the Dock on macOS when run
    /// outside the app bundle (`cargo run`). Windows takes the icon from the executable's
    /// resources instead; a bundle, from its own.
    pub icon: Option<&'static [u8]>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: kernel::brand::NAME.into(),
            size: Size::new(1280.0, 800.0),
            min_size: Size::new(400.0, 300.0),
            menus: Vec::new(),
            unified_titlebar: None,
            transparent: false,
            icon: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Cmd on macOS, the Windows / Super key elsewhere.
    pub meta: bool,
}

impl Modifiers {
    /// Primary modifier for shortcuts: Cmd on macOS, Ctrl elsewhere.
    pub fn primary(&self) -> bool {
        if cfg!(target_os = "macos") { self.meta } else { self.ctrl }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Space,
    F(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Named(NamedKey),
    /// Character produced by the key, taking the layout into account (e.g. `"a"`, `"ă"`).
    Character(String),
    Unidentified,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: Modifiers,
    /// Text to insert, if the key produces text.
    pub text: Option<String>,
    /// The key is on the numeric keypad (its + and − are shortcuts of their own).
    pub numpad: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Mouse wheel: number of lines.
    Lines { x: f32, y: f32 },
    /// Trackpad: physical pixels, already smoothed by the system.
    Pixels { x: f32, y: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEvent {
    Resized(PhysicalSize),
    /// What a screen reader asked of a node of the window's accessibility tree.
    Access(access::Request),
    /// The color picked with the screen eyedropper (`None`: cancelled).
    ScreenColor(Option<kernel::Rgba>),
    ScaleFactorChanged(f64),
    RedrawRequested,
    CloseRequested,
    Focused(bool),
    /// `true` when the window becomes fully hidden, `false` when it becomes visible again.
    Occluded(bool),
    Key(KeyEvent),
    Scroll(ScrollDelta),
    /// Trackpad pinch gesture: how much it zoomed in (positive) or out
    /// (negative) since the previous step, e.g. `0.05` = +5%.
    Pinch(f32),
    /// Mouse cursor position, in physical pixels.
    CursorMoved(Point),
    /// The mouse cursor left the window (moves come again as it returns).
    CursorLeft,
    /// The held modifier keys changed.
    ModifiersChanged(Modifiers),
    /// Button pressed, at the current cursor position (physical pixels).
    MouseDown {
        button: MouseButton,
        position: Point,
        modifiers: Modifiers,
    },
    MouseUp {
        button: MouseButton,
        position: Point,
    },
    /// Command from the native menu (see [`MenuItem::Command`]).
    Command(String),
    Ime(ImeEvent),
    /// The user changed the system theme.
    SystemAppearanceChanged(SystemAppearance),
    /// A background task has results (see [`Window::waker`]).
    Wake,
    /// The native menu bar is about to show (macOS): the rows' states it should have now
    /// (`Window::set_menu_states`), read again whatever changed.
    MenuOpening,
    /// Files or folders from the system's file manager are dragged over the window: all the
    /// items, once, as the drag enters, with the pointer (physical pixels). While the drag
    /// stays over the window the pointer's moves come as [`PlatformEvent::CursorMoved`]
    /// (where the system tells; otherwise the last known position stays).
    FilesHovered {
        paths: Vec<PathBuf>,
        position: Point,
    },
    /// That drag left the window or was cancelled: nothing was dropped.
    FilesHoverCancelled,
    /// Files or folders were dropped on the window: all the items, in order, at the pointer
    /// (physical pixels). Ends the hover (no [`PlatformEvent::FilesHoverCancelled`] follows).
    FilesDropped {
        paths: Vec<PathBuf>,
        position: Point,
    },
}

/// What the platform does after the app has handled an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Continue,
    /// Closes the window that received the event (the app exits after the last one).
    Close,
    /// Closes all windows and exits the app.
    Exit,
}

/// The app, as the platform sees it. All windows run in the same process, on
/// the main thread; events arrive with the window they belong to.
pub trait Application {
    /// A new window was created and can be used for rendering.
    fn window_opened(&mut self, window: Arc<dyn Window>);

    fn event(&mut self, window: WindowId, event: PlatformEvent) -> Control;

    /// The process ends (quit, the system's own quit too): what's still to write, written.
    fn exiting(&mut self) {}

    /// The system asked the app to open `paths` (macOS: Finder's "Open With", a file dropped
    /// on the Dock's icon), before its first window too.
    fn paths_opened(&mut self, _paths: Vec<std::path::PathBuf>) {}
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("can't start the event loop: {0}")]
    EventLoop(String),
    #[error("can't create the window: {0}")]
    Window(String),
    /// What makes the app (see [`Platform::run_prepared`]) failed.
    #[error("{0}")]
    Start(String),
}

/// Makes the first window's options and the app, once the event loop exists (see
/// [`Platform::run_prepared`]); an error ends the start.
pub type Prepare = Box<dyn FnOnce() -> Result<(WindowOptions, Box<dyn Application>), String>>;

/// The concrete platform implementation.
pub trait Platform: Sized {
    /// Runs the event loop until the app closes.
    fn run(self, options: WindowOptions, app: Box<dyn Application>) -> Result<(), PlatformError> {
        self.run_prepared(Box::new(move || Ok((options, app))))
    }

    /// Runs the event loop until the app closes, with the options and the app `prepare`
    /// makes once the loop is built: the system's part of starting (the loop, the app's
    /// object) and the app's own (on other threads meanwhile) go side by side.
    fn run_prepared(self, prepare: Prepare) -> Result<(), PlatformError>;
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
