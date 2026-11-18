use super::*;

type Glyphs = Vec<render_api::GlyphInstance>;

/// `source` drawn from x 10 up to `max_x`, fitted and as `draw_code` draws it.
fn drawn(text: &mut TextSystem, font: &Font, source: &str, max_x: f32) -> (Glyphs, Glyphs) {
    let palette = Palette::dark();
    let spec = CodeLine {
        source,
        extension: "sql",
        font,
        x: 10.0,
        baseline: 20.0,
        palette: &palette,
        max_x,
    };
    let mut fitted = render_api::Layer::default();
    draw_code_fitted(&mut fitted, text, spec);
    let mut plain = render_api::Layer::default();
    draw_code(&mut plain, text, spec);
    (fitted.glyphs, plain.glyphs)
}

/// A line that fits is drawn as `draw_code` draws it; one too long ends in "…" before
/// `max_x`, never a glyph cut by it.
#[test]
fn a_line_too_long_ends_in_dots() {
    let mut text = TextSystem::new();
    let font = Font::new(text_layout::FontFamily::Monospace, 13.0);
    let (fitted, plain) = drawn(&mut text, &font, "SELECT 1", 1000.0);
    assert_eq!(fitted, plain);
    let long = "SELECT c.name, count(o.id) AS orders FROM customers c JOIN orders o";
    let dots = text.shape_line("…", &font).glyphs[0].key;
    let advance = text.shape_line("M", &font).width;
    let max_x = 10.0 + advance * 20.5;
    let (fitted, _) = drawn(&mut text, &font, long, max_x);
    assert_eq!(fitted.last().map(|g| g.key), Some(dots), "ends in dots");
    assert!(fitted.iter().all(|g| g.origin.x + advance <= max_x + 0.5), "nothing past the end");
    assert!(fitted.len() >= 18, "as much as fits: {}", fitted.len());
}
