use super::*;
use crate::palette::Palette;

/// Every second row is tinted, the others not; a header's name and subtitle drawn in its
/// column; the lines between the columns only within the clip.
#[test]
fn the_shared_table_parts_draw_where_they_belong() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut scene = render_api::Scene::default();
    let mut text = TextSystem::new();
    let row = Rect::new(0.0, 0.0, 200.0, 24.0);
    let layer = scene.push_layer(None);
    stripe(layer, &ctx, row, 0);
    assert_eq!(layer.quads.len(), 0);
    stripe(layer, &ctx, row, 1);
    assert_eq!(layer.quads.len(), 1);
    let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
    column_lines(layer, &ctx, [50.0, 150.0], 0.0, 100.0, clip);
    assert_eq!(layer.quads.len(), 2, "the one past the clip left out");
    let cell = HeaderCell {
        rect: Rect::new(0.0, 0.0, 120.0, HEADER_H),
        name: "level",
        subtitle: "Level",
        right: false,
        hovered: true,
        sorted: Some(true),
        strong: true,
        pad: ctx.px(CELL_PAD),
    };
    header_cell(layer, &mut text, &ctx, &cell, 0.0);
    assert!(layer.quads.len() > 2, "the hover");
}

/// A table in a window that fades (a modal appearing) fades with it: every part's color
/// at the window's opacity.
#[test]
fn the_table_parts_fade_with_their_window() {
    let palette = Palette::dark();
    let (solid, faded) = (Ctx::new(&palette, 1.0), Ctx::new(&palette, 1.0).with_opacity(0.5));
    let mut text = TextSystem::new();
    let header = Rect::new(0.0, 0.0, 240.0, HEADER_H);
    let draw = |ctx: &Ctx<'_>, text: &mut TextSystem| {
        let mut layer = Layer::default();
        header_band(&mut layer, ctx, header);
        let cell = HeaderCell {
            rect: Rect::new(0.0, 0.0, 120.0, HEADER_H),
            name: "level",
            subtitle: "Level",
            right: false,
            hovered: false,
            sorted: Some(false),
            strong: false,
            pad: ctx.px(CELL_PAD),
        };
        header_cell(&mut layer, text, ctx, &cell, 0.0);
        header_edge(&mut layer, ctx, 120.0, header, true);
        header_rule(&mut layer, ctx, header);
        column_lines(
            &mut layer,
            ctx,
            [120.0],
            HEADER_H,
            100.0,
            header.union(&Rect::new(0.0, 0.0, 240.0, 200.0)),
        );
        stripe(&mut layer, ctx, Rect::new(0.0, HEADER_H, 240.0, 24.0), 1);
        layer
    };
    let (a, b) = (draw(&solid, &mut text), draw(&faded, &mut text));
    assert_eq!(a.quads.len(), b.quads.len());
    for (s, f) in a.quads.iter().zip(&b.quads) {
        assert!((f.color.a - s.color.a * 0.5).abs() < 1e-4, "a quad at half");
    }
    for (s, f) in a.glyphs.iter().zip(&b.glyphs) {
        assert!((f.color.a - s.color.a * 0.5).abs() < 1e-4, "a glyph at half");
    }
}

/// To a screen reader: a column's header, named by its name (its texts not read again).
#[test]
fn a_header_is_a_column_header() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    let cell =
        HeaderCell::new(&ctx, Rect::new(0.0, 0.0, 120.0, HEADER_H), "level").subtitle("Level");
    header_cell(&mut layer, &mut text, &ctx, &cell, 0.0);
    let frame = recording.take();
    let header = frame.nodes().iter().find(|n| n.role == ui_kit::access::Role::ColumnHeader);
    assert_eq!(header.map(|h| &*h.name), Some("level"));
}
