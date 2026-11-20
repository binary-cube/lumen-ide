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

/// A long list's scrollbar is the app's: its thumb taken along the panel's right edge and
/// dragged moves the list a row at a time (never past its end); a click there picks
/// nothing and doesn't close it. A short list has none.
#[test]
fn a_long_list_s_thumb_is_dragged_a_row_at_a_time() {
    let open = |count: usize| {
        let mut menu = ChoiceMenu::default();
        menu.set_viewport(Rect::new(0.0, 0.0, 1200.0, 900.0), 1.0);
        let rows = (0..count).map(|i| option(&format!("commit {i}"), "abc", 0)).collect();
        menu.open(("History", "a.php", "git-branch", "compare"), rows, None);
        menu.paint(
            &Icons::builtin(),
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
        menu
    };
    let mut menu = open(30);
    let top = menu.rows[0].min_y() + 4.0;
    let right = menu.panel.max_x();
    let grabbed = (0..14)
        .map(|dx| right - 1.0 - dx as f32)
        .find(|x| menu.scrollbar_press(Point::new(*x, top)));
    let x = grabbed.expect("the thumb at the list's top, along the right edge");
    assert_eq!(menu.hit(Point::new(x, top)), Some(ChoiceAction::Inside), "nothing picked");
    assert!(menu.scrollbar_held());
    assert!(menu.scrollbar_drag(Point::new(x, top + 120.0)));
    assert!(menu.first > 0 && menu.first <= 30 - VISIBLE, "{}", menu.first);
    menu.scrollbar_drag(Point::new(x, top + 10_000.0));
    assert_eq!(menu.first, 30 - VISIBLE, "at its end, no further");
    assert!(menu.scrollbar_release() && !menu.scrollbar_held());
    let mut short = open(4);
    let p = Point::new(short.panel.max_x() - 4.0, short.rows[0].min_y() + 4.0);
    assert!(!short.scrollbar_press(p), "all shown: no scrollbar");
}
