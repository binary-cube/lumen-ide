use super::*;
use crate::Palette;

fn card(lead: Lead<'_>, name: &str) -> (Rect, usize) {
    let (palette, icons) = (Palette::dark(), Icons::builtin());
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let r = paint(
        layer,
        &Ctx::new(&palette, 1.0),
        &mut TextSystem::new(),
        &icons,
        lead,
        name,
        Point::new(100.0, 50.0),
    );
    (r, layer.glyphs.len())
}

/// Beside the mouse (below and right of it, never under it), as wide as what it says; a long
/// name cut, an icon or a tag before it.
#[test]
fn the_card_follows_the_mouse_and_fits_its_name() {
    let icons = Icons::builtin();
    let folder = Lead::Icon(icons.id_or_file("folder"), Rgba::WHITE);
    let (short, _) = card(folder, "db");
    assert!(short.min_x() > 100.0 && short.min_y() > 50.0, "{short:?}");
    let (long, _) = card(folder, &"a very long name ".repeat(30));
    assert!(long.size.width > short.size.width && long.size.width < 340.0, "{long:?}");
    let (tag, _) = card(Lead::Tag("POST", Rgba::WHITE), "db");
    let (bare, _) = card(Lead::Nothing, "db");
    assert!(tag.size.width > bare.size.width && short.size.width > bare.size.width);
}
