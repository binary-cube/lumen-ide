//! Key hints in a popup footer: `↑` `↓` select · `↵` open · `esc` close.
//! Each key is drawn as a small "keycap", followed by the action.

use kernel::{Rect, Rgba};
use render_api::Layer;
use text_layout::{Font, TextSystem};

use crate::keycap::{KeycapLook, keycap_label_x, paint_keycap};
use crate::{centered_baseline, draw_text};

#[derive(Debug, Clone)]
pub struct KeyHintsStyle {
    /// Font for the actions.
    pub font: Font,
    /// Font for the keys (usually smaller).
    pub key_font: Font,
    pub text: Rgba,
    pub key_text: Rgba,
    pub key_background: Rgba,
    pub key_border: Rgba,
    pub scale: f32,
}

/// One hint: the keys and what they do.
pub type KeyHint<'a> = (&'a [&'a str], &'a str);

/// Width of the hints (same as in [`paint_key_hints`]).
pub fn key_hints_width(text: &mut TextSystem, hints: &[KeyHint<'_>], style: &KeyHintsStyle) -> f32 {
    let px = |v: f32| (v * style.scale).round();
    let cap_h = px(18.0);
    let mut x = 0.0;
    for (i, (keys, action)) in hints.iter().enumerate() {
        if i > 0 {
            x += px(14.0);
        }
        for key in *keys {
            let w = (text.shape_line(key, &style.key_font).width + px(10.0)).max(cap_h);
            x += w.round() + px(4.0);
        }
        x += px(2.0) + text.shape_line(action, &style.font).width;
    }
    x
}

/// The leading hints that fit in `room` (the first one always, the rest dropped from the end):
/// a narrow footer keeps what matters most, never a hint cut in half.
pub fn fitting_key_hints<'h, 'a>(
    text: &mut TextSystem,
    hints: &'h [KeyHint<'a>],
    style: &KeyHintsStyle,
    room: f32,
) -> &'h [KeyHint<'a>] {
    let mut fit = hints.len();
    while fit > 1 && key_hints_width(text, &hints[..fit], style) > room {
        fit -= 1;
    }
    &hints[..fit]
}

/// Draws the hints on row `row`, from the left; returns the `x` after the last one.
pub fn paint_key_hints(
    layer: &mut Layer,
    text: &mut TextSystem,
    hints: &[KeyHint<'_>],
    row: Rect,
    style: &KeyHintsStyle,
) -> f32 {
    let s = style.scale;
    let px = |v: f32| (v * s).round();
    let look = KeycapLook::hint(style);
    let metrics = text.metrics(&style.font);
    let key_metrics = text.metrics(&style.key_font);
    let cap_h = px(18.0).min(row.size.height);
    let cap_top = (row.min_y() + (row.size.height - cap_h) / 2.0).round();
    let baseline = centered_baseline(row.min_y(), row.size.height, &metrics);
    let mut x = row.min_x();
    for (i, (keys, action)) in hints.iter().enumerate() {
        if i > 0 {
            x += px(14.0);
        }
        for key in *keys {
            let shaped = text.shape_line(key, &style.key_font);
            let w = (shaped.width + px(10.0)).max(cap_h);
            let cap = Rect::new(x, cap_top, w.round(), cap_h);
            let base = centered_baseline(cap_top, cap_h, &key_metrics);
            let tx = keycap_label_x(cap.min_x(), cap.size.width, shaped.width);
            paint_keycap(layer, cap, &shaped, (tx, base), &look);
            x += cap.size.width + px(4.0);
        }
        x += px(2.0);
        let shaped = text.shape_line(action, &style.font);
        draw_text(layer, &shaped, x, baseline, style.text);
        x += shaped.width;
    }
    x
}
