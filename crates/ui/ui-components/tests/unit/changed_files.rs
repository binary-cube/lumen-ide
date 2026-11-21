use super::*;

fn file(path: &str, state: ChangeState) -> ChangedFile {
    ChangedFile::new(path, state)
}

fn shown(tree: &FilesTree) -> Vec<String> {
    tree.rows()
        .iter()
        .map(|r| {
            let pad = "  ".repeat(r.depth);
            match &r.kind {
                RowKind::Folder { label, count, .. } => format!("{pad}{label}/ ({count})"),
                RowKind::File { index, .. } => format!("{pad}{}", tree.files()[*index].path),
            }
        })
        .collect()
}

fn sample() -> FilesTree {
    let mut tree = FilesTree::default();
    tree.set_files(vec![
        file("src/FrontOffice/AI/Service/Chat/Bot.php", ChangeState::Added),
        file("src/FrontOffice/AI/Service/Chat/Reply.php", ChangeState::Modified),
        file("src/Shop.php", ChangeState::Modified),
        file("README.md", ChangeState::Deleted),
    ]);
    tree
}

/// Folders first, one holding only a folder folded onto it with how many files it has;
/// files after; the list: every path sorted.
#[test]
fn single_child_folders_fold_onto_one_row() {
    let mut tree = sample();
    assert_eq!(
        shown(&tree),
        [
            "src/ (3)",
            "  FrontOffice/AI/Service/Chat/ (2)",
            "    src/FrontOffice/AI/Service/Chat/Bot.php",
            "    src/FrontOffice/AI/Service/Chat/Reply.php",
            "  src/Shop.php",
            "README.md",
        ]
    );
    tree.set_flat(true);
    assert_eq!(shown(&tree)[0], "README.md");
    assert_eq!(shown(&tree).len(), 4);
}

/// The search keeps the files found and their folders (open), what matched lit in the
/// name; nothing found: no rows.
#[test]
fn the_search_keeps_the_folders_with_something_found() {
    let mut tree = sample();
    tree.expand_all(false);
    assert_eq!(shown(&tree), ["src/ (3)", "README.md"], "collapsed");
    tree.searching = true;
    tree.text_input("reply");
    assert_eq!(
        shown(&tree),
        ["src/FrontOffice/AI/Service/Chat/ (1)", "  src/FrontOffice/AI/Service/Chat/Reply.php"],
        "one folder left: folded whole"
    );
    let RowKind::File { lit, .. } = &tree.rows()[1].kind else { panic!() };
    assert_eq!(lit, &[0, 1, 2, 3, 4], "in the name");
    tree.text_input("zzz");
    assert!(tree.rows().is_empty());
}

/// Keys as in the project tree: ↓ moves, ← closes a folder (then goes up), → opens it, ↵
/// gives a file's change.
#[test]
fn keys_walk_the_tree() {
    let mut tree = sample();
    tree.key(ViewerKey::Down);
    assert_eq!(tree.key(ViewerKey::Left), Some(None));
    assert_eq!(shown(&tree), ["src/ (3)", "README.md"]);
    tree.key(ViewerKey::Right);
    assert_eq!(shown(&tree).len(), 6);
    tree.key(ViewerKey::Down);
    tree.key(ViewerKey::Down);
    assert_eq!(tree.key(ViewerKey::Enter), Some(Some(TreeEvent::Open(0))));
    // Left from a file: its folder.
    tree.key(ViewerKey::Left);
    assert!(matches!(&tree.rows()[1].kind, RowKind::Folder { .. }));
    assert_eq!(tree.selected_row(), Some(1));
    // The files change: the one chosen stays chosen.
    tree.key(ViewerKey::Down);
    let mut files = tree.files().to_vec();
    files.insert(0, file("a.php", ChangeState::Added));
    tree.set_files(files);
    assert_eq!(
        tree.selected_file().map(|i| tree.files()[i].path.as_str()),
        Some("src/FrontOffice/AI/Service/Chat/Bot.php")
    );
}

/// The search is a text field: ⌘A, typing over it, ⌥ by word, ⌘Z.
#[test]
fn the_search_takes_the_edit_commands() {
    let mut tree = sample();
    assert!(!tree.command("select_all"), "not without the keys");
    tree.searching = true;
    tree.text_input("shop bot");
    assert!(tree.command("select_all"));
    assert_eq!(tree.selected_text().as_deref(), Some("shop bot"));
    tree.text_input("reply");
    assert_eq!(shown(&tree).len(), 2, "typing replaced it: the list follows");
    tree.key_with(ViewerKey::Left, crate::KeyReach { word: true, extend: true });
    assert_eq!(tree.selected_text().as_deref(), Some("reply"));
    assert!(tree.command("undo"));
    assert_eq!(tree.query.text(), "shop bot");
}

/// Its scrollbar hides at rest: a scroll shows it (fading in, frames asked), then it goes
/// (no frames once gone).
#[test]
fn the_scrollbar_shows_while_scrolled() {
    let mut tree = FilesTree::default();
    tree.set_files((0..80).map(|i| file(&format!("f{i:02}.php"), ChangeState::Modified)).collect());
    let mut text = text_layout::TextSystem::new();
    let palette = crate::Palette::dark();
    let icons = icons::Icons::builtin();
    let ds = crate::ds::Ctx::new(&palette, 1.0);
    let area = Rect::new(0.0, 0.0, 400.0, 300.0);
    let mut paint = |tree: &mut FilesTree| {
        let mut scene = render_api::Scene::default();
        tree.paint(&mut scene, &mut text, &icons, &ds, (area, true), None);
    };
    paint(&mut tree);
    let now = std::time::Instant::now();
    assert_eq!(tree.bar.opacity_at(now), 0.0, "hidden at rest");
    assert!(tree.next_frame().is_none(), "no frames at rest");
    assert!(tree.scroll_by(120.0));
    paint(&mut tree);
    assert!(tree.next_frame().is_some(), "shown: fading in");
    let later = std::time::Instant::now() + std::time::Duration::from_secs(5);
    assert_eq!(tree.bar.opacity_at(later), 0.0, "gone at rest");
    assert_eq!(tree.bar.next_frame_at(later), None);
}

/// Several files chosen: ⌘ adds one (the one chosen before counts), ⇧ takes a range, a
/// folder chosen is its files; a right click on one outside them chooses it alone.
#[test]
fn several_files_are_chosen() {
    let mut tree = sample();
    let row = |tree: &FilesTree, path: &str| {
        tree.rows()
            .iter()
            .position(|r| matches!(&r.kind, RowKind::File { index, .. } if tree.files()[*index].path == path))
            .unwrap()
    };
    let paths = |tree: &FilesTree| -> Vec<String> {
        tree.chosen().into_iter().map(|i| tree.files()[i].path.clone()).collect()
    };
    let primary = PressMods { primary: true, ..Default::default() };
    let shift = PressMods { shift: true, ..Default::default() };
    tree.choose(row(&tree, "src/Shop.php"), PressMods::default());
    tree.choose(row(&tree, "README.md"), primary);
    assert_eq!(paths(&tree), ["src/Shop.php", "README.md"]);
    // Again: one less.
    tree.choose(row(&tree, "README.md"), primary);
    assert_eq!(paths(&tree), ["src/Shop.php"]);
    // A range: from the last chosen.
    tree.choose(row(&tree, "src/FrontOffice/AI/Service/Chat/Bot.php"), PressMods::default());
    tree.choose(row(&tree, "src/Shop.php"), shift);
    assert_eq!(
        paths(&tree),
        [
            "src/FrontOffice/AI/Service/Chat/Bot.php",
            "src/FrontOffice/AI/Service/Chat/Reply.php",
            "src/Shop.php"
        ]
    );
    // A folder: its files.
    tree.choose(0, PressMods::default());
    assert_eq!(paths(&tree).len(), 3, "{:?}", shown(&tree));
    assert!(tree.choose_all());
    assert_eq!(tree.chosen().len(), 4);
    // Kept by path when the files change.
    let mut files = tree.files().to_vec();
    files.remove(0);
    tree.set_files(files);
    assert_eq!(tree.chosen().len(), 3);
}

/// Its search is the app's search field: pressed it has the keys (typed into, ⌘A), its
/// caret only while the tree has the focus, × (a hand over it) empties it and every file
/// comes back.
#[test]
fn its_search_is_the_shared_field_with_its_clear_button() {
    let mut tree = sample();
    let all = shown(&tree).len();
    let mut text = text_layout::TextSystem::new();
    let palette = crate::Palette::dark();
    let icons = icons::Icons::builtin();
    let ds = crate::ds::Ctx::new(&palette, 1.0);
    let area = Rect::new(0.0, 0.0, 400.0, 300.0);
    let mut paint = |tree: &mut FilesTree, focused: bool| {
        let mut scene = render_api::Scene::default();
        tree.paint(&mut scene, &mut text, &icons, &ds, (area, focused), None);
    };
    paint(&mut tree, true);
    let field = tree.query.area();
    let inside = Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0);
    assert_eq!(tree.cursor_at(inside), Some(crate::ViewerCursor::Text));
    assert!(tree.press(inside, 1).is_some());
    assert!(tree.searching, "pressed: it has the keys");
    assert!(tree.text_input("reply"));
    assert!(tree.command("select_all"));
    assert_eq!(tree.selected_text().as_deref(), Some("reply"));
    paint(&mut tree, true);
    assert!(tree.query.shows_caret());
    paint(&mut tree, false);
    assert!(!tree.query.shows_caret(), "the view without the focus: no caret");
    let clear = tree.query.clear_area().expect("× shown with text");
    let at = Point::new(clear.min_x() + 2.0, clear.min_y() + 2.0);
    assert_eq!(tree.cursor_at(at), Some(crate::ViewerCursor::Pointer));
    tree.press(at, 1);
    assert_eq!(tree.query.text(), "");
    assert_eq!(shown(&tree).len(), all, "emptied: every file back");
}
