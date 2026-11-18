//! The resizable separator between two panels: vertical (side-by-side panels, e.g.
//! the tree and the editor) or horizontal (stacked panels, e.g. the editor and the terminal).
//!
//! A subtle line that fades toward the ends, with a rounded handle (three dots)
//! in the middle. On hover and while dragging, the accent "grows" from the middle toward the
//! ends (ease-out, ~220 ms), and the handle smoothly turns the accent color; on exit, reversed.
//!
//! Its handle is wider than the strip between the panels, so it reaches over both: a user
//! paints it **after** both panes ([`Splitter::paint`] last), never before a sibling pane
//! that would cover half of it; [`Splitter::is_on_top`] checks a painted scene.
//!
//! The appearance comes from [`SplitterStyle`]: built from the theme
//! ([`SplitterStyle::from_palette`]) or given individually by the component's user.

use std::time::{Duration, Instant};

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};

use crate::palette::Palette;

/// The direction of the separator line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Vertical line: separates left / right panels, dragged horizontally.
    #[default]
    Vertical,
    /// Horizontal line: separates top / bottom panels, dragged vertically.
    Horizontal,
}

/// The separator's appearance. Dimensions are in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitterStyle {
    pub line: Rgba,
    pub handle_background: Rgba,
    pub handle_border: Rgba,
    pub grip: Rgba,
    /// The line, the handle border and the dots on hover / drag.
    pub active: Rgba,
    pub line_width: f32,
    /// The handle's thickness (perpendicular to the line) and its length (along the line).
    pub handle_thickness: f32,
    pub handle_length: f32,
}

impl SplitterStyle {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            line: p.splitter_line,
            handle_background: p.splitter_handle_background,
            handle_border: p.splitter_handle_border,
            grip: p.splitter_grip,
            active: p.splitter_active,
            line_width: 1.0,
            handle_thickness: 10.0,
            handle_length: 34.0,
        }
    }
}

impl Default for SplitterStyle {
    fn default() -> Self {
        Self::from_palette(&Palette::dark())
    }
}

/// The hover transition duration.
const TRANSITION: Duration = Duration::from_millis(220);

/// The transition between rest (0) and active (1).
#[derive(Debug, Clone, Copy)]
struct Transition {
    from: f32,
    to: f32,
    start: Instant,
}

impl Transition {
    fn value(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / TRANSITION.as_secs_f32())
        .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }

    fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= TRANSITION
    }
}

#[derive(Debug, Default)]
pub struct Splitter {
    style: SplitterStyle,
    orientation: Orientation,
    /// The grab area (the whole strip between the panels), physical pixels.
    bounds: Rect,
    scale: f32,
    hovered: bool,
    /// Distance from the cursor to the line's center at the moment of grabbing.
    drag: Option<f32>,
    transition: Option<Transition>,
}

impl Splitter {
    pub fn new(style: SplitterStyle, orientation: Orientation) -> Self {
        Self { style, orientation, ..Self::default() }
    }

    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    pub fn set_style(&mut self, style: SplitterStyle) {
        self.style = style;
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn contains(&self, p: Point) -> bool {
        self.bounds.size.width > 0.0 && self.bounds.size.height > 0.0 && self.bounds.contains(p)
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The coordinate along which it is dragged (x for vertical, y for horizontal).
    fn axis(&self, p: Point) -> f32 {
        match self.orientation {
            Orientation::Vertical => p.x,
            Orientation::Horizontal => p.y,
        }
    }

    /// The position of the line's center on the drag axis.
    fn center(&self) -> f32 {
        let b = self.bounds;
        match self.orientation {
            Orientation::Vertical => b.min_x() + b.size.width / 2.0,
            Orientation::Horizontal => b.min_y() + b.size.height / 2.0,
        }
    }

    /// Updates the hover state. `true` if a redraw is needed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hovered = self.contains(p);
        let changed = hovered != self.hovered;
        if changed {
            let was = self.is_active();
            self.hovered = hovered;
            self.retarget(was);
        }
        changed
    }

    /// The mouse is over it.
    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    fn is_active(&self) -> bool {
        self.hovered || self.drag.is_some()
    }

    /// How "lit" the separator is now (0 = rest, 1 = active).
    fn activity(&self, now: Instant) -> f32 {
        match self.transition {
            Some(t) => t.value(now),
            None => f32::from(u8::from(self.is_active())),
        }
    }

    /// Starts the transition toward the current state, from where it was (`was` =
    /// the state before the change, if no transition was in progress).
    fn retarget(&mut self, was: bool) {
        let now = Instant::now();
        let from = match self.transition {
            Some(t) => t.value(now),
            None => f32::from(u8::from(was)),
        };
        let to = f32::from(u8::from(self.is_active()));
        self.transition = (from != to).then_some(Transition { from, to, start: now });
    }

    /// The hover transition is in progress: the painter must request another frame.
    pub fn is_animating(&self) -> bool {
        self.transition.is_some_and(|t| !t.done(Instant::now()))
    }

    /// Starts dragging if `p` is on the separator.
    pub fn begin_drag(&mut self, p: Point) -> bool {
        if !self.contains(p) {
            return false;
        }
        let was = self.is_active();
        self.drag = Some(self.axis(p) - self.center());
        self.retarget(was);
        true
    }

    /// The new position of the line's center (physical pixels, on the drag axis) for the cursor
    /// at `p`. The separator's user turns it into the panel size.
    pub fn drag_to(&self, p: Point) -> Option<f32> {
        self.drag.map(|offset| self.axis(p) - offset)
    }

    /// Ends dragging. `true` if it was in progress (a redraw is needed).
    pub fn end_drag(&mut self) -> bool {
        let before = self.is_active();
        let was = self.drag.take().is_some();
        if was {
            self.retarget(before);
        }
        was
    }

    /// The handle's pill (physical pixels), centered on the line; empty when hidden.
    pub fn handle(&self) -> Rect {
        let b = self.bounds;
        if b.size.width <= 0.0 || b.size.height <= 0.0 {
            return Rect::default();
        }
        let px = |v: f32| (v * self.scale).round();
        let (thick, long) = (px(self.style.handle_thickness), px(self.style.handle_length));
        let center = self.center();
        let cross = (center - thick / 2.0).round();
        if self.orientation == Orientation::Vertical {
            let mid = (b.min_y() + (b.size.height - long) / 2.0).round();
            Rect::new(cross, mid, thick, long)
        } else {
            let mid = (b.min_x() + (b.size.width - long) / 2.0).round();
            Rect::new(mid, cross, long, thick)
        }
    }

    /// Whether the handle painted in `scene` is drawn whole: found, and nothing painted in a
    /// later layer (a pane's background, an image, a glyph's pen position) reaches over it. What its
    /// users' tests check, so no pane is ever painted after the handle it borders.
    pub fn is_on_top(&self, scene: &Scene) -> bool {
        let handle = self.handle();
        if handle.size.width <= 0.0 {
            return false;
        }
        let layers = scene.layers();
        let Some(own) = layers.iter().rposition(|l| l.quads.iter().any(|q| q.bounds == handle))
        else {
            return false;
        };
        let reaches = |l: &Layer, r: Rect| {
            let r = match l.clip {
                Some(clip) => r.intersect(&clip),
                None => Some(r),
            };
            r.and_then(|r| r.intersect(&handle))
                .is_some_and(|r| r.size.width > 0.0 && r.size.height > 0.0)
        };
        layers[own + 1..].iter().all(|l| {
            !l.quads.iter().any(|q| q.color.a > 0.0 && reaches(l, q.bounds))
                && !l.images.iter().any(|i| reaches(l, i.bounds))
                // A glyph by its pen position (its box isn't in the scene).
                && !l.glyphs.iter().any(|g| reaches(l, Rect::new(g.origin.x, g.origin.y, 1.0, 1.0)))
        })
    }

    /// Paints the line and its handle in a layer of their own: call it after both panes.
    pub fn paint(&self, scene: &mut Scene) {
        let b = self.bounds;
        if b.size.width <= 0.0 || b.size.height <= 0.0 {
            return;
        }
        let px = |v: f32| (v * self.scale).round();
        let st = &self.style;
        let progress = self.activity(Instant::now());
        let vertical = self.orientation == Orientation::Vertical;
        let thickness = px(st.line_width).max(1.0);
        let center = self.center();
        // The rect of a portion [from, to] along the line.
        let along = |from: f32, to: f32, cross: f32, width: f32| {
            if vertical {
                Rect::new(cross, from, width, to - from)
            } else {
                Rect::new(from, cross, to - from, width)
            }
        };
        let (start, length) =
            if vertical { (b.min_y(), b.size.height) } else { (b.min_x(), b.size.width) };
        // Clipped to its strip along the line only: across it, the handle (wider than a
        // thin strip) shows whole.
        let room = px(16.0);
        let clip = if vertical {
            Rect::new(b.min_x() - room, b.min_y(), b.size.width + 2.0 * room, b.size.height)
        } else {
            Rect::new(b.min_x(), b.min_y() - room, b.size.width, b.size.height + 2.0 * room)
        };
        let layer = scene.push_layer(Some(clip));

        // The subtle line, faded toward the ends; over it, the accent that grows from the middle
        // toward the ends as the separator lights up.
        let cross = (center - thickness / 2.0).round();
        let rect = |from: f32, to: f32| along(from, to, cross, thickness);
        fade_line(layer, start, length, st.line, 1.0, rect);
        if progress > 0.0 {
            fade_line(layer, start, length, st.active, progress, rect);
        }

        // The handle: a pill with three dots, centered on the line.
        let (thick, long) = (px(st.handle_thickness), px(st.handle_length));
        let mid = (start + (length - long) / 2.0).round();
        let handle = self.handle();
        let border = st.handle_border.mix(st.active, progress);
        layer.quad(
            Quad::filled(handle, st.handle_background)
                .rounded(thick / 2.0)
                .bordered(thickness, border),
        );
        let dot = px(2.5).max(2.0);
        let gap = px(4.0);
        let grip = st.grip.mix(st.active, progress);
        let first = mid + (long - (3.0 * dot + 2.0 * gap)) / 2.0;
        for i in 0..3 {
            let from = (first + i as f32 * (dot + gap)).round();
            let r = along(from, from + dot, (center - dot / 2.0).round(), dot);
            layer.quad(Quad::filled(r, grip).rounded(dot / 2.0));
        }
    }
}

/// Linear interpolation between two colors.
/// A line that fades toward the ends (gradient approximated by segments): full opacity
/// in the middle, nearly transparent at the edges. `reach` (0–1) is how far it has spread
/// from the middle: segments beyond it are not drawn, and the edge is soft.
fn fade_line(
    layer: &mut Layer,
    start: f32,
    length: f32,
    color: Rgba,
    reach: f32,
    rect: impl Fn(f32, f32) -> Rect,
) {
    const SEGMENTS: usize = 48;
    let step = length / SEGMENTS as f32;
    for i in 0..SEGMENTS {
        let t = (i as f32 + 0.5) / SEGMENTS as f32;
        let d = ((t - 0.5).abs() * 2.0).min(1.0);
        let front = ((reach * 1.15 - d) / 0.15).clamp(0.0, 1.0);
        if front <= 0.0 {
            continue;
        }
        let alpha = (1.0 - d * d).max(0.06) * front;
        let from = (start + i as f32 * step).round();
        let to = (start + (i + 1) as f32 * step).round();
        if to > from {
            layer.quad(Quad::filled(rect(from, to), color.with_alpha(color.a * alpha)));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/splitter.rs"]
mod tests;
