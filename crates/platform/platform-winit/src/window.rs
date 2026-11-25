//! A window as the app sees it ([`Window`]): its size and redraws, menus, dialogs,
//! clipboard, wakers, the system's settings, the title bar's buttons, links, the file
//! manager and the cursor.

use super::events::appearance;
use super::*;

impl Window for WinitWindow {
    fn id(&self) -> platform_api::WindowId {
        platform_api::WindowId(u64::from(self.window.id()))
    }

    fn access_asked(&self) -> Option<u64> {
        Some(self.access_asked.load(std::sync::atomic::Ordering::Relaxed)).filter(|&n| n > 0)
    }

    fn update_access(
        &self,
        frame: &platform_api::access::Frame,
        previous: Option<&platform_api::access::Frame>,
    ) {
        let ask = self.access_asked.load(std::sync::atomic::Ordering::Relaxed);
        let update = access_bridge::tree_update(frame, &self.window.title(), previous);
        if let Ok(proxy) = self.proxy.lock() {
            let _ = proxy.send_event(UserEvent::AccessTree(self.window.id(), ask, update));
        }
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
                tracing::warn!(error = %e, "can't put the files on the clipboard");
            }
        });
    }

    fn clipboard_text(&self) -> Option<String> {
        self.with_clipboard(|c| c.get_text().ok()).flatten()
    }

    fn set_clipboard_text(&self, text: &str) {
        self.with_clipboard(|c| {
            if let Err(e) = c.set_text(text) {
                tracing::warn!(error = %e, "can't write to the clipboard");
            }
        });
    }

    fn set_clipboard_image(&self, width: usize, height: usize, rgba: &[u8]) {
        self.with_clipboard(|c| {
            let image = arboard::ImageData { width, height, bytes: rgba.into() };
            if let Err(e) = c.set_image(image) {
                tracing::warn!(error = %e, "can't put the image on the clipboard");
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
            tracing::debug!(error = %e, "the window move didn't start");
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
            tracing::warn!(%url, "URL ignored (unknown scheme)");
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
            tracing::warn!(%url, error = %e, "can't open");
        }
    }

    fn web_view_holes(&self) -> bool {
        cfg!(any(target_os = "macos", target_os = "windows"))
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn floating_layer(&self) -> Option<Box<dyn platform_api::FloatingLayer>> {
        // `EDITOR_NO_FLOATING_LAYER=1`: what floats over the pages goes in holes cut out of
        // them, as before.
        if std::env::var_os("EDITOR_NO_FLOATING_LAYER").is_some() {
            return None;
        }
        floating::Floating::new(&self.window)
            .map(|layer| Box::new(layer) as Box<dyn platform_api::FloatingLayer>)
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
            tracing::warn!(path = %path.display(), "not a page, not opened");
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
            tracing::warn!(path = %path.display(), error = %e, "can't open");
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
            tracing::warn!(error = %e, "can't open the file manager");
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
            CursorIcon::Crosshair => winit::window::CursorIcon::Crosshair,
            CursorIcon::Move => winit::window::CursorIcon::Move,
            CursorIcon::Grab => winit::window::CursorIcon::Grab,
            CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
            CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
            CursorIcon::Wait => winit::window::CursorIcon::Wait,
            CursorIcon::Progress => winit::window::CursorIcon::Progress,
            CursorIcon::Help => winit::window::CursorIcon::Help,
            CursorIcon::ResizeDiagonal => winit::window::CursorIcon::NwseResize,
            CursorIcon::ResizeAntiDiagonal => winit::window::CursorIcon::NeswResize,
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
