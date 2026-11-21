use super::*;
use text_layout::{Font, FontFamily, TextSystem};

fn shaped(text: &mut TextSystem, key: &str) -> std::sync::Arc<ShapedLine> {
    text.shape_line(key, &Font::new(FontFamily::SansSerif, 11.0))
}

/// A hint's cap: the tile outlined, its relief line under it, the label centered.
#[test]
fn a_hint_cap_has_its_outline_and_its_relief() {
    let style = KeyHintsStyle {
        font: Font::new(FontFamily::SansSerif, 12.0),
        key_font: Font::new(FontFamily::SansSerif, 11.0),
        text: Rgba::WHITE,
        key_text: Rgba::WHITE,
        key_background: Rgba::BLACK,
        key_border: Rgba::rgb(0x808080),
        scale: 2.0,
    };
    let look = KeycapLook::hint(&style);
    assert_eq!(look.radius, 8.0);
    assert_eq!(look.relief.map(|r| (r.inset, r.thickness)), Some((4.0, 2.0)));
    let mut text = TextSystem::new();
    let label = shaped(&mut text, "esc");
    let mut scene = render_api::Scene::default();
    let cap = Rect::new(10.0, 0.0, 60.0, 36.0);
    paint_keycap(scene.push_layer(None), cap, &label, (20.0, 24.0), &look);
    let quads = &scene.layers()[0].quads;
    assert_eq!(quads.len(), 2, "the tile and its relief");
    assert_eq!(quads[1].bounds, Rect::new(14.0, 34.0, 52.0, 2.0));
}

/// A flat cap: the tile alone, no outline nor relief.
#[test]
fn a_flat_cap_is_the_tile_alone() {
    let look = KeycapLook::flat(Rgba::BLACK, Rgba::WHITE, 4.0);
    let mut text = TextSystem::new();
    let label = shaped(&mut text, "K");
    let mut scene = render_api::Scene::default();
    let cap = Rect::new(0.0, 0.0, 18.0, 18.0);
    paint_keycap(scene.push_layer(None), cap, &label, (4.0, 13.0), &look);
    assert_eq!(scene.layers()[0].quads.len(), 1);
    assert!(scene.glyph_count() >= 1);
}

/// A label is centered across the width its owner measured, on a whole pixel.
#[test]
fn a_label_is_centered_on_a_whole_pixel() {
    assert_eq!(keycap_label_x(10.0, 40.0, 20.0), 20.0);
    assert_eq!(keycap_label_x(10.0, 41.0, 20.0), 21.0, "20.5 rounded");
}
