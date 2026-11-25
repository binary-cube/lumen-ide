use super::*;

#[test]
fn primary_modifier_matches_platform_convention() {
    let cmd = Modifiers { meta: true, ..Modifiers::default() };
    let ctrl = Modifiers { ctrl: true, ..Modifiers::default() };
    if cfg!(target_os = "macos") {
        assert!(cmd.primary() && !ctrl.primary());
    } else {
        assert!(ctrl.primary() && !cmd.primary());
    }
}

/// A window that has only what every window must have: the defaults answer the rest.
#[derive(Debug)]
struct Bare;

impl Window for Bare {
    fn id(&self) -> WindowId {
        WindowId(1)
    }
    fn open_window(&self) {}
    fn inner_size(&self) -> PhysicalSize {
        PhysicalSize::new(800, 600)
    }
    fn scale_factor(&self) -> f64 {
        1.0
    }
    fn request_redraw(&self) {}
    fn request_redraw_at(&self, _: std::time::Instant) {}
    fn request_inner_size(&self, _: PhysicalSize) {}
    fn set_title(&self, _: &str) {}
    fn handles(&self) -> Arc<dyn WindowHandles> {
        unreachable!("no system window behind it")
    }
    fn has_native_menu(&self) -> bool {
        false
    }
    fn pick_file(&self, _: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn pick_folder(&self, _: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn pick_save_path(&self, _: Option<&Path>, _: &str) -> Option<PathBuf> {
        None
    }
    fn clipboard_text(&self) -> Option<String> {
        None
    }
    fn set_clipboard_text(&self, _: &str) {}
    fn set_ime_cursor_area(&self, _: Rect) {}
    fn set_cursor_icon(&self, _: CursorIcon) {}
    fn show_context_menu(&self, _: &[MenuItem], _: Point) -> bool {
        false
    }
    fn drag_window(&self) {}
    fn toggle_maximize(&self) {}
    fn titlebar_inset(&self) -> f32 {
        0.0
    }
    fn reveal_in_file_manager(&self, _: &Path) {}
    fn open_url(&self, _: &str) {}
    fn file_manager(&self) -> FileManager {
        FileManager::Other
    }
    fn system_appearance(&self) -> Option<SystemAppearance> {
        None
    }
    fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(|| {})
    }
}

/// A window whose system can't keep it on top: asked to, nothing happens (and nothing
/// breaks), either way.
#[test]
fn keeping_on_top_does_nothing_by_default() {
    let window = Bare;
    window.set_always_on_top(true);
    window.set_always_on_top(false);
    assert!(!window.can_keep_on_top(), "said so: nothing offers it");
    assert!(!window.is_maximized() && !window.is_fullscreen(), "the window as it was");
}
