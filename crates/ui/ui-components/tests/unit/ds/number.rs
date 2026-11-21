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

/// A color's swatch is ringed in the text at rest, stronger under the mouse, in the accent
/// while it's being picked.
#[test]
fn a_color_swatch_is_ringed_by_its_state() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let red = Rgba::rgb(0xff0000);
    let ring = |swatch: ColorSwatch| {
        let mut scene = Scene::default();
        let layer = scene.push_layer(None);
        swatch.paint(layer, &ctx, Rect::new(0.0, 0.0, 28.0, 28.0));
        (layer.quads[0].color, layer.quads[0].border_color)
    };
    let (fill, rest) = ring(ColorSwatch::new(red));
    assert_eq!(fill, red);
    let (_, hovered) = ring(ColorSwatch::new(red).hovered(true));
    let (_, editing) = ring(ColorSwatch::new(red).hovered(true).editing(true));
    assert_ne!(rest, hovered);
    assert_eq!(editing, palette.accent);
}

/// A color's field: its frame, its swatch (where the picker opens) on the left, its value
/// beside it, a pipette on its right; lit in the accent while picked.
#[test]
fn a_color_field_shows_its_swatch_value_and_pipette() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let rect = Rect::new(10.0, 10.0, 168.0, 30.0);
    let red = Rgba::rgb(0xff0000);
    let mut scene = Scene::default();
    let layer = scene.push_layer(None);
    let swatch = ColorField::new(red, "#ff0000").paint(layer, &mut text, &icons, &ctx, rect);
    assert_eq!(swatch, Rect::new(14.0, 16.0, 18.0, 18.0));
    assert_eq!(layer.quads.len(), 2, "the frame and the swatch");
    assert_eq!(layer.quads[1].color, red);
    assert!(layer.glyphs.len() > 7, "the value's glyphs and the pipette");
    let mut picked = Scene::default();
    let layer = picked.push_layer(None);
    ColorField::new(red, "#ff0000")
        .editing(true)
        .icon(None)
        .paint(layer, &mut text, &icons, &ctx, rect);
    assert_eq!(layer.quads[0].border_color, palette.accent.with_alpha(0.7));
    assert_eq!(layer.glyphs.len(), 7, "the value alone");
}

/// A swatch to choose among a few: no edge; ringed around in its color while chosen, muted
/// under the mouse, nothing at rest.
#[test]
fn a_swatch_may_be_ringed_around() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let red = Rgba::rgb(0xff0000);
    let quads = |swatch: ColorSwatch| {
        let mut scene = Scene::default();
        let layer = scene.push_layer(None);
        swatch.radius(10.0).paint(layer, &ctx, Rect::new(10.0, 10.0, 20.0, 20.0));
        layer.quads.clone()
    };
    let rest = quads(ColorSwatch::new(red).ringed_around(false));
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].border_width, 0.0);
    let chosen = quads(ColorSwatch::new(red).ringed_around(true));
    assert_eq!(chosen[0].bounds, Rect::new(7.0, 7.0, 26.0, 26.0));
    assert_eq!(chosen[0].border_color, red);
    let hovered = quads(ColorSwatch::new(red).ringed_around(false).hovered(true));
    assert_eq!(hovered[0].border_color, palette.text_muted.with_alpha(0.5));
}
