//! Custom UI kit: element tree, layout, focus, input, virtualized lists.
//!
//! For now: drawing helpers for text and the single-line text field.

pub mod access;
pub mod caret;
mod filter;
mod key_hints;
mod keycap;
mod text_input;

pub use filter::{ListFilter, fold};
pub use key_hints::{KeyHint, KeyHintsStyle, fitting_key_hints, key_hints_width, paint_key_hints};
pub use keycap::{KeycapBorder, KeycapLook, KeycapRelief, keycap_label_x, paint_keycap};
pub use text_input::{TextInput, TextInputStyle};

use icons::{IconId, Icons};
use kernel::{Point, Rgba};
use render_api::{GlyphInstance, Layer};
use text_layout::{FontMetrics, ShapedLine};

/// Adds the glyphs of a shaped line to the layer, starting at `x` with the baseline at
/// `baseline`.
pub fn draw_text(layer: &mut Layer, line: &ShapedLine, x: f32, baseline: f32, color: Rgba) {
    access::text(line, x, baseline);
    layer.glyphs.extend(line.glyphs.iter().map(|g| GlyphInstance {
        key: g.key,
        origin: Point::new(x + g.x, baseline + g.y),
        color,
    }));
}

/// Draws `line` keeping only the glyphs that fit between `left` and `right` (a field's
/// inside: what's scrolled out isn't drawn over its border).
pub fn draw_text_within(
    layer: &mut Layer,
    line: &ShapedLine,
    x: f32,
    baseline: f32,
    color: Rgba,
    (left, right): (f32, f32),
) {
    layer.glyphs.extend(
        line.glyphs
            .iter()
            .filter(|g| x + g.x >= left - 0.5 && x + g.x + g.advance <= right + 0.5)
            .map(|g| GlyphInstance {
                key: g.key,
                origin: Point::new(x + g.x, baseline + g.y),
                color,
            }),
    );
}

/// Draws icon `id` in the square `rect` (side = height), colored with `color`.
pub fn draw_icon(layer: &mut Layer, icons: &Icons, id: IconId, rect: kernel::Rect, color: Rgba) {
    let size = rect.size.height.round().max(1.0);
    let x = (rect.min_x() + (rect.size.width - size) / 2.0).round();
    let y = rect.min_y().round();
    layer.glyphs.push(GlyphInstance {
        key: icons.key(id, size as u16),
        origin: Point::new(x, y + size),
        color,
    });
}

/// The index after moving `delta` rows in a list of `len` items: a single step wraps from
/// the end to the start (and back), a bigger jump (a page) stops at the ends.
pub fn step_index(current: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as isize + delta;
    let last = len as isize - 1;
    (if delta.abs() == 1 { next.rem_euclid(last + 1) } else { next.clamp(0, last) }) as usize
}

/// The baseline that vertically centers text in a row starting at `top`.
pub fn centered_baseline(top: f32, height: f32, metrics: &FontMetrics) -> f32 {
    (top + (height - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent).round()
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
