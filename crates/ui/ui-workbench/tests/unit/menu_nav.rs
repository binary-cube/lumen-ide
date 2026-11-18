use super::*;

fn item(id: &str, label: &str) -> MenuEntry {
    MenuEntry::Item {
        id: id.into(),
        label: label.into(),
        shortcut: None,
        icon: None,
        on: None,
        look: Default::default(),
    }
}

fn menus() -> Vec<DrawerMenu> {
    vec![
        DrawerMenu {
            title: "File".into(),
            icon: Some("folder"),
            entries: vec![
                item("file.new", "New File"),
                MenuEntry::Separator,
                item("file.save", "Save"),
            ],
        },
        DrawerMenu {
            title: "Refactor".into(),
            icon: None,
            entries: vec![item("refactor.rename", "Rename…"), item("refactor.move", "Move…")],
        },
        DrawerMenu {
            title: "Căutare".into(),
            icon: None,
            entries: vec![item("find.find", "Găsește")],
        },
    ]
}

#[test]
fn the_keyboard_walks_the_categories_and_their_submenus() {
    let mut nav = DrawerNav::new(menus());
    assert_eq!(nav.focus(), NavFocus::Menus);
    assert_eq!(nav.open_menu(), None, "nothing shown before a key or the mouse");
    assert_eq!(nav.key(NavKey::Down), NavOutcome::Used);
    assert_eq!(nav.open_menu(), Some(0), "the selected category's submenu is shown");
    nav.key(NavKey::Up);
    assert_eq!(nav.open_menu(), Some(2), "around the start");
    nav.key(NavKey::Down);
    // → enters the submenu on its first row; ↓ skips the separator; ← leaves it.
    nav.key(NavKey::Right);
    assert_eq!((nav.focus(), nav.selected_item()), (NavFocus::Items, Some(0)));
    nav.key(NavKey::Down);
    assert_eq!(nav.selected_item(), Some(2), "the separator is skipped");
    nav.key(NavKey::Down);
    assert_eq!(nav.selected_item(), Some(0), "around the end");
    nav.key(NavKey::Up);
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Run("file.save".into()));
    nav.key(NavKey::Left);
    assert_eq!(
        (nav.focus(), nav.selected_item(), nav.open_menu()),
        (NavFocus::Menus, None, Some(0))
    );
    // ↵ on a category enters it too.
    nav.key(NavKey::Down);
    nav.key(NavKey::Enter);
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Run("refactor.rename".into()));
}

#[test]
fn the_filter_searches_every_submenu_with_its_path() {
    let mut nav = DrawerNav::new(menus());
    nav.set_query("ref");
    assert!(nav.filtering());
    assert_eq!(nav.open_menu(), None, "no submenu while filtering");
    // "Refactor" matches its menu's title: its items, in the menus' order.
    let paths: Vec<(String, String)> = nav.results().iter().filter_map(|f| nav.path(*f)).collect();
    assert_eq!(
        paths,
        [("Refactor".to_owned(), "Rename…".to_owned()), ("Refactor".into(), "Move…".into())]
    );
    assert_eq!(nav.selected_result(), Some(0));
    nav.key(NavKey::Down);
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Run("refactor.move".into()));
    nav.key(NavKey::Down);
    assert_eq!(nav.selected_result(), Some(0), "around the end");
    // ← → belong to the field's caret.
    assert_eq!(nav.key(NavKey::Left), NavOutcome::Unused);
    // Without diacritics and case: "cautare" finds "Căutare › Găsește".
    nav.set_query("cautare gas");
    assert_eq!(nav.results().len(), 1);
    assert_eq!(nav.result_command(0).as_deref(), Some("find.find"));
    nav.set_query("nothing like it");
    assert!(nav.results().is_empty());
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Used, "nothing to run");
    // Cleared: the categories again.
    nav.set_query("");
    assert_eq!(nav.focus(), NavFocus::Menus);
}

#[test]
fn the_mouse_shows_categories_and_selects_rows() {
    let mut nav = DrawerNav::new(menus());
    assert!(nav.show_menu(1));
    assert!(!nav.show_menu(1), "already shown");
    assert!(nav.hover_item(Some(1)));
    assert_eq!((nav.focus(), nav.selected_item()), (NavFocus::Items, Some(1)));
    assert_eq!(nav.item_command(1).as_deref(), Some("refactor.move"));
    // Between rows: still in the submenu, the last row still selected.
    assert!(!nav.hover_item(None));
    assert_eq!((nav.focus(), nav.selected_item()), (NavFocus::Items, Some(1)));
    nav.show_menu(0);
    assert_eq!(nav.focus(), NavFocus::Menus);
    // A separator isn't selected, but the mouse is in the submenu.
    assert!(nav.hover_item(Some(1)));
    assert_eq!((nav.focus(), nav.selected_item()), (NavFocus::Items, None));
    assert_eq!(nav.item_command(1), None);
}

fn submenu() -> Rect {
    Rect::new(300.0, 40.0, 300.0, 400.0)
}

#[test]
fn the_intent_triangle_points_at_the_submenu() {
    // From a category on the left toward the submenu's edge: inside.
    assert!(aims_at(Point::new(200.0, 100.0), Point::new(210.0, 104.0), submenu()));
    // Straight down (another category, not toward it): outside.
    assert!(!aims_at(Point::new(200.0, 100.0), Point::new(200.0, 130.0), submenu()));
    // Back to the left: outside.
    assert!(!aims_at(Point::new(200.0, 100.0), Point::new(190.0, 101.0), submenu()));
    // Steeply down-right past the submenu's bottom corner's line: outside.
    assert!(!aims_at(Point::new(200.0, 420.0), Point::new(202.0, 470.0), submenu()));
}

#[test]
fn another_category_waits_briefly_and_longer_when_heading_for_the_submenu() {
    let t = Instant::now();
    let mut intent = HoverIntent::default();
    // Nothing shown: at once.
    assert_eq!(intent.moved(Point::new(100.0, 60.0), Some(0), None, None, t), Intent::Show(0));
    // On the shown one: nothing.
    assert_eq!(
        intent.moved(Point::new(150.0, 62.0), Some(0), Some(0), Some(submenu()), t),
        Intent::Keep
    );
    // Down onto the next category (not toward the submenu): a short wait.
    let Intent::Wait(at) =
        intent.moved(Point::new(150.0, 96.0), Some(1), Some(0), Some(submenu()), t)
    else {
        panic!("a short wait expected");
    };
    assert_eq!(at, t + SWITCH_DELAY);
    assert_eq!(intent.due(t), None, "not yet");
    assert_eq!(intent.deadline(), Some(at));
    assert_eq!(intent.due(at), Some(1));
    assert_eq!(intent.due(at), None, "taken");

    // Diagonally toward the submenu across category 2: the switch put off while it moves.
    let mut intent = HoverIntent::default();
    intent.moved(Point::new(150.0, 62.0), Some(0), Some(0), Some(submenu()), t);
    let Intent::Wait(at) =
        intent.moved(Point::new(180.0, 70.0), Some(1), Some(0), Some(submenu()), t)
    else {
        panic!("a wait expected");
    };
    assert_eq!(at, t + AIM_DELAY);
    // It reaches the submenu: the switch dropped.
    assert_eq!(
        intent.moved(Point::new(320.0, 90.0), None, Some(0), Some(submenu()), t),
        Intent::Keep
    );
    assert_eq!(intent.due(t + AIM_DELAY), None);
}

/// Code: Complete, then Folding › (Collapse, Expand | Collapse All), then Reformat.
fn nested() -> Vec<DrawerMenu> {
    vec![DrawerMenu {
        title: "Code".into(),
        icon: None,
        entries: vec![
            item("code.complete", "Complete"),
            MenuEntry::Submenu {
                label: "Folding".into(),
                icon: None,
                entries: vec![
                    item("code.fold", "Collapse"),
                    item("code.unfold", "Expand"),
                    MenuEntry::Separator,
                    item("code.fold_all", "Collapse All"),
                ],
            },
            item("code.format", "Reformat"),
        ],
    }]
}

#[test]
fn the_keyboard_walks_three_levels() {
    let mut nav = DrawerNav::new(nested());
    nav.key(NavKey::Down);
    nav.key(NavKey::Right);
    nav.key(NavKey::Down);
    assert_eq!(
        (nav.focus(), nav.selected_item(), nav.open_sub()),
        (NavFocus::Items, Some(1), None)
    );
    // ↵ on a submenu's row opens it (nothing runs), on its first row.
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Used);
    assert_eq!(
        (nav.focus(), nav.open_sub(), nav.selected_sub_item()),
        (NavFocus::Sub, Some(1), Some(0))
    );
    assert_eq!(nav.sub_entries().map(|(title, _)| title), Some("Folding"));
    // ↓ skips the separator, around the end.
    nav.key(NavKey::Down);
    nav.key(NavKey::Down);
    assert_eq!(nav.selected_sub_item(), Some(3));
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Run("code.fold_all".into()));
    // ← closes it: back on its row.
    nav.key(NavKey::Left);
    assert_eq!(
        (nav.focus(), nav.selected_item(), nav.open_sub()),
        (NavFocus::Items, Some(1), None)
    );
    // → opens it again; ↓ in the submenu closes it.
    nav.key(NavKey::Right);
    nav.key(NavKey::Left);
    nav.key(NavKey::Down);
    assert_eq!((nav.selected_item(), nav.open_sub()), (Some(2), None));
    assert_eq!(nav.key(NavKey::Right), NavOutcome::Used, "no submenu: nothing");
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Run("code.format".into()));
}

#[test]
fn the_mouse_opens_a_rows_submenu_and_selects_in_it() {
    let mut nav = DrawerNav::new(nested());
    nav.show_menu(0);
    assert!(nav.hover_item(Some(1)));
    assert!(nav.item_is_submenu(1) && nav.item_command(1).is_none());
    assert!(nav.show_sub(1));
    assert_eq!(
        (nav.focus(), nav.open_sub()),
        (NavFocus::Items, Some(1)),
        "shown, the keys still here"
    );
    assert!(nav.hover_sub_item(Some(1)));
    assert_eq!((nav.focus(), nav.selected_sub_item()), (NavFocus::Sub, Some(1)));
    assert_eq!(nav.sub_item_command(1).as_deref(), Some("code.unfold"));
    // Between its rows the last stays; back on another row: the submenu closes.
    assert!(!nav.hover_sub_item(None));
    assert_eq!(nav.selected_sub_item(), Some(1));
    nav.hover_item(Some(2));
    assert!(nav.show_sub(2));
    assert_eq!(nav.open_sub(), None);
    // Its header back (a narrow window): on its row again.
    nav.show_sub(1);
    nav.hover_sub_item(Some(0));
    assert!(nav.leave_sub());
    assert_eq!(
        (nav.focus(), nav.selected_item(), nav.open_sub()),
        (NavFocus::Items, Some(1), None)
    );
}

#[test]
fn the_filter_finds_submenus_rows_with_their_whole_path() {
    let mut nav = DrawerNav::new(nested());
    // The submenu's title matches: every row of it.
    nav.set_query("folding");
    let paths: Vec<(String, String)> = nav.results().iter().filter_map(|f| nav.path(*f)).collect();
    assert_eq!(
        paths,
        [
            ("Code › Folding".to_owned(), "Collapse".to_owned()),
            ("Code › Folding".into(), "Expand".into()),
            ("Code › Folding".into(), "Collapse All".into()),
        ]
    );
    nav.key(NavKey::Up);
    assert_eq!(nav.key(NavKey::Enter), NavOutcome::Run("code.fold_all".into()));
    // The path with its label ("code folding collapse all").
    nav.set_query("code folding collapse all");
    assert_eq!(nav.results().len(), 1);
    assert_eq!(nav.result_command(0).as_deref(), Some("code.fold_all"));
    nav.set_query("reformat");
    assert_eq!(nav.result_command(0).as_deref(), Some("code.format"));
}
