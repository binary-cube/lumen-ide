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
