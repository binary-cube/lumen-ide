use super::*;
use render_api::Scene;

/// − on the left, + on the right, the value between: each a part of the field.
#[test]
fn a_number_s_parts() {
    let (palette, mut text, mut scene) = (Palette::dark(), TextSystem::new(), Scene::default());
    let ctx = Ctx::new(&palette, 1.0);
    let layer = scene.push_layer(None);
    let rect = Rect::new(10.0, 10.0, 120.0, 28.0);
    let a = number_field(layer, &mut text, &ctx, rect, "42", false);
    assert_eq!(a.minus, Rect::new(10.0, 10.0, 28.0, 28.0));
    assert_eq!(a.plus, Rect::new(102.0, 10.0, 28.0, 28.0));
    assert_eq!(a.value, Rect::new(38.0, 10.0, 64.0, 28.0));
}

/// A swatch per color, side by side; the chosen one ringed (one more quad).
#[test]
fn swatches_side_by_side() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let colors = [Swatch::Default, Swatch::None, Swatch::Color(Rgba::rgb(0xdae8fc))];
    let mut scene = Scene::default();
    let areas = swatch_row(scene.push_layer(None), &ctx, (0.0, 20.0), &colors, None);
    assert_eq!(areas.len(), 3);
    assert_eq!(areas[1].min_x(), 23.0);
    assert_eq!(swatch_row_width(&ctx, 3), 64.0);
    let plain = scene.quad_count();
    let mut ringed = Scene::default();
    swatch_row(ringed.push_layer(None), &ctx, (0.0, 20.0), &colors, Some(2));
    assert_eq!(ringed.quad_count(), plain + 1);
}
