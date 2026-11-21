use super::*;
use crate::palette::Palette;

fn choices() -> LevelChoices {
    LevelChoices {
        anchor: Rect::new(900.0, 700.0, 80.0, 20.0),
        name: "PHP".into(),
        levels: vec!["8.2".into(), "8.3".into(), "8.4".into()],
        project: "8.3".into(),
        auto: true,
        detected: None,
        source: None,
        file_name: None,
        file: None,
    }
}

fn painted(picker: &mut LevelPicker, choices: &LevelChoices) {
    let palette = Palette::dark();
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let icons = icons::Icons::builtin();
    picker.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    picker.paint(
        choices,
        Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons },
    );
}

/// Its filter is the app's search field: typed into, ⌘A selects it, × (a hand over it)
/// empties it; the popover's one field is the one with the caret.
#[test]
fn its_filter_is_the_shared_field_with_its_clear_button() {
    let choices = choices();
    let mut picker = LevelPicker::default();
    picker.open(&choices);
    picker.type_text("8.4");
    assert!(picker.command("select_all"));
    assert_eq!(picker.selected_text().as_deref(), Some("8.4"));
    painted(&mut picker, &choices);
    assert!(picker.query.shows_caret());
    let clear = picker.query.clear_area().expect("× shown with text");
    let at = Point::new(clear.min_x() + 2.0, clear.min_y() + 2.0);
    assert!(picker.over_clickable(at));
    assert_eq!(picker.click(at), LevelClick::Inside);
    assert_eq!(picker.query.text(), "");
    painted(&mut picker, &choices);
    assert_eq!(picker.query.clear_area(), None, "empty: no ×");
}
