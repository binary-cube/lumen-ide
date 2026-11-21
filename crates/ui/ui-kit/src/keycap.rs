//! A key cap: a small tile with a key's label centered on it (`esc`, `⌘K`, `↵`). One drawing
//! for every key the app shows — a footer's hints, a key inside a button, a key in a
//! field's corner — each with its look ([`KeycapLook`]): its fill, an outline, a relief line
//! under it, its corners, its label's color.

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::ShapedLine;

use crate::draw_text;
use crate::key_hints::KeyHintsStyle;

/// A cap's outline (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeycapBorder {
    pub width: f32,
    pub color: Rgba,
}

/// The line under a cap that gives it relief: `inset` from both sides, `thickness` high
/// (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeycapRelief {
    pub color: Rgba,
    pub inset: f32,
    pub thickness: f32,
}

/// How a cap looks (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeycapLook {
    pub background: Rgba,
    pub border: Option<KeycapBorder>,
    pub relief: Option<KeycapRelief>,
    pub radius: f32,
    pub text: Rgba,
}

/// A hint's cap: its corners and the relief's inset (logical pixels).
const HINT_RADIUS: f32 = 4.0;
const HINT_RELIEF_INSET: f32 = 2.0;

impl KeycapLook {
    /// A flat tile: a fill and its label, no outline nor relief.
    pub fn flat(background: Rgba, text: Rgba, radius: f32) -> Self {
        Self { background, border: None, relief: None, radius, text }
    }

    /// A footer's hint: outlined, a relief line under it in the outline's color.
    pub fn hint(style: &KeyHintsStyle) -> Self {
        let px = |v: f32| (v * style.scale).round();
        let line = style.scale.round().max(1.0);
        Self {
            background: style.key_background,
            border: Some(KeycapBorder { width: line, color: style.key_border }),
            relief: Some(KeycapRelief {
                color: style.key_border,
                inset: px(HINT_RELIEF_INSET),
                thickness: line,
            }),
            radius: px(HINT_RADIUS),
            text: style.key_text,
        }
    }
}

/// Where a label `label_width` wide starts to be centered across `width` from `left` (on a
/// whole pixel). A cap's owner gives the width it centers on: the tile's, or the one it
/// measured before rounding the tile.
pub fn keycap_label_x(left: f32, width: f32, label_width: f32) -> f32 {
    (left + (width - label_width) / 2.0).round()
}

/// Draws a cap in `cap` with its `label` from `(x, baseline)` (see [`keycap_label_x`]).
pub fn paint_keycap(
    layer: &mut Layer,
    cap: Rect,
    label: &ShapedLine,
    (x, baseline): (f32, f32),
    look: &KeycapLook,
) {
    let tile = Quad::filled(cap, look.background).rounded(look.radius);
    let tile = match look.border {
        Some(border) => tile.bordered(border.width, border.color),
        None => tile,
    };
    layer.quad(tile);
    if let Some(relief) = look.relief {
        layer.quad(Quad::filled(
            Rect::new(
                cap.min_x() + relief.inset,
                cap.max_y() - relief.thickness,
                cap.size.width - 2.0 * relief.inset,
                relief.thickness,
            ),
            relief.color,
        ));
    }
    draw_text(layer, label, x, baseline, look.text);
}

#[cfg(test)]
#[path = "../tests/unit/keycap.rs"]
mod tests;
