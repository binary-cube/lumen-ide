//! What the module's lists share (the history window, the consoles' popover): the search
//! field as the app's search fields look, and a line of SQL colored with what was searched
//! for lit under it.

use std::ops::Range;

use kernel::Rect;
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::code_tokens::{self, CodeLine};
use ui_components::ds::Ctx;
use ui_kit::TextInputStyle;

/// The search field's style: the app's search pill, room left for its magnifier.
pub fn search_style(ds: &Ctx<'_>, height: f32) -> TextInputStyle {
    ui_components::ds::search_input_style(ds, height)
}

/// `line` of SQL in `font`, drawn from `x` (ending in "…" if it doesn't fit before `max_x`)
/// in the row at `top`, `height` tall, colored, the bytes of `lit` (in the line, in order)
/// lit under it as a search's matches are.
pub fn lit_code(
    layer: &mut Layer,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    (line, font): (&str, &Font),
    (x, max_x): (f32, f32),
    (top, height): (f32, f32),
    lit: &[Range<usize>],
) {
    let p = ds.palette;
    for r in lit {
        let (a, z) = (r.start.min(line.len()), r.end.min(line.len()));
        if a >= z || !line.is_char_boundary(a) || !line.is_char_boundary(z) {
            continue;
        }
        let x0 = x + text.shape_line(&line[..a], font).width;
        if x0 >= max_x {
            break;
        }
        let x1 = (x + text.shape_line(&line[..z], font).width).min(max_x);
        let rect = Rect::new(
            (x0 - ds.px(1.0)).round(),
            (top + height / 2.0 - font.size * 0.75).round(),
            (x1 - x0 + ds.px(2.0)).round(),
            (font.size * 1.5).round(),
        );
        layer.quad(Quad::filled(rect, ds.fade(p.search_match)).rounded(ds.px(3.0)));
    }
    let metrics = text.metrics(font);
    let baseline = ui_kit::centered_baseline(top, height, &metrics);
    code_tokens::draw_code_fitted(
        layer,
        text,
        CodeLine { source: line, extension: "sql", font, x, baseline, palette: p, max_x },
    );
}

/// The ranges of `lit` (in a text) that fall in its line `line` (starting at byte `at`),
/// moved to the line's bytes.
pub fn in_line(lit: &[Range<usize>], at: usize, line: &str) -> Vec<Range<usize>> {
    let end = at + line.len();
    lit.iter()
        .filter(|r| r.start < end && r.end > at)
        .map(|r| r.start.max(at) - at..r.end.min(end) - at)
        .collect()
}
