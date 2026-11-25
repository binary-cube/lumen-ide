//! Platform implementation on top of winit (+ rfd for dialogs, muda for the macOS menu).

mod access_bridge;
mod app_icon;
#[cfg(target_os = "windows")]
mod caption_windows;
mod color_sampler;
#[cfg(target_os = "windows")]
mod color_sampler_windows;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod context_menu;
#[cfg(target_os = "macos")]
mod dock;
mod events;
mod file_drag;
#[cfg(target_os = "macos")]
#[path = "floating_macos.rs"]
mod floating;
#[cfg(target_os = "windows")]
#[path = "floating_windows.rs"]
mod floating;
mod ime_caret;
#[cfg(target_os = "windows")]
mod jump_list;
#[cfg(target_os = "macos")]
mod menu;
mod motion;
#[cfg(target_os = "macos")]
mod opened;
mod system_idle;
#[cfg(target_os = "macos")]
mod system_sign;
#[cfg(target_os = "windows")]
mod system_sign_windows;
#[cfg(target_os = "macos")]
mod titlebar;
#[cfg(target_os = "macos")]
mod web_holes;
#[cfg(target_os = "windows")]
#[path = "web_holes_windows.rs"]
mod web_holes;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod web_view;
mod window;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use kernel::{PhysicalSize, Point, Rect};
use platform_api::{
    Application, Control, CursorIcon, FileManager, ImeEvent, Key, KeyEvent, MenuItem, Modifiers,
    MouseButton, NamedKey, Platform, PlatformError, PlatformEvent, ScrollDelta, SystemAppearance,
    Window, WindowHandles, WindowOptions,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{self, ModifiersState};
use winit::window::WindowId;

/// Events sent into the winit loop from outside it (e.g. from the native menu).
#[derive(Debug)]
pub(crate) enum UserEvent {
    /// A native menu's row chosen (the menus are macOS' and Windows' only).
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    Command(String),
    /// A background task finished: for this window (its pending flag cleared then), or for
    /// every window (`None`).
    Wake(Option<(WindowId, Arc<AtomicBool>)>),
    /// A timed redraw was requested (see `request_redraw_at`).
    Timer,
    /// A new window (requested by the app).
    OpenWindow,
    /// New menus (see `Window::set_menus`).
    SetMenus(Vec<platform_api::Menu>),
    /// The rows that don't apply now (see `Window::set_menu_states`).
    SetMenuStates(Vec<String>),
    /// The menu bar is about to show (macOS): its rows' states asked for again.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    MenuOpening,
    /// The keyboard's input source changed (macOS): see [`ime_caret`].
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    InputSourceChanged,
    /// The app icon's menu (see `Window::set_icon_menu`).
    SetIconMenu(platform_api::IconMenu),
    /// The app's sign outside its windows (see `Window::set_system_sign`).
    SetSystemSign(Option<platform_api::SystemSign>),
    /// A window to the front (see `Window::focus_window`).
    Focus(platform_api::WindowId),
    /// The screen eyedropper finished, for this window.
    ScreenColor(WindowId, Option<kernel::Rgba>),
    /// A native dialog closed: the modifier keys released while it was up never reached the
    /// window — they're assumed released (otherwise ⌘ would stay "held").
    ResetModifiers(WindowId),
    /// The system's accessibility: its tree asked for, a node acted on, a screen reader gone.
    Access(accesskit_winit::Event),
    /// A window's accessibility tree's update, for the system's ask it was made for.
    AccessTree(WindowId, u64, accesskit::TreeUpdate),
    /// Files the system asked the app to open (macOS: Finder's "Open With", the Dock).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PathsOpened(Vec<std::path::PathBuf>),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Access(event)
    }
}

/// The winit platform (macOS, Windows, Linux X11/Wayland).
#[derive(Debug, Default)]
pub struct WinitPlatform;

impl Platform for WinitPlatform {
    fn run_prepared(self, prepare: platform_api::Prepare) -> Result<(), PlatformError> {
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .map_err(|e| PlatformError::EventLoop(e.to_string()))?;
        event_loop.set_control_flow(ControlFlow::Wait);
        // The files the system opens with the app (before the first window too).
        #[cfg(target_os = "macos")]
        opened::listen(event_loop.create_proxy());
        let (options, app) = prepare().map_err(PlatformError::Start)?;
        let mut handler = Handler {
            proxy: event_loop.create_proxy(),
            timers: Timers::default(),
            options,
            app,
            windows: Vec::new(),
            focused: None,
            started: false,
            native_menu: None,
            #[cfg(target_os = "macos")]
            menu_disabled: Vec::new(),
            error: None,
        };
        event_loop.run_app(&mut handler).map_err(|e| PlatformError::EventLoop(e.to_string()))?;
        handler.error.map_or(Ok(()), Err)
    }
}

/// A `Wake` for every window is queued and not yet handled (see the app's waker).
static WAKE_PENDING: AtomicBool = AtomicBool::new(false);

/// Pending timed redraws (window, when), shared by the windows and the event loop.
type Timers = Arc<Mutex<Vec<(WindowId, Instant)>>>;

struct WinitWindow {
    window: Arc<winit::window::Window>,
    timers: Timers,
    proxy: Mutex<EventLoopProxy<UserEvent>>,
    /// A `Wake` for this window is queued and not yet handled (see the waker).
    wake_pending: Arc<AtomicBool>,
    native_menu: bool,
    /// Created on first use; some platforms have no clipboard available.
    clipboard: Mutex<Option<arboard::Clipboard>>,
    /// Space taken by the window buttons in the unified bar (logical pixels), or 0.
    titlebar_inset: f32,
    /// The system's asks for its accessibility tree (see [`access_bridge`]).
    access_asked: Arc<std::sync::atomic::AtomicU64>,
    /// Windows without the system's title bar: where the app's maximize button is.
    #[cfg(target_os = "windows")]
    caption: Option<Arc<caption_windows::Caption>>,
    /// The caret's place for the input methods (see [`ime_caret`]).
    ime: Arc<Mutex<ime_caret::ImeCaret>>,
}

impl std::fmt::Debug for WinitWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitWindow").field("window", &self.window.id()).finish_non_exhaustive()
    }
}

impl WinitWindow {
    /// See [`UserEvent::ResetModifiers`].
    fn after_native_dialog(&self) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::ResetModifiers(self.window.id()));
        }
    }

    fn with_clipboard<T>(&self, f: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
        let mut guard = self.clipboard.lock().ok()?;
        if guard.is_none() {
            *guard = arboard::Clipboard::new()
                .inspect_err(|e| tracing::warn!(error = %e, "the clipboard isn't available"))
                .ok();
        }
        guard.as_mut().map(f)
    }
}

/// Position of the first window button and the total space taken by all three (logical).
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHTS_LEFT: f64 = 14.0;
const TRAFFIC_LIGHTS_INSET: f32 = 84.0;

/// Platform state for one window.
struct WinState {
    window: Arc<winit::window::Window>,
    modifiers: Modifiers,
    cursor: Point,
    /// Files dragged from the file manager over it.
    drag: file_drag::FileDrag,
    /// The caret's place for the input methods (the window's [`WinitWindow::ime`]).
    ime: Arc<Mutex<ime_caret::ImeCaret>>,
    /// Its accessibility adapter (see [`access_bridge`]).
    access: access_bridge::Access,
}

struct Handler {
    proxy: EventLoopProxy<UserEvent>,
    /// See [`Timers`].
    timers: Timers,
    options: WindowOptions,
    app: Box<dyn Application>,
    windows: Vec<WinState>,
    /// The active window (receives commands from the native menu).
    focused: Option<WindowId>,
    started: bool,
    /// The menu must be kept alive while the app runs.
    #[cfg(target_os = "macos")]
    native_menu: Option<menu::NativeMenu>,
    /// The rows that don't apply now (kept for a menu bar made again).
    #[cfg(target_os = "macos")]
    menu_disabled: Vec<String>,
    #[cfg(not(target_os = "macos"))]
    native_menu: Option<()>,
    error: Option<PlatformError>,
}

fn api_id(id: WindowId) -> platform_api::WindowId {
    platform_api::WindowId(u64::from(id))
}

impl Handler {
    /// On macOS, with the unified title bar: the window buttons, centered on our bar.
    fn layout_titlebar(&self, window: &winit::window::Window) {
        #[cfg(target_os = "macos")]
        if let Some(height) = self.options.unified_titlebar {
            titlebar::layout_buttons(window, f64::from(height), TRAFFIC_LIGHTS_LEFT);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window;
    }

    fn titlebar_inset(&self) -> f32 {
        if cfg!(target_os = "macos") && self.options.unified_titlebar.is_some() {
            TRAFFIC_LIGHTS_INSET
        } else {
            0.0
        }
    }

    fn state(&mut self, id: WindowId) -> Option<&mut WinState> {
        self.windows.iter_mut().find(|w| w.window.id() == id)
    }

    fn dispatch(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: PlatformEvent) {
        match self.app.event(api_id(id), event) {
            Control::Continue => {}
            Control::Close => {
                // Hidden immediately: it disappears even if something (e.g. the GPU surface) keeps
                // it alive for a few moments; the other windows redraw and one gets focus, so they
                // don't look frozen.
                if let Some(state) = self.windows.iter().find(|w| w.window.id() == id) {
                    state.window.set_visible(false);
                }
                self.windows.retain(|w| w.window.id() != id);
                tracing::info!(?id, remaining = self.windows.len(), "window closed");
                if self.focused == Some(id) {
                    self.focused = self.windows.last().map(|w| w.window.id());
                    if let Some(next) = self.windows.last() {
                        next.window.focus_window();
                    }
                }
                for w in &self.windows {
                    w.window.request_redraw();
                }
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            Control::Exit => event_loop.exit(),
        }
    }

    fn open_window(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(unused_mut))]
        let mut attributes = winit::window::Window::default_attributes()
            .with_title(self.options.title.clone())
            .with_inner_size(LogicalSize::new(self.options.size.width, self.options.size.height))
            .with_min_inner_size(LogicalSize::new(
                self.options.min_size.width,
                self.options.min_size.height,
            ));
        #[cfg(target_os = "macos")]
        if self.options.unified_titlebar.is_some() {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attributes = attributes
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true);
        }
        // Windows: the system's title bar goes (the app's bar is it). Transparent (its
        // corners cut by the app), without the system's shadow — square, it would show
        // around the rounded corners — and without the bitmap the system would draw behind
        // (DirectComposition shows the app's frames, alpha included); otherwise its shadow.
        #[cfg(target_os = "windows")]
        if self.options.unified_titlebar.is_some() {
            use winit::platform::windows::WindowAttributesExtWindows;
            let transparent = self.options.transparent;
            attributes = attributes
                .with_decorations(false)
                .with_undecorated_shadow(!transparent)
                .with_no_redirection_bitmap(transparent);
        }
        // Shown once its accessibility adapter is made (it must be made before).
        let attributes = app_icon::with_icon(attributes, self.options.icon).with_visible(false);
        match event_loop.create_window(attributes) {
            Ok(window) => {
                let access = access_bridge::Access {
                    adapter: accesskit_winit::Adapter::with_event_loop_proxy(
                        event_loop,
                        &window,
                        self.proxy.clone(),
                    ),
                    asks: access_bridge::Asks::default(),
                };
                let access_asked = Arc::clone(&access.asks.asked);
                let id = window.id();
                // Without IME, dead keys (e.g. composed diacritics) don't work on macOS.
                window.set_ime_allowed(true);
                let window = Arc::new(window);
                self.layout_titlebar(&window);
                self.focused = Some(window.id());
                let ime = ime_caret::ImeCaret::shared();
                self.windows.push(WinState {
                    window: Arc::clone(&window),
                    modifiers: Modifiers::default(),
                    cursor: Point::default(),
                    drag: file_drag::FileDrag::default(),
                    ime: Arc::clone(&ime),
                    access,
                });
                let native_menu = self.native_menu.is_some();
                #[cfg(target_os = "windows")]
                let caption =
                    self.options.unified_titlebar.and_then(|_| caption_windows::install(&window));
                #[cfg(target_os = "macos")]
                dock::list_window(&window);
                #[cfg(target_os = "windows")]
                system_sign_windows::window_opened(&window);
                self.app.window_opened(Arc::new(WinitWindow {
                    window,
                    timers: Arc::clone(&self.timers),
                    proxy: Mutex::new(self.proxy.clone()),
                    wake_pending: Arc::default(),
                    native_menu,
                    clipboard: Mutex::new(None),
                    titlebar_inset: self.titlebar_inset(),
                    access_asked,
                    #[cfg(target_os = "windows")]
                    caption,
                    ime,
                }));
                // Shown once all of it is set (its bar, its caption, what the app restored):
                // it was made hidden for its accessibility adapter.
                if let Some(state) = self.windows.iter().find(|w| w.window.id() == id) {
                    state.window.set_visible(true);
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "can't create the window");
                if self.windows.is_empty() {
                    self.error = Some(PlatformError::Window(e.to_string()));
                    event_loop.exit();
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn install_menu(&mut self) {
        if self.options.menus.is_empty() {
            return;
        }
        match menu::install(&self.options.menus, self.proxy.clone()) {
            Ok(m) => {
                m.set_disabled(&self.menu_disabled);
                self.native_menu = Some(m);
            }
            Err(e) => tracing::warn!(error = %e, "can't create the native menu"),
        }
        // A new window list starts empty: the open windows go in it (and in the Dock's menu).
        for w in &self.windows {
            dock::list_window(&w.window);
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn install_menu(&mut self) {
        // Windows/Linux: the menu bar will be drawn by the app; until then,
        // shortcuts are handled by the app from the keyboard. Native context menus
        // (Windows) still send commands.
        #[cfg(target_os = "windows")]
        context_menu::forward_events(self.proxy.clone());
        let _ = &self.proxy;
    }
}

impl Handler {
    /// The file manager's drags, told to their windows now that the loop is idle (winit
    /// gives their items one by one): a hover as it enters, a drop, and while one is over a
    /// window, its pointer's moves. `true` if a drag is over a window (its pointer followed).
    fn tell_file_drags(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let ids: Vec<WindowId> = self.windows.iter().map(|w| w.window.id()).collect();
        let mut dragging = false;
        for id in ids {
            let Some(state) = self.state(id) else { continue };
            let news = state.drag.take_news();
            let active = state.drag.active();
            if news.is_none() && !active {
                continue;
            }
            // The pointer, asked of the system (the last known one where it can't be).
            let moved = file_drag::pointer(&state.window).filter(|p| *p != state.cursor);
            if let Some(p) = moved {
                state.cursor = p;
            }
            let position = state.cursor;
            match news {
                Some(file_drag::DragNews::Hovered(paths)) => {
                    self.dispatch(event_loop, id, PlatformEvent::FilesHovered { paths, position });
                }
                Some(file_drag::DragNews::Dropped(paths)) => {
                    self.dispatch(event_loop, id, PlatformEvent::FilesDropped { paths, position });
                }
                None => {
                    if moved.is_some() {
                        self.dispatch(event_loop, id, PlatformEvent::CursorMoved(position));
                    }
                }
            }
            dragging |= self.state(id).is_some_and(|s| s.drag.active());
        }
        dragging
    }
}
