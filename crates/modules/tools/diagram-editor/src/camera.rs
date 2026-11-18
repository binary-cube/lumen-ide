//! Where the diagram is on the screen: its zoom (1 = 100%) and where its point (0, 0) falls,
//! relative to the view's corner. Zoomed around the point under the mouse (it stays under
//! it), fitted to the view when a diagram opens.

use kernel::{Point, Rect};

/// The zoom's limits.
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 8.0;

/// The view on the diagram.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    /// Where diagram (0, 0) is, from the view's corner (physical pixels).
    pub offset: Point,
}

impl Default for Camera {
    fn default() -> Self {
        Self { zoom: 1.0, offset: Point::new(0.0, 0.0) }
    }
}

impl Camera {
    /// Physical pixels per diagram unit.
    pub fn k(&self, scale: f32) -> f32 {
        self.zoom * scale
    }

    /// Diagram point `p` on the screen of a view at `view`.
    pub fn to_screen(self, p: Point, view: Rect, scale: f32) -> Point {
        let k = self.k(scale);
        Point::new(view.min_x() + self.offset.x + p.x * k, view.min_y() + self.offset.y + p.y * k)
    }

    /// Screen point `p` in the diagram.
    pub fn to_diagram(self, p: Point, view: Rect, scale: f32) -> Point {
        let k = self.k(scale);
        Point::new(
            (p.x - view.min_x() - self.offset.x) / k,
            (p.y - view.min_y() - self.offset.y) / k,
        )
    }

    /// Moved by (`dx`, `dy`) physical pixels.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.offset.x += dx;
        self.offset.y += dy;
    }

    /// Zoomed by `factor` (within the limits), the diagram point under `at` staying there.
    /// `false`: at its limit already.
    pub fn zoom_at(&mut self, factor: f32, at: Point, view: Rect, scale: f32) -> bool {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (zoom - self.zoom).abs() < 1e-6 {
            return false;
        }
        let fixed = self.to_diagram(at, view, scale);
        self.zoom = zoom;
        let now = self.to_screen(fixed, view, scale);
        self.pan(at.x - now.x, at.y - now.y);
        true
    }

    /// `content` (diagram units) in the middle of `view`, as large as fits but never past
    /// 100%.
    pub fn fit(&mut self, content: Rect, view: Rect, scale: f32) {
        let (w, h) = (content.size.width.max(1.0), content.size.height.max(1.0));
        let room = (view.size.width / scale / w).min(view.size.height / scale / h);
        self.zoom = room.clamp(MIN_ZOOM, 1.0);
        let k = self.k(scale);
        self.offset = Point::new(
            (view.size.width - w * k) / 2.0 - content.min_x() * k,
            (view.size.height - h * k) / 2.0 - content.min_y() * k,
        );
    }
}

#[cfg(test)]
#[path = "../tests/unit/camera.rs"]
mod tests;
