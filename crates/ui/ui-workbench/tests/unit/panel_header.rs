use super::*;
use icons::Icons;
use render_api::Scene;
use text_layout::TextSystem;

/// Narrow, its title is cut before the buttons (never drawn under them); it says the width
/// a short title and its buttons need.
#[test]
fn a_narrow_header_cuts_its_title_before_its_buttons() {
    let (mut text, palette, icons) = (TextSystem::new(), crate::Palette::dark(), Icons::builtin());
    let id = icons.id("plus").unwrap();
    let buttons: Vec<PanelAction> = ["a", "b", "c", "d", "e", "f"]
        .map(|id_| PanelAction { id: id_, icon: id, lit: false })
        .to_vec();
    let groups: Vec<&[PanelAction]> = vec![&buttons[..4], &buttons[4..]];
    let mut header = PanelHeader::default();
    header.set_bounds(Rect::new(0.0, 0.0, 220.0, 44.0), 1.0);
    let mut scene = Scene::default();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    header.paint(Some(id), "Agent Sessions", &groups, painter);
    let first_button = header.rects.iter().map(|(r, _)| r.min_x()).fold(f32::MAX, f32::min);
    let glyphs_right = scene
        .layers()
        .iter()
        .flat_map(|l| l.glyphs.iter())
        // The title's letters (the icons are glyphs too).
        .filter(|g| !icons::is_icon(g.key))
        .map(|g| g.origin.x)
        .fold(0.0_f32, f32::max);
    assert!(glyphs_right < first_button, "{glyphs_right} under the buttons from {first_button}");
    let needs = header.min_width();
    assert!(needs > 220.0 - first_button + TITLE_MIN, "{needs}");
    // Wide enough for its whole title: it asks for no more than it has.
    header.set_bounds(Rect::new(0.0, 0.0, 600.0, 44.0), 1.0);
    let mut scene = Scene::default();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    header.paint(Some(id), "Agent Sessions", &groups, painter);
    assert!(header.min_width() <= 600.0);
}

/// A title that fit before (the project's, beside its buttons) still fits whole.
#[test]
fn a_title_that_fits_is_whole() {
    let (mut text, palette, icons) = (TextSystem::new(), crate::Palette::dark(), Icons::builtin());
    let id = icons.id("plus").unwrap();
    let b: Vec<PanelAction> = ["a", "b", "c", "d", "e", "f"]
        .map(|i| PanelAction { id: i, icon: id, lit: false })
        .to_vec();
    let groups: Vec<&[PanelAction]> = vec![&b[..2], &b[2..4], &b[4..]];
    let mut header = PanelHeader::default();
    // Room for the title in any font (Linux's are wider): only whether it's cut is asked.
    header.set_bounds(Rect::new(90.0, 80.0, 900.0, 88.0), 2.0);
    let mut scene = Scene::default();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    header.paint(Some(id), "Project", &groups, painter);
    let letters = scene
        .layers()
        .iter()
        .flat_map(|l| l.glyphs.iter())
        .filter(|g| !icons::is_icon(g.key))
        .count();
    assert_eq!(letters, "Project".len(), "not cut");
}
