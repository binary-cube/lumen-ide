//! A slider over a few values (its stops: 1 · 2 · 4 · 8): a track filled up to the current
//! value, a tick at each stop, the thumb, and the values under the stops (the current one
//! stands out). A click or a drag anywhere on it picks the nearest stop. Its colors come
//! from the theme ([`SliderStyle::from_palette`]) and can be changed.

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use crate::ds::{Ctx, optical_baseline};
use crate::palette::Palette;

/// How a slider looks (logical pixels for sizes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderStyle {
    pub track: Rgba,
    pub fill: Rgba,
    pub tick: Rgba,
    /// A tick the fill has passed.
    pub tick_passed: Rgba,
    pub thumb: Rgba,
    pub thumb_border: Rgba,
    /// The ring around the thumb under the mouse.
    pub halo: Rgba,
    pub label: Rgba,
    pub label_current: Rgba,
    pub track_height: f32,
    pub thumb_size: f32,
    /// At most this wide (it doesn't stretch across a wide window).
    pub max_width: f32,
    /// A tick at each stop.
    pub ticks: bool,
    /// The values under the stops (without them it's only as tall as its thumb).
    pub labels: bool,
}

impl SliderStyle {
    /// The theme's: the accent fills, the text's colors for the rest.
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            track: p.text.with_alpha(0.10),
            fill: p.accent,
            tick: p.text.with_alpha(0.28),
            tick_passed: p.popup_background.with_alpha(0.85),
            thumb: p.popup_background,
            thumb_border: p.accent,
            halo: p.accent.with_alpha(0.16),
            label: p.text_muted,
            label_current: p.accent,
            track_height: 4.0,
            thumb_size: 16.0,
            max_width: 360.0,
            ticks: true,
            labels: true,
        }
    }
}

/// Where a painted slider is (for clicks and drags).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliderArea {
    /// What takes the mouse: the track, the thumb and the labels.
    pub rect: Rect,
    /// Each stop's x.
    pub stops: Vec<f32>,
}

impl SliderArea {
    /// The stop nearest to `x` (its index).
    pub fn nearest(&self, x: f32) -> Option<usize> {
        self.stops
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map(|(i, _)| i)
    }
}

/// How tall a slider with its values is (logical pixels): the thumb and the values under it.
pub const HEIGHT: f32 = 44.0;

/// How tall a slider in `style` is (logical pixels).
pub fn height(style: &SliderStyle) -> f32 {
    if style.labels { HEIGHT } else { style.thumb_size + 4.0 }
}

/// What [`slider`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct SliderSpec<'a> {
    pub x: f32,
    pub top: f32,
    pub width: f32,
    pub labels: &'a [&'a str],
    pub current: usize,
    pub hovered: bool,
    pub style: &'a SliderStyle,
}

/// Draws a slider at `x`, `top`, at most `width` wide, its stops named `labels`, at stop
/// `current`; `hovered`: the mouse is over it (the thumb gets its ring).
pub fn slider(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: SliderSpec<'_>,
) -> SliderArea {
    let SliderSpec { x, top, width, labels, current, hovered, style } = spec;
    let px = |v: f32| ctx.px(v);
    let width = width.min(px(style.max_width));
    let thumb = px(style.thumb_size);
    let mid = (top + thumb / 2.0 + px(2.0)).round();
    let (start, end) = (x + thumb / 2.0, x + width - thumb / 2.0);
    let count = labels.len().max(1);
    let stops: Vec<f32> = (0..count)
        .map(|i| {
            let t = if count == 1 { 0.0 } else { i as f32 / (count - 1) as f32 };
            (start + (end - start) * t).round()
        })
        .collect();
    let current = current.min(count - 1);
    let at = stops[current];
    // The track, filled up to the current stop.
    let h = px(style.track_height).max(2.0);
    let track = Rect::new(start, (mid - h / 2.0).round(), end - start, h);
    layer.quad(Quad::filled(track, ctx.fade(style.track)).rounded(h / 2.0));
    let filled = Rect::new(start, track.min_y(), at - start, h);
    if filled.size.width > 0.0 {
        layer.quad(Quad::filled(filled, ctx.fade(style.fill)).rounded(h / 2.0));
    }
    // A tick at each stop (on the fill, a lighter one).
    let tick = px(4.0);
    for (i, &sx) in stops.iter().enumerate().filter(|_| style.ticks) {
        let color = if i < current { style.tick_passed } else { style.tick };
        let r = Rect::new((sx - tick / 2.0).round(), (mid - tick / 2.0).round(), tick, tick);
        layer.quad(Quad::filled(r, ctx.fade(color)).rounded(tick / 2.0));
    }
    // The thumb (a ring around it under the mouse).
    if hovered {
        let ring = thumb + px(10.0);
        let r = Rect::new((at - ring / 2.0).round(), (mid - ring / 2.0).round(), ring, ring);
        layer.quad(Quad::filled(r, ctx.fade(style.halo)).rounded(ring / 2.0));
    }
    let r = Rect::new((at - thumb / 2.0).round(), (mid - thumb / 2.0).round(), thumb, thumb);
    layer.quad(
        Quad::filled(r, ctx.fade(style.thumb))
            .rounded(thumb / 2.0)
            .bordered(px(2.0), ctx.fade(style.thumb_border)),
    );
    // The values, under their stops.
    let font = ctx.ui_font(-1.0);
    let strong = font.clone().with_weight(Font::SEMIBOLD);
    let row = mid + thumb / 2.0 + px(4.0);
    for (i, (&sx, label)) in stops.iter().zip(labels).enumerate().filter(|_| style.labels) {
        let (font, color) =
            if i == current { (&strong, style.label_current) } else { (&font, style.label) };
        let shaped = text.shape_line(label, font);
        let lx = (sx - shaped.width / 2.0).round();
        draw_text(layer, &shaped, lx, optical_baseline(row, px(18.0), font), ctx.fade(color));
    }
    let rect = Rect::new(x, top, width, px(height(style)));
    SliderArea { rect, stops }
}

#[cfg(test)]
#[path = "../tests/unit/slider.rs"]
mod tests;
