//! Zoom in the editor, images and the canvas.

use super::*;

impl Workbench {
    /// The zoom shown in the status bar: the active viewer's (image, canvas) or the
    /// editor's.
    pub fn zoom_level(&self) -> f32 {
        self.tabs
            .get(self.active)
            .and_then(|t| t.viewer.as_ref().filter(|v| !v.editor_visible()))
            .and_then(|v| v.viewer.zoom_level())
            .unwrap_or(self.editor_zoom)
    }

    /// Zooms by `factor` around point `at` (pinch gesture, ⌘ + scroll, ±): on the viewer
    /// under it or on the editor's text.
    pub fn zoom_by(&mut self, factor: f32, at: Point) -> bool {
        // A window over everything: its view's zoom (a diagram's), else nothing behind it.
        if self.modal_open() {
            return self.zoom_modal_view(factor, at).unwrap_or(false);
        }
        if let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && v.viewer_visible()
            && (v.bounds.contains(at) || !v.editor_visible())
            && v.viewer.zoom_level().is_some()
        {
            return v.viewer.zoom(factor, at);
        }
        self.set_editor_zoom(self.editor_zoom * factor)
    }

    /// A zoom step (`+` / `−` from the status bar or the menu).
    pub fn zoom_step(&mut self, zoom_in: bool) -> bool {
        if self.modal_open() {
            let factor = if zoom_in { 1.25 } else { 0.8 };
            return self.zoom_modal_view(factor, Point::new(f32::MIN, f32::MIN)).unwrap_or(false);
        }
        let at = Point::new(
            self.editor_area.min_x() + self.editor_area.size.width / 2.0,
            self.editor_area.min_y() + self.editor_area.size.height / 2.0,
        );
        let viewer = self
            .tabs
            .get(self.active)
            .and_then(|t| t.viewer.as_ref())
            .is_some_and(|v| !v.editor_visible() && v.viewer.zoom_level().is_some());
        if viewer {
            return self.zoom_by(if zoom_in { 1.25 } else { 0.8 }, at);
        }
        // The editor: 10% steps, rounded.
        let step = if zoom_in { 0.1 } else { -0.1 };
        self.set_editor_zoom(((self.editor_zoom + step) * 10.0).round() / 10.0)
    }

    /// Back to 100%.
    pub fn zoom_reset(&mut self) -> bool {
        if self.modal_open() {
            return self.reset_modal_zoom().unwrap_or(false);
        }
        if let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && !v.editor_visible()
            && v.viewer.zoom_level().is_some()
        {
            return v.viewer.reset_zoom();
        }
        self.set_editor_zoom(1.0)
    }

    pub(super) fn set_editor_zoom(&mut self, zoom: f32) -> bool {
        let zoom = zoom.clamp(0.5, 3.0);
        if (zoom - self.editor_zoom).abs() < 0.001 {
            return false;
        }
        self.editor_zoom = zoom;
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            tab.view.set_zoom(zoom);
        }
        true
    }

    /// The modifiers changed: ⌘⌥ (Ctrl+Alt) makes the wheel zoom.
    pub fn set_wheel_zoom(&mut self, on: bool) {
        self.wheel_zoom = on;
    }
}
