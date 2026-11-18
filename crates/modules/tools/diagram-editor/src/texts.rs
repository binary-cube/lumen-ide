//! A cell's text laid out in its box — its paragraphs, wrapped to the box when its style
//! says so, aligned across and down — and drawn: the same for the view, the SVG export and
//! the PNG export, so a label breaks the same everywhere.

use std::sync::Arc;

use kernel::{Point, Rect};
use render_api::{Layer, Quad};
use text_layout::{Font, FontFamily, ShapedLine, TextSystem};
use ui_kit::draw_text;

use crate::drawing::Text;
use crate::look::{Align, VAlign};

/// A line laid out: its text, its shape, where it starts (its left and its top).
pub struct Laid {
    pub text: String,
    pub shaped: Arc<ShapedLine>,
    pub at: Point,
}

/// The font `t` is drawn in at `size`.
pub fn font(family: &FontFamily, t: &Text, size: f32) -> Font {
    let font = Font::new(family.clone(), size).with_italic(t.italic);
    if t.bold { font.with_weight(Font::BOLD) } else { font }
}

/// `t`'s lines in box `area` (pixels) at scale `k`, and their height. Empty when it's too
/// small to read.
pub fn lay(
    text: &mut TextSystem,
    family: &FontFamily,
    t: &Text,
    area: Rect,
    k: f32,
) -> (Vec<Laid>, f32) {
    let size = t.size * k;
    if size < 3.0 || t.text.is_empty() {
        return (Vec::new(), 0.0);
    }
    let font = font(family, t, size);
    let pad = 2.0 * k;
    let room = (area.size.width - 2.0 * pad).max(1.0);
    let mut lines: Vec<String> = Vec::new();
    for paragraph in t.text.split('\n') {
        if !t.wrap {
            lines.push(paragraph.to_owned());
            continue;
        }
        wrap(text, &font, paragraph, room, &mut lines);
    }
    let line_h = size * 1.2;
    let height = line_h * lines.len() as f32;
    let top = match t.valign {
        VAlign::Top => area.min_y() + pad,
        VAlign::Bottom => area.max_y() - pad - height,
        VAlign::Middle => area.center().y - height / 2.0,
    };
    let laid = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let shaped = text.shape_line(&line, &font);
            let x = match t.align {
                Align::Left => area.min_x() + pad,
                Align::Right => area.max_x() - pad - shaped.width,
                Align::Center => area.center().x - shaped.width / 2.0,
            };
            Laid { text: line, shaped, at: Point::new(x, top + line_h * i as f32) }
        })
        .collect();
    (laid, line_h)
}

/// `paragraph`'s lines within `room`, broken at spaces (a word longer than the room on a
/// line of its own), pushed to `lines`. The paragraph is shaped once and a line's width read
/// from its glyphs' places; only a line within a pixel of the room is shaped on its own to
/// decide (what was shaped on its own decides, as when every line was): one shaping a
/// paragraph instead of one a word.
fn wrap(text: &mut TextSystem, font: &Font, paragraph: &str, room: f32, lines: &mut Vec<String>) {
    let whole = text.shape_line(paragraph, font);
    // Read from the glyphs only where they run left to right in the text's order (else, a
    // right-to-left script: each line shaped on its own).
    let ordered = whole.glyphs.windows(2).all(|w| w[0].start <= w[1].start && w[0].x <= w[1].x);
    // How close to the room an estimate is checked by shaping (kerning at the break moves
    // a line's width by a fraction of the font's size).
    let near = 1.0 + font.size * 0.15;
    // Where each byte's glyph starts (a byte inside a glyph: the next glyph's place); the
    // glyphs in the text's order, found by halving (a long label stays linear).
    let x_at = |byte: usize| -> f32 {
        let i = whole.glyphs.partition_point(|g| g.start < byte);
        whole.glyphs.get(i).map_or(whole.width, |g| g.x)
    };
    let mut start = 0usize;
    let mut end: Option<usize> = None;
    let words = paragraph.match_indices(' ').map(|(i, _)| i).chain([paragraph.len()]);
    for at in words {
        let Some(line_end) = end else {
            end = Some(at);
            continue;
        };
        // An empty line so far (two spaces, a leading one): the word replaces it, the space
        // before it dropped.
        if line_end == start {
            start = line_end + 1;
            end = Some(at);
            continue;
        }
        let candidate = &paragraph[start..at];
        let estimate = if ordered { x_at(at) - x_at(start) } else { f32::NAN };
        let fits = if !ordered || (estimate - room).abs() <= near {
            text.shape_line(candidate, font).width <= room
        } else {
            estimate <= room
        };
        if fits {
            end = Some(at);
        } else {
            lines.push(paragraph[start..line_end].to_owned());
            start = line_end + 1;
            end = Some(at);
        }
    }
    lines.push(paragraph.get(start..).unwrap_or_default().to_owned());
}

/// `t` drawn into `layer` in box `area` (pixels) at scale `k` (its backdrop under each line).
pub fn paint(
    layer: &mut Layer,
    text: &mut TextSystem,
    family: &FontFamily,
    t: &Text,
    area: Rect,
    k: f32,
) {
    let (lines, line_h) = lay(text, family, t, area, k);
    draw(layer, &lines, line_h, t, Point::new(0.0, 0.0), (k, 1.0));
}

/// Lines laid out at scale `k` (as [`lay`] gives them) drawn moved by `by`, their places
/// stretched by `stretch` (laid out at a scale near the one shown: the lines where they'd
/// be, their glyphs at the scale they were shaped at).
pub fn draw(
    layer: &mut Layer,
    lines: &[Laid],
    line_h: f32,
    t: &Text,
    by: Point,
    (k, stretch): (f32, f32),
) {
    let size = t.size * k;
    for line in lines {
        // The glyphs keep their width: what's stretched is the side the line is aligned
        // to (its right end, its middle), so it stays there.
        let w = line.shaped.width;
        let x = match t.align {
            Align::Left => line.at.x * stretch,
            Align::Right => (line.at.x + w) * stretch - w,
            Align::Center => (line.at.x + w / 2.0) * stretch - w / 2.0,
        };
        let (x, y) = (by.x + x, by.y + line.at.y * stretch);
        if let Some(back) = t.backdrop {
            let h = line_h * stretch;
            layer.quad(Quad::filled(Rect::new(x - 2.0, y, line.shaped.width + 4.0, h), back));
        }
        draw_text(layer, &line.shaped, x.round(), (y + size * stretch).round(), t.color);
    }
}

/// The steps texts are laid out at while zooming: a twelfth of an octave apart (a glyph at
/// most ~3% off its exact size), so a zoom lays them out again only when it crosses one.
pub fn text_step(k: f32) -> (i32, f32) {
    let n = (k.max(1e-3).log2() * 12.0).round() as i32;
    (n, 2f32.powf(n as f32 / 12.0))
}

#[cfg(test)]
#[path = "../tests/unit/texts.rs"]
mod tests;
