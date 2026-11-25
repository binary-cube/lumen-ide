//! 2D geometry, shared by the platform, rendering and the UI.
//!
//! The unit (logical or physical pixels) is set by the contract that uses the type;
//! `Scene` in render-api, for example, works in physical pixels.

/// 2D point.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Moved by `by` (a place inside something, in what holds it).
    pub fn offset(self, by: Point) -> Point {
        Point::new(self.x + by.x, self.y + by.y)
    }

    /// Where it is from `origin` (a place in what holds something, inside it).
    pub fn relative_to(self, origin: Point) -> Point {
        Point::new(self.x - origin.x, self.y - origin.y)
    }

    /// Both coordinates times `factor` (from one unit to another).
    pub fn scaled(self, factor: f32) -> Point {
        Point::new(self.x * factor, self.y * factor)
    }
}

/// 2D size.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// The size of a surface in physical pixels (e.g. the window).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

impl PhysicalSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn to_size(self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }
}

/// Axis-aligned rectangle.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { origin: Point::new(x, y), size: Size::new(width, height) }
    }

    pub fn from_size(size: Size) -> Self {
        Self { origin: Point::default(), size }
    }

    pub fn min_x(&self) -> f32 {
        self.origin.x
    }

    pub fn min_y(&self) -> f32 {
        self.origin.y
    }

    pub fn max_x(&self) -> f32 {
        self.origin.x + self.size.width
    }

    pub fn max_y(&self) -> f32 {
        self.origin.y + self.size.height
    }

    /// Its middle.
    pub fn center(&self) -> Point {
        Point::new(self.origin.x + self.size.width / 2.0, self.origin.y + self.size.height / 2.0)
    }

    pub fn is_empty(&self) -> bool {
        self.size.width <= 0.0 || self.size.height <= 0.0
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min_x() && p.x < self.max_x() && p.y >= self.min_y() && p.y < self.max_y()
    }

    /// The intersection of two rectangles; `None` if they don't overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.min_x().max(other.min_x());
        let y0 = self.min_y().max(other.min_y());
        let x1 = self.max_x().min(other.max_x());
        let y1 = self.max_y().min(other.max_y());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// Moved by `by` (a rectangle inside something, in what holds it).
    pub fn offset(self, by: Point) -> Rect {
        Rect { origin: self.origin.offset(by), size: self.size }
    }

    /// Its place and size times `factor` (from one unit to another).
    pub fn scaled(self, factor: f32) -> Rect {
        let size = Size::new(self.size.width * factor, self.size.height * factor);
        Rect { origin: self.origin.scaled(factor), size }
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Rect) -> Rect {
        let x0 = self.min_x().min(other.min_x());
        let y0 = self.min_y().min(other.min_y());
        let x1 = self.max_x().max(other.max_x());
        let y1 = self.max_y().max(other.max_y());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// A rectangle's corners as bits (a card's rounded ones, a page's): one definition for every
/// part that says which corners it rounds.
#[derive(Debug, Clone, Copy)]
pub struct Corners;

impl Corners {
    pub const TOP_LEFT: u8 = 1;
    pub const TOP_RIGHT: u8 = 2;
    pub const BOTTOM_LEFT: u8 = 4;
    pub const BOTTOM_RIGHT: u8 = 8;
}

#[cfg(test)]
#[path = "../tests/unit/geometry.rs"]
mod tests;
