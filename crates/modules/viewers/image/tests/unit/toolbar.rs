use super::*;

/// The toolbar painted in an 800 × 600 view, `can_save` or not.
fn painted(can_save: bool) -> Toolbar {
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    let (mut text, mut scene) = (text_layout::TextSystem::new(), render_api::Scene::default());
    let mut bar = Toolbar::default();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    bar.paint(Rect::new(0.0, 0.0, 800.0, 600.0), 100, can_save, 1.0, painter);
    bar
}

/// Its buttons are the shared one's: Save, with nothing to save, is greyed out and takes no
/// click; with something to save it does, as every other tool.
#[test]
fn save_takes_a_click_only_with_something_to_save() {
    let center = |bar: &Toolbar, tool: Tool| {
        let (r, ..) = bar.buttons.iter().find(|b| b.1 == tool).copied().unwrap();
        Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
    };
    let idle = painted(false);
    assert_eq!(idle.hit(center(&idle, Tool::Save)), None);
    assert_eq!(idle.hit(center(&idle, Tool::Fit)), Some(Tool::Fit));
    let edited = painted(true);
    assert_eq!(edited.hit(center(&edited, Tool::Save)), Some(Tool::Save));
}
