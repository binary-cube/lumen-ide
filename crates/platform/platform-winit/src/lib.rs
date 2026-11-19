//! Platform implementation on top of winit (+ rfd for dialogs, muda for the macOS menu).

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
mod file_drag;
mod ime_caret;
#[cfg(target_os = "windows")]
mod jump_list;
#[cfg(target_os = "macos")]
mod menu;
mod motion;
mod system_idle;
#[cfg(target_os = "macos")]
mod system_sign;
#[cfg(target_os = "windows")]
mod system_sign_windows;
#[cfg(target_os = "macos")]
mod titlebar;
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
mod web_app;
#[cfg(target_os = "macos")]
mod web_holes;
#[cfg(target_os = "windows")]
#[path = "web_holes_windows.rs"]
mod web_holes;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod web_view;

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
#[derive(Debug, Clone)]
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
                .inspect_err(|e| tracing::warn!("the clipboard isn't available: {e}"))
                .ok();
        }
        guard.as_mut().map(f)
    }
}

impl Window for WinitWindow {
    fn id(&self) -> platform_api::WindowId {
        platform_api::WindowId(u64::from(self.window.id()))
    }

    fn set_menus(&self, menus: Vec<platform_api::Menu>) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::SetMenus(menus));
        }
    }

    fn set_menu_states(&self, disabled: Vec<String>) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::SetMenuStates(disabled));
        }
    }

    fn set_icon_menu(&self, menu: platform_api::IconMenu) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::SetIconMenu(menu));
        }
    }

    fn set_system_sign(&self, sign: Option<platform_api::SystemSign>) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::SetSystemSign(sign));
        }
    }

    fn focus_window(&self, id: platform_api::WindowId) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::Focus(id));
        }
    }

    fn can_sample_screen_color(&self) -> bool {
        crate::color_sampler::SUPPORTED
    }

    fn sample_screen_color(&self) -> bool {
        let Ok(proxy) = self.proxy.lock().map(|p| p.clone()) else { return false };
        let id = self.window.id();
        crate::color_sampler::sample(Box::new(move |color| {
            let _ = proxy.send_event(UserEvent::ScreenColor(id, color));
        }))
    }

    fn request_redraw_at(&self, at: Instant) {
        let id = self.window.id();
        let mut sooner = true;
        if let Ok(mut timers) = self.timers.lock() {
            match timers.iter_mut().find(|(w, _)| *w == id) {
                Some((_, when)) => {
                    sooner = at < *when;
                    *when = (*when).min(at);
                }
                None => timers.push((id, at)),
            }
        }
        // Wakes the loop so it picks up the new deadline (one it has already, or a sooner
        // one: nothing to pick up — a frame asks for several).
        if sooner && let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::Timer);
        }
    }

    fn open_window(&self) {
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::OpenWindow);
        }
    }

    fn inner_size(&self) -> PhysicalSize {
        let s = self.window.inner_size();
        PhysicalSize::new(s.width, s.height)
    }

    fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn request_inner_size(&self, size: PhysicalSize) {
        let _ =
            self.window.request_inner_size(winit::dpi::PhysicalSize::new(size.width, size.height));
    }

    fn set_title(&self, title: &str) {
        self.window.set_title(title);
        // The window list (and the Dock's menu) shows the new title.
        #[cfg(target_os = "macos")]
        dock::list_window(&self.window);
    }

    fn handles(&self) -> Arc<dyn WindowHandles> {
        Arc::clone(&self.window) as Arc<dyn WindowHandles>
    }

    fn has_native_menu(&self) -> bool {
        self.native_menu
    }

    fn pick_file(&self, start_dir: Option<&Path>) -> Option<PathBuf> {
        let picked = dialog(start_dir).pick_file();
        self.after_native_dialog();
        picked
    }

    fn pick_folder(&self, start_dir: Option<&Path>) -> Option<PathBuf> {
        let picked = dialog(start_dir).pick_folder();
        self.after_native_dialog();
        picked
    }

    fn pick_save_path(&self, start_dir: Option<&Path>, file_name: &str) -> Option<PathBuf> {
        let picked = dialog(start_dir).set_file_name(file_name).save_file();
        self.after_native_dialog();
        picked
    }

    fn clipboard_files(&self) -> Vec<PathBuf> {
        self.with_clipboard(|c| c.get().file_list().ok()).flatten().unwrap_or_default()
    }

    fn set_clipboard_files(&self, paths: &[PathBuf]) {
        self.with_clipboard(|c| {
            if let Err(e) = c.set().file_list(paths) {
                tracing::warn!("can't put the files on the clipboard: {e}");
            }
        });
    }

    fn clipboard_text(&self) -> Option<String> {
        self.with_clipboard(|c| c.get_text().ok()).flatten()
    }

    fn set_clipboard_text(&self, text: &str) {
        self.with_clipboard(|c| {
            if let Err(e) = c.set_text(text) {
                tracing::warn!("can't write to the clipboard: {e}");
            }
        });
    }

    fn set_clipboard_image(&self, width: usize, height: usize, rgba: &[u8]) {
        self.with_clipboard(|c| {
            let image = arboard::ImageData { width, height, bytes: rgba.into() };
            if let Err(e) = c.set_image(image) {
                tracing::warn!("can't put the image on the clipboard: {e}");
            }
        });
    }

    fn set_ime_cursor_area(&self, area: Rect) {
        let defer = cfg!(target_os = "macos");
        if let Some(area) = ime_caret::lock(&self.ime).moved(area, defer) {
            ime_caret::tell(&self.window, area);
        }
    }

    fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let proxy = self.proxy.lock().map(|p| p.clone()).ok();
        let proxy = Mutex::new(proxy);
        let (id, pending) = (self.window.id(), Arc::clone(&self.wake_pending));
        Arc::new(move || {
            // At most one Wake in flight per window: a burst (a chatty terminal, the
            // indexer) becomes a single poll of this window instead of one per message.
            if pending.swap(true, Ordering::AcqRel) {
                return;
            }
            if let Ok(guard) = proxy.lock()
                && let Some(proxy) = guard.as_ref()
            {
                let _ = proxy.send_event(UserEvent::Wake(Some((id, Arc::clone(&pending)))));
            }
        })
    }

    fn app_waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let proxy = self.proxy.lock().map(|p| p.clone()).ok();
        let proxy = Mutex::new(proxy);
        Arc::new(move || {
            // At most one in flight, as for a window's.
            if WAKE_PENDING.swap(true, Ordering::AcqRel) {
                return;
            }
            if let Ok(guard) = proxy.lock()
                && let Some(proxy) = guard.as_ref()
            {
                let _ = proxy.send_event(UserEvent::Wake(None));
            }
        })
    }

    fn system_appearance(&self) -> Option<SystemAppearance> {
        self.window.theme().map(appearance)
    }

    fn reduce_motion(&self) -> bool {
        motion::reduce_motion()
    }

    fn system_idle(&self) -> Option<std::time::Duration> {
        system_idle::system_idle()
    }

    fn power_saving(&self) -> bool {
        motion::power_saving()
    }

    fn show_context_menu(&self, items: &[MenuItem], position: Point) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return context_menu::show(&self.window, items, position);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (items, position);
            false
        }
    }

    fn drag_window(&self) {
        #[cfg(target_os = "macos")]
        titlebar::start_drag(&self.window);
        #[cfg(not(target_os = "macos"))]
        if let Err(e) = self.window.drag_window() {
            tracing::debug!("the window move didn't start: {e}");
        }
    }

    fn toggle_maximize(&self) {
        self.window.set_maximized(!self.window.is_maximized());
    }

    fn titlebar_inset(&self) -> f32 {
        self.titlebar_inset
    }

    fn window_controls(&self) -> bool {
        #[cfg(target_os = "windows")]
        return self.caption.is_some();
        #[cfg(not(target_os = "windows"))]
        false
    }

    fn minimize(&self) {
        self.window.set_minimized(true);
    }

    fn is_maximized(&self) -> bool {
        self.window.is_maximized()
    }

    fn toggle_fullscreen(&self) {
        let full = self.window.fullscreen().is_none();
        // Its edges don't resize then (Windows: the subclass answers where things are).
        #[cfg(target_os = "windows")]
        if let Some(caption) = &self.caption {
            caption.set_fullscreen(full);
        }
        self.window.set_fullscreen(full.then_some(winit::window::Fullscreen::Borderless(None)));
    }

    fn is_fullscreen(&self) -> bool {
        self.window.fullscreen().is_some()
    }

    fn display_id(&self) -> Option<String> {
        let monitor = self.window.current_monitor()?;
        let at = monitor.position();
        Some(format!("{}@{},{}", monitor.name().unwrap_or_default(), at.x, at.y))
    }

    fn maximize_hovered(&self) -> bool {
        #[cfg(target_os = "windows")]
        return self.caption.as_ref().is_some_and(|c| c.maximize_hovered());
        #[cfg(not(target_os = "windows"))]
        false
    }

    fn set_maximize_button(&self, rect: Option<kernel::Rect>) {
        #[cfg(target_os = "windows")]
        if let Some(caption) = &self.caption {
            caption.set_maximize(rect.map(|r| {
                (r.min_x() as i32, r.min_y() as i32, r.size.width as i32, r.size.height as i32)
            }));
        }
        #[cfg(not(target_os = "windows"))]
        let _ = rect;
    }

    fn open_url(&self, url: &str) {
        // Known schemes only: a link in a document must not launch local programs.
        if !(url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("mailto:"))
        {
            tracing::warn!("URL ignored (unknown scheme): {url}");
            return;
        }
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(url).spawn()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("rundll32")
                .args(["url.dll,FileProtocolHandler", url])
                .spawn()
        } else {
            std::process::Command::new("xdg-open").arg(url).spawn()
        };
        if let Err(e) = result {
            tracing::warn!("can't open {url}: {e}");
        }
    }

    fn web_view_holes(&self) -> bool {
        cfg!(any(target_os = "macos", target_os = "windows"))
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn web_view(
        &self,
        source: platform_api::WebSource<'_>,
        bounds: Rect,
        data: &Path,
        inbox: platform_api::WebInbox,
    ) -> Option<Box<dyn platform_api::WebView>> {
        web_view::WryWebView::new(&self.window, source, bounds, data, inbox)
            .map(|view| Box::new(view) as Box<dyn platform_api::WebView>)
    }

    fn open_in_browser(&self, path: &Path) {
        // A page only: whatever else is opened by its program, which might run it.
        let page = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
        if !page || !path.is_absolute() {
            tracing::warn!("not a page, not opened: {}", path.display());
            return;
        }
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(path).spawn()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("rundll32")
                .arg("url.dll,FileProtocolHandler")
                .arg(path)
                .spawn()
        } else {
            std::process::Command::new("xdg-open").arg(path).spawn()
        };
        if let Err(e) = result {
            tracing::warn!("can't open {}: {e}", path.display());
        }
    }

    fn reveal_in_file_manager(&self, path: &Path) {
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg("-R").arg(path).spawn()
        } else if cfg!(target_os = "windows") {
            reveal_in_explorer(path)
        } else {
            let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
            std::process::Command::new("xdg-open").arg(dir).spawn()
        };
        if let Err(e) = result {
            tracing::warn!("can't open the file manager: {e}");
        }
    }

    fn file_manager(&self) -> FileManager {
        if cfg!(target_os = "macos") {
            FileManager::Finder
        } else if cfg!(target_os = "windows") {
            FileManager::Explorer
        } else {
            FileManager::Other
        }
    }

    fn set_cursor_icon(&self, icon: CursorIcon) {
        self.window.set_cursor(match icon {
            CursorIcon::Default => winit::window::CursorIcon::Default,
            CursorIcon::Text => winit::window::CursorIcon::Text,
            CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
            CursorIcon::ResizeHorizontal => winit::window::CursorIcon::ColResize,
            CursorIcon::ResizeVertical => winit::window::CursorIcon::RowResize,
        });
    }
}

fn dialog(start_dir: Option<&Path>) -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new();
    match start_dir {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
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
        let attributes = app_icon::with_icon(attributes, self.options.icon);
        match event_loop.create_window(attributes) {
            Ok(window) => {
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
                    #[cfg(target_os = "windows")]
                    caption,
                    ime,
                }));
            }
            Err(e) => {
                tracing::error!("can't create the window: {e}");
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
            Err(e) => tracing::warn!("can't create the native menu: {e}"),
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

impl ApplicationHandler<UserEvent> for Handler {
    /// The app ends: its sign leaves the notification area (Windows keeps a dead icon there
    /// until the mouse passes over it otherwise); the app writes what it still has to.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "windows")]
        system_sign_windows::set(None, std::iter::empty());
        self.app.exiting();
    }

    /// Fires the timed redraws that are due and sleeps until the next one (or until an
    /// event, when there is none).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let dragging = self.tell_file_drags(event_loop);
        let now = Instant::now();
        let next = match self.timers.lock() {
            Ok(mut timers) => {
                for (id, _) in timers.iter().filter(|(_, at)| *at <= now) {
                    if let Some(w) = self.windows.iter().find(|w| w.window.id() == *id) {
                        w.window.request_redraw();
                    }
                }
                let windows = &self.windows;
                timers.retain(|(id, at)| *at > now && windows.iter().any(|w| w.window.id() == *id));
                timers.iter().map(|(_, at)| *at).min()
            }
            Err(_) => None,
        };
        // A drag over a window: its pointer asked again soon.
        let next = match (next, dragging.then(|| now + file_drag::POLL)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        self.install_menu();
        #[cfg(target_os = "macos")]
        menu::observe_opening(self.proxy.clone());
        #[cfg(target_os = "macos")]
        {
            let proxy = Mutex::new(self.proxy.clone());
            ime_caret::observe_source(move || {
                if let Ok(proxy) = proxy.lock() {
                    let _ = proxy.send_event(UserEvent::InputSourceChanged);
                }
            });
        }
        self.open_window(event_loop);
        // The Dock's icon once the window is on its way (it doesn't hold the window up).
        #[cfg(target_os = "macos")]
        app_icon::set_dock_icon(self.options.icon);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            // Commands from the native menu: the active window.
            // The sign's row bringing the app to the front: its last window, unminimized.
            UserEvent::Command(id) if id == platform_api::SHOW_APP => {
                if let Some(w) = self.windows.iter().find(|w| Some(w.window.id()) == self.focused) {
                    w.window.set_minimized(false);
                    w.window.focus_window();
                } else if let Some(w) = self.windows.last() {
                    w.window.set_minimized(false);
                    w.window.focus_window();
                }
                #[cfg(target_os = "macos")]
                if let Some(mtm) = objc2::MainThreadMarker::new() {
                    objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
                }
            }
            UserEvent::Command(id) => {
                if let Some(focused) =
                    self.focused.or_else(|| self.windows.first().map(|w| w.window.id()))
                {
                    self.dispatch(event_loop, focused, PlatformEvent::Command(id));
                }
            }
            // A background task finished: its window checks its results.
            UserEvent::Wake(Some((id, pending))) => {
                // Cleared before polling: anything finishing during the poll wakes it again.
                pending.store(false, Ordering::Release);
                if self.windows.iter().any(|w| w.window.id() == id) {
                    self.dispatch(event_loop, id, PlatformEvent::Wake);
                }
            }
            // Something every window shows: each one checks.
            UserEvent::Wake(None) => {
                WAKE_PENDING.store(false, Ordering::Release);
                let ids: Vec<WindowId> = self.windows.iter().map(|w| w.window.id()).collect();
                for id in ids {
                    self.dispatch(event_loop, id, PlatformEvent::Wake);
                }
            }
            UserEvent::OpenWindow => self.open_window(event_loop),
            UserEvent::SetMenus(menus) => {
                self.options.menus = menus;
                self.install_menu();
            }
            UserEvent::SetMenuStates(disabled) => {
                #[cfg(target_os = "macos")]
                {
                    if let Some(menu) = &self.native_menu {
                        menu.set_disabled(&disabled);
                    }
                    self.menu_disabled = disabled;
                }
                #[cfg(not(target_os = "macos"))]
                let _ = disabled;
            }
            UserEvent::InputSourceChanged => {
                if let Some(state) = self.focused.and_then(|id| self.state(id)) {
                    let told = ime_caret::lock(&state.ime).source_changed();
                    if let Some(area) = told {
                        ime_caret::tell(&state.window, area);
                    }
                }
            }
            UserEvent::MenuOpening => {
                if let Some(focused) = self.focused {
                    self.dispatch(event_loop, focused, PlatformEvent::MenuOpening);
                }
            }
            UserEvent::SetIconMenu(menu) => {
                #[cfg(target_os = "macos")]
                dock::set(&menu);
                #[cfg(target_os = "windows")]
                jump_list::set(&menu);
                let _ = menu;
            }
            UserEvent::SetSystemSign(sign) => {
                #[cfg(target_os = "macos")]
                system_sign::set(sign.as_ref());
                #[cfg(target_os = "windows")]
                system_sign_windows::set(sign.as_ref(), self.windows.iter().map(|w| &*w.window));
                let _ = sign;
            }
            UserEvent::Focus(id) => {
                if let Some(w) = self.windows.iter().find(|w| api_id(w.window.id()) == id) {
                    w.window.set_minimized(false);
                    w.window.focus_window();
                }
            }
            UserEvent::ScreenColor(id, color) => {
                self.dispatch(event_loop, id, PlatformEvent::ScreenColor(color));
            }
            // A timed redraw was requested: `about_to_wait` sets the deadline.
            UserEvent::Timer => {}
            UserEvent::ResetModifiers(id) => {
                let Some(state) = self.state(id) else { return };
                if state.modifiers != Modifiers::default() {
                    state.modifiers = Modifiers::default();
                    self.dispatch(
                        event_loop,
                        id,
                        PlatformEvent::ModifiersChanged(Modifiers::default()),
                    );
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.state(id).map(|w| Arc::clone(&w.window)) else { return };
        let translated = match event {
            WindowEvent::Resized(size) => {
                // AppKit repositions the window buttons on every resize.
                self.layout_titlebar(&window);
                PlatformEvent::Resized(PhysicalSize::new(size.width, size.height))
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.layout_titlebar(&window);
                PlatformEvent::ScaleFactorChanged(scale_factor)
            }
            WindowEvent::RedrawRequested => PlatformEvent::RedrawRequested,
            WindowEvent::CloseRequested => PlatformEvent::CloseRequested,
            WindowEvent::Focused(focused) => {
                self.layout_titlebar(&window);
                if focused {
                    self.focused = Some(id);
                }
                PlatformEvent::Focused(focused)
            }
            WindowEvent::Occluded(occluded) => PlatformEvent::Occluded(occluded),
            WindowEvent::ThemeChanged(theme) => {
                PlatformEvent::SystemAppearanceChanged(appearance(theme))
            }
            WindowEvent::ModifiersChanged(m) => {
                let changed = modifiers(m.state());
                let Some(state) = self.state(id) else { return };
                if changed == state.modifiers {
                    return;
                }
                state.modifiers = changed;
                PlatformEvent::ModifiersChanged(changed)
            }
            WindowEvent::KeyboardInput { event, .. } => PlatformEvent::Key(KeyEvent {
                key: key(&event.logical_key),
                pressed: event.state == ElementState::Pressed,
                repeat: event.repeat,
                modifiers: self.state(id).map(|s| s.modifiers).unwrap_or_default(),
                text: event.text.map(|t| t.to_string()),
                numpad: event.location == keyboard::KeyLocation::Numpad,
            }),
            WindowEvent::Ime(ime) => {
                // A composition starting tells the system the caret's place (its candidates
                // shown there), one ending stops following it.
                let composing =
                    matches!(&ime, winit::event::Ime::Preedit(text, _) if !text.is_empty());
                if let Some(state) = self.state(id) {
                    let told = ime_caret::lock(&state.ime).composing(composing);
                    if let Some(area) = told {
                        ime_caret::tell(&state.window, area);
                    }
                }
                match ime {
                    winit::event::Ime::Preedit(text, cursor) => {
                        PlatformEvent::Ime(ImeEvent::Preedit { text, cursor })
                    }
                    winit::event::Ime::Commit(text) => PlatformEvent::Ime(ImeEvent::Commit(text)),
                    winit::event::Ime::Enabled | winit::event::Ime::Disabled => return,
                }
            }
            WindowEvent::PinchGesture { delta, .. } => PlatformEvent::Pinch(delta as f32),
            WindowEvent::MouseWheel { delta, .. } => PlatformEvent::Scroll(match delta {
                MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
                MouseScrollDelta::PixelDelta(p) => {
                    ScrollDelta::Pixels { x: p.x as f32, y: p.y as f32 }
                }
            }),
            WindowEvent::CursorMoved { position, .. } => {
                #[cfg(target_os = "macos")]
                titlebar::end_drag(self.windows.iter().map(|w| &w.window));
                let p = Point::new(position.x as f32, position.y as f32);
                if let Some(state) = self.state(id) {
                    state.cursor = p;
                }
                PlatformEvent::CursorMoved(p)
            }
            WindowEvent::CursorLeft { .. } => PlatformEvent::CursorLeft,
            WindowEvent::MouseInput { state, button, .. } => {
                #[cfg(target_os = "macos")]
                titlebar::end_drag(self.windows.iter().map(|w| &w.window));
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                let (cursor, modifiers) =
                    self.state(id).map(|s| (s.cursor, s.modifiers)).unwrap_or_default();
                match state {
                    ElementState::Pressed => {
                        PlatformEvent::MouseDown { button, position: cursor, modifiers }
                    }
                    ElementState::Released => PlatformEvent::MouseUp { button, position: cursor },
                }
            }
            WindowEvent::HoveredFile(path) => {
                if let Some(state) = self.state(id) {
                    state.drag.hovered(path);
                }
                return;
            }
            WindowEvent::DroppedFile(path) => {
                if let Some(state) = self.state(id) {
                    state.drag.dropped(path);
                }
                return;
            }
            WindowEvent::HoveredFileCancelled => {
                if !self.state(id).is_some_and(|state| state.drag.cancelled()) {
                    return;
                }
                PlatformEvent::FilesHoverCancelled
            }
            _ => return,
        };
        self.dispatch(event_loop, id, translated);
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

fn appearance(theme: winit::window::Theme) -> SystemAppearance {
    match theme {
        winit::window::Theme::Light => SystemAppearance::Light,
        winit::window::Theme::Dark => SystemAppearance::Dark,
    }
}

fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}

fn key(key: &keyboard::Key) -> Key {
    use keyboard::NamedKey as W;
    match key {
        keyboard::Key::Character(c) => Key::Character(c.to_string()),
        keyboard::Key::Named(named) => Key::Named(match named {
            W::ArrowUp => NamedKey::ArrowUp,
            W::ArrowDown => NamedKey::ArrowDown,
            W::ArrowLeft => NamedKey::ArrowLeft,
            W::ArrowRight => NamedKey::ArrowRight,
            W::PageUp => NamedKey::PageUp,
            W::PageDown => NamedKey::PageDown,
            W::Home => NamedKey::Home,
            W::End => NamedKey::End,
            W::Enter => NamedKey::Enter,
            W::Escape => NamedKey::Escape,
            W::Tab => NamedKey::Tab,
            W::Backspace => NamedKey::Backspace,
            W::Delete => NamedKey::Delete,
            W::Insert => NamedKey::Insert,
            W::Space => NamedKey::Space,
            W::F1 => NamedKey::F(1),
            W::F2 => NamedKey::F(2),
            W::F3 => NamedKey::F(3),
            W::F4 => NamedKey::F(4),
            W::F5 => NamedKey::F(5),
            W::F6 => NamedKey::F(6),
            W::F7 => NamedKey::F(7),
            W::F8 => NamedKey::F(8),
            W::F9 => NamedKey::F(9),
            W::F10 => NamedKey::F(10),
            W::F11 => NamedKey::F(11),
            W::F12 => NamedKey::F(12),
            _ => return Key::Unidentified,
        }),
        _ => Key::Unidentified,
    }
}

/// Explorer with `path` selected. Its `/select,` takes the path quoted inside the one
/// argument, as written (`/select,"C:\My Projects\a.php"`): Rust's own quoting of the whole
/// argument opens the user's documents instead when the path has spaces.
#[cfg(windows)]
fn reveal_in_explorer(path: &Path) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
}

#[cfg(not(windows))]
fn reveal_in_explorer(path: &Path) -> std::io::Result<std::process::Child> {
    std::process::Command::new("explorer").arg(path).spawn()
}
