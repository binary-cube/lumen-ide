use super::*;

/// Without a session (not a remote project): nothing read, nothing shown.
#[test]
fn without_a_session_the_list_is_empty() {
    let mut list = PendingList::default();
    list.set_data(&PendingData { project: kernel::ProjectId::new(), queries: None });
    assert!(!list.poll());
    assert!(list.rows.is_empty());
    assert_eq!(list.generation, 0);
}

/// Each reason as the tree shows it: its state, where it was, why it couldn't go.
#[test]
fn rows_show_as_changed_files() {
    let file = |why: Why| changed_file(&("a.php".to_owned(), why));
    assert_eq!(file(Why::New).state, ChangeState::Added);
    assert_eq!(file(Why::Changed).state, ChangeState::Modified);
    assert_eq!(file(Why::Deleted).state, ChangeState::Deleted);
    let moved = file(Why::Renamed { from: "old.php".into() });
    assert_eq!((moved.state, moved.old_path.as_deref()), (ChangeState::Renamed, Some("old.php")));
    let failed = file(Why::Waiting("refused".into()));
    assert_eq!((failed.state, failed.note.as_deref()), (ChangeState::Failed, Some("refused")));
    assert_eq!(file(Why::Conflict).state, ChangeState::Conflicted);
}

/// The menu: Compare for one file only; `*.ext` offered when all share it.
#[test]
fn the_menu_follows_what_is_chosen() {
    let rows = |paths: &[&str]| -> Vec<(String, Why)> {
        paths.iter().map(|p| ((*p).to_owned(), Why::Changed)).collect()
    };
    let ids = |menu: Vec<ViewMenuEntry>| -> Vec<String> {
        menu.into_iter()
            .filter_map(|e| match e {
                ViewMenuEntry::Item { id, .. } => Some(id),
                _ => None,
            })
            .collect()
    };
    let one = ids(PendingList::menu_for(&rows(&["logs/a.log"]), None));
    assert!(one.iter().any(|i| i == COMPARE) && one.iter().any(|i| i == EXCLUDE_EXT));
    let many = ids(PendingList::menu_for(&rows(&["a.log", "b.LOG"]), Some("logs")));
    assert!(!many.iter().any(|i| i == COMPARE));
    assert!(many.iter().any(|i| i == EXCLUDE_EXT));
    let mixed = ids(PendingList::menu_for(&rows(&["a.log", "b.php"]), None));
    assert!(!mixed.iter().any(|i| i == EXCLUDE_EXT));
    assert_eq!(shared_extension(&["a.tar.gz", "x/b.gz"]).as_deref(), Some(".gz"));
    assert_eq!(shared_extension(&[".env"]), None, "a hidden file has no extension");
}

/// A list of `paths` laid out in a flat tree as a paint would (no session needed).
fn laid_out(paths: &[&str]) -> PendingList {
    let mut list = PendingList::default();
    let rows: Vec<(String, Why)> = paths.iter().map(|p| ((*p).to_owned(), Why::Changed)).collect();
    list.rows = std::sync::Arc::new(rows);
    list.tree.set_files(list.rows.iter().map(changed_file).collect());
    list.tree.set_flat(true);
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let ds = Ctx::new(&palette, 1.0);
    let mut scene = render_api::Scene::default();
    let area = Rect::new(0.0, 0.0, 400.0, 300.0);
    list.tree.paint(&mut scene, &mut text, &icons, &ds, (area, true), None);
    list
}

/// Where file `index`'s row is (looked for down the list).
fn row_point(list: &mut PendingList, index: usize) -> Point {
    let found = (0..300)
        .map(|y| Point::new(200.0, y as f32))
        .find(|&p| list.tree.file_at(p) == Some(index));
    found.expect("the row is drawn")
}

/// The tree is a part: a press chooses a row, ⇧ a range from it, the keys move the choice
/// (the tree has them from the start), the search takes what's typed.
#[test]
fn the_tree_takes_presses_keys_and_text_through_its_controls() {
    let mut list = laid_out(&["a.php", "b.php", "c.php"]);
    let (a, c) = (row_point(&mut list, 0), row_point(&mut list, 2));
    list.press_with(a, 1, PressMods::default());
    list.release();
    assert_eq!(list.tree.chosen(), [0]);
    list.press_with(c, 1, PressMods { shift: true, ..Default::default() });
    list.release();
    assert_eq!(list.tree.chosen().len(), 3, "⇧: the range");
    list.press_with(a, 1, PressMods::default());
    list.release();
    assert!(list.edit_key(ViewerKey::Down), "the keys are the tree's");
    assert_eq!(list.tree.chosen(), [1]);
    assert!(!list.edit_key(ViewerKey::Tab), "Tab isn't the list's");
    list.tree.searching = true;
    assert!(list.text_input("b"));
    assert!(list.command("select_all"));
    assert_eq!(list.tree.selected_text().as_deref(), Some("b"));
    assert!(list.edit_key(ViewerKey::Tab), "kept while searching");
}

/// The header's buttons: a hand over one, none over one still going (pressed, it does
/// nothing).
#[test]
fn a_busy_button_shows_no_hand() {
    let mut list = PendingList::default();
    let button = Rect::new(10.0, 10.0, 80.0, 24.0);
    list.buttons = vec![(button, Button::All)];
    let p = Point::new(20.0, 20.0);
    assert!(list.hover(p), "lit");
    assert_eq!(list.cursor_at(p), Some(ui_components::ViewerCursor::Pointer));
    list.busy = (true, false);
    list.hover(Point::new(21.0, 20.0));
    assert_eq!(list.cursor_at(Point::new(21.0, 20.0)), None);
    assert_eq!(list.press(p, 1), None);
}
