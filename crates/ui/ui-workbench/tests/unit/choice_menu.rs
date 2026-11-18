use super::*;

fn option(label: &str, value: &str, group: u8) -> Choice {
    Choice { icon: "file", label: label.into(), value: value.into(), shortcut: None, group }
}

#[test]
fn rows_copy_their_value_and_outside_closes() {
    let mut menu = ChoiceMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    let long = format!("/Users/me/{}/file.php", "deep/".repeat(60));
    let _ = &long;
    menu.open(
        ("Copy", "file.php", "copy", "copy"),
        vec![option("Absolute", &long, 0), option("Name", "file.php", 1)],
        None,
    );
    menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    assert_eq!(menu.rows.len(), 2);
    let second = menu.rows[1].origin;
    let at = Point::new(second.x + 4.0, second.y + 4.0);
    assert_eq!(menu.hit(at), Some(ChoiceAction::Pick(1)));
    assert_eq!(menu.confirm(), Some(0));
    menu.step(1);
    assert_eq!(menu.confirm(), Some(1));
    assert_eq!(menu.hit(Point::new(5.0, 5.0)), Some(ChoiceAction::Close));
}

#[test]
fn long_lists_scroll_with_the_selection() {
    let mut menu = ChoiceMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 1200.0, 900.0), 1.0);
    let rows = (0..30).map(|i| option(&format!("commit {i}"), "abc", 0)).collect();
    menu.open(("History", "a.php", "git-branch", "compare"), rows, None);
    menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    assert_eq!(menu.rows.len(), VISIBLE);
    menu.step(12);
    assert_eq!(menu.confirm(), Some(12));
    assert!(menu.first > 0 && menu.first <= 12 && 12 < menu.first + VISIBLE);
    menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    let first_row = menu.rows[0].origin;
    let at = Point::new(first_row.x + 4.0, first_row.y + 4.0);
    assert_eq!(menu.hit(at), Some(ChoiceAction::Pick(menu.first)));
}
