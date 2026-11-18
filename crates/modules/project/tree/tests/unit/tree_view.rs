use super::*;
use std::path::PathBuf;
use workspace::DirEntry;

fn setup() -> (ProjectTreeView, FileTree) {
    let mut tree = FileTree::new(PathBuf::from("/p"));
    tree.set_children(
        tree.root(),
        vec![DirEntry::new("app", NodeKind::Directory), DirEntry::new("index.php", NodeKind::File)],
    );
    let mut view =
        ProjectTreeView::new(ProjectTreeStyle { row_height: 20.0, ..Default::default() });
    view.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0), 1.0);
    (view, tree)
}

#[test]
fn click_maps_rows_to_actions() {
    let (mut view, mut tree) = setup();
    let app = tree.visible_rows()[1].id;
    let index = tree.visible_rows()[2].id;
    // Rows start after 6px of top padding and are 20px each.
    assert_eq!(view.click(Point::new(10.0, 31.0), &mut tree), Some(TreeAction::Toggle(app)));
    assert_eq!(view.click(Point::new(10.0, 51.0), &mut tree), Some(TreeAction::Open(index)));
    assert_eq!(view.click(Point::new(10.0, 95.0), &mut tree), None);
    assert_eq!(view.click(Point::new(300.0, 31.0), &mut tree), None);
}

#[test]
fn scroll_is_limited_to_content() {
    let (mut view, mut tree) = setup();
    // 3 rows x 20px = 60px < 100px: nothing to scroll.
    assert!(!view.scroll_by_pixels(50.0, &mut tree));
}

#[test]
fn a_panel_grown_after_scrolling_shows_the_rows_above_again() {
    let (mut view, mut tree) = setup();
    let names: Vec<DirEntry> =
        (0..10).map(|i| DirEntry::new(format!("f{i}.php"), NodeKind::File)).collect();
    tree.set_children(tree.root(), names);
    view.scroll_by_pixels(10_000.0, &mut tree);
    // 232px of content in 400px: nothing left to scroll, the root back at the top.
    view.set_bounds(Rect::new(0.0, 0.0, 200.0, 400.0), 1.0);
    assert!(view.scroll_by(0.0, 0.0, &mut tree), "kept within the new limit");
    let root = tree.root();
    assert_eq!(view.click(Point::new(10.0, 11.0), &mut tree), Some(TreeAction::Toggle(root)));
}

#[test]
fn a_new_entrys_row_at_the_end_can_be_scrolled_to() {
    let (mut view, mut tree) = setup();
    let names: Vec<DirEntry> =
        (0..11).map(|i| DirEntry::new(format!("d{i:02}"), NodeKind::Directory)).collect();
    tree.set_children(tree.root(), names);
    let last = tree.visible_rows().last().unwrap().id;
    tree.set_children(last, Vec::new());
    tree.set_expanded(last, true);
    view.begin_new(last, false, &mut tree);
    view.scroll_by_pixels(10_000.0, &mut tree);
    // 12 rows and the input row under the last (272px in 100px): the last folder's row
    // just above the input row, both whole above the padding below.
    assert!(!view.scroll_by(0.0, 0.0, &mut tree), "the input row counted: nothing cut");
    assert_eq!(view.click(Point::new(10.0, 64.0), &mut tree), Some(TreeAction::Toggle(last)));
}

#[test]
fn at_the_end_the_last_row_shows_whole() {
    let (mut view, mut tree) = setup();
    let names: Vec<DirEntry> =
        (0..10).map(|i| DirEntry::new(format!("f{i}.php"), NodeKind::File)).collect();
    tree.set_children(tree.root(), names);
    // 11 rows x 20px + 6px above and below = 232px, in 100px.
    view.scroll_by_pixels(10_000.0, &mut tree);
    let last = tree.visible_rows().last().unwrap().id;
    // The last row's middle (its bottom 6px above the edge) is clickable.
    assert_eq!(view.click(Point::new(10.0, 84.0), &mut tree), Some(TreeAction::Open(last)));
    assert_eq!(view.click(Point::new(10.0, 95.0), &mut tree), None, "the padding below");
}

#[test]
fn long_names_are_cut_short_never_scrolled_and_shown_whole_on_hover() {
    let (mut view, mut tree) = setup();
    let paint = |view: &ProjectTreeView, tree: &mut FileTree| {
        let icons = Icons::builtin();
        let file = |_: &str| (icons.id_or_file("file"), Rgba::WHITE);
        view.paint(tree, &mut TextSystem::new(), &mut Scene::default(), &icons, &file, &|_| None);
    };
    let long = "un_nume_foarte_lung_".repeat(4);
    tree.set_children(tree.root(), vec![DirEntry::new(long.clone(), NodeKind::File)]);
    paint(&view, &mut tree);
    assert!(!view.scroll_by(40.0, 0.0, &mut tree), "no horizontal scroll");
    let (rect, id) = view.cut.borrow().first().copied().expect("the name is cut short");
    let at = Point::new(rect.min_x() + 4.0, rect.min_y() + rect.size.height / 2.0);
    assert_eq!(view.cut_name_at(at).map(|(_, n)| tree.name(n).to_owned()), Some(long));
    assert_eq!(tree.name(id).len(), 80);
}

#[test]
fn a_marked_entry_and_what_it_holds_know_their_mark() {
    use workspace::marks::{PathMark, PathMarks};
    let (view, mut tree) = setup();
    tree.set_children(tree.root(), vec![DirEntry::new("var".to_owned(), NodeKind::Directory)]);
    let ids: Vec<NodeId> = tree.visible_rows().iter().map(|r| r.id).collect();
    let var = ids.into_iter().find(|id| tree.name(*id) == "var").unwrap();
    tree.set_children(var, vec![DirEntry::new("a.php".to_owned(), NodeKind::File)]);
    let mut marks = PathMarks::default();
    marks.set("var", Some(PathMark::Excluded));
    view.set_path_marks(Some(std::sync::Arc::new(marks)));
    assert_eq!(view.path_mark(&tree, var), Some((PathMark::Excluded, true, false)));
}

/// A dependency folder nobody marked (a module's name, a folder a manager's file names): a
/// library found, said so on it, its tint on what it holds; a mark of the user's goes first.
#[test]
fn a_dependency_folder_is_shown_as_a_library_found() {
    use workspace::marks::{PathMark, PathMarks};
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["node_modules"],
        ..Default::default()
    });
    let (view, mut tree) = setup();
    let folder = |name: &str| DirEntry::new(name.to_owned(), NodeKind::Directory);
    tree.set_children(tree.root(), vec![folder("node_modules"), folder("libs"), folder("src")]);
    let id = |tree: &mut FileTree, name: &str| {
        let rows: Vec<NodeId> = tree.visible_rows().iter().map(|r| r.id).collect();
        rows.into_iter().find(|id| tree.name(*id) == name).unwrap()
    };
    let modules = id(&mut tree, "node_modules");
    tree.set_children(modules, vec![folder("left-pad")]);
    tree.set_expanded(modules, true);
    let pad = id(&mut tree, "left-pad");
    let (libs, src) = (id(&mut tree, "libs"), id(&mut tree, "src"));
    kernel::paths::set_found(tree.root_path(), vec!["libs".into()]);
    assert_eq!(view.path_mark(&tree, modules), Some((PathMark::Library, true, true)));
    assert_eq!(view.path_mark(&tree, pad), Some((PathMark::Library, false, true)), "in it");
    assert_eq!(view.path_mark(&tree, libs), Some((PathMark::Library, true, true)));
    assert_eq!(view.path_mark(&tree, src), None);
    let mut marks = PathMarks::default();
    marks.set("node_modules", Some(PathMark::Normal));
    view.set_path_marks(Some(std::sync::Arc::new(marks)));
    assert_eq!(view.path_mark(&tree, modules), Some((PathMark::Normal, true, false)), "the user's");
    kernel::paths::set_found(tree.root_path(), Vec::new());
}

#[test]
fn more_rows_are_selected_with_cmd_and_shift() {
    let (mut view, mut tree) = setup();
    let names: Vec<DirEntry> =
        (0..4).map(|i| DirEntry::new(format!("f{i}.php"), NodeKind::File)).collect();
    tree.set_children(tree.root(), names);
    let ids: Vec<NodeId> = tree.visible_rows().iter().map(|r| r.id).collect();
    // Rows of 20px after 6px of padding: row i's middle at 16 + 20 i.
    let row = |i: usize| Point::new(10.0, 16.0 + 20.0 * i as f32);
    view.click(row(1), &mut tree);
    // ⌘: one more, then one out again.
    assert!(view.select_more(row(3), &mut tree, true));
    assert_eq!(view.selection(&tree), vec![ids[1], ids[3]]);
    assert!(view.select_more(row(1), &mut tree, true));
    assert_eq!(view.selection(&tree), vec![ids[3]]);
    // ⇧: the rows from the last one clicked (row 1) to it.
    assert!(view.select_more(row(3), &mut tree, false));
    assert_eq!(view.selection(&tree), vec![ids[1], ids[2], ids[3]]);
    assert!(view.is_selected(ids[2]) && !view.is_selected(ids[4]));
    // A plain click: only it.
    view.click(row(4), &mut tree);
    assert_eq!(view.selection(&tree), vec![ids[4]]);
    assert!(!view.select_more(Point::new(10.0, 500.0), &mut tree, true), "no row there");
}
