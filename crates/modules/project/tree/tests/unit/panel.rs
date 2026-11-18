use super::*;
use code_intelligence::{SymbolHit, SymbolQuery};
use workspace::FileSystem as _;
use workspace::PathQuery;

/// A project on disk (a scratch folder), as the queries port tells it: the disk's entries,
/// a fixed list of its files for the search.
struct Disk {
    root: PathBuf,
    files: Vec<String>,
}

impl ProjectQueries for Disk {
    fn paths(&self, query: &PathQuery<'_>) -> workspace::PathResults {
        let folders: Vec<String> = vec!["src".into()];
        workspace::search_paths(&self.files, &folders, query)
    }
    fn symbols(&self, _: &SymbolQuery<'_>) -> Vec<SymbolHit> {
        Vec::new()
    }
    fn text(&self, _: &text_search::SearchQuery) -> Vec<text_search::Hit> {
        Vec::new()
    }
    fn files(&self, _: &workspace::FileFilter) -> Vec<PathBuf> {
        Vec::new()
    }
    fn class_diagram(&self, _: &str) -> Option<code_intelligence::ClassDiagram> {
        None
    }
    fn root(&self) -> PathBuf {
        self.root.clone()
    }
    fn file_list(&self) -> Arc<Vec<String>> {
        Arc::new(self.files.clone())
    }
    fn symbol_index(&self) -> code_intelligence::SymbolIndex {
        code_intelligence::SymbolIndex::default()
    }
    fn language_of(&self, _: &Path) -> String {
        "php".to_owned()
    }
    fn generation(&self) -> u64 {
        1
    }
    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<DirEntry>> {
        fs_local::LocalFileSystem.read_dir(dir)
    }
    fn file_icon(&self, _: &str) -> (&'static str, String) {
        ("file", "php".to_owned())
    }
}

/// A scratch project: `src/User.php`, `index.php`; a panel showing it, laid out.
fn project(name: &str) -> (PathBuf, ProjectPanel) {
    let root = std::env::temp_dir().join(format!("project-panel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/User.php"), "<?php\n").unwrap();
    std::fs::write(root.join("index.php"), "<?php\n").unwrap();
    let root = root.canonicalize().unwrap();
    let files = vec!["index.php".to_owned(), "src/User.php".to_owned()];
    let mut panel = ProjectPanel::new();
    let queries: Arc<dyn ProjectQueries> = Arc::new(Disk { root: root.clone(), files });
    panel.set_data(&ProjectView {
        project: Some(kernel::ProjectId::new()),
        queries: Some(queries),
        events: kernel::EventBus::new(),
        indexing: false,
    });
    panel.layout(Rect::new(0.0, 0.0, 240.0, 400.0), 1.0);
    (root, panel)
}

/// The visible rows' names, in order.
fn names(panel: &mut ProjectPanel) -> Vec<String> {
    let tree = panel.tree.as_mut().unwrap();
    let ids: Vec<_> = tree.visible_rows().iter().map(|r| r.id).collect();
    ids.into_iter().map(|id| tree.name(id).to_owned()).collect()
}

/// Where row `name` is drawn (its middle).
fn row(panel: &mut ProjectPanel, name: &str) -> Point {
    let tree = panel.tree.as_mut().unwrap();
    let ids: Vec<_> = tree.visible_rows().iter().map(|r| r.id).collect();
    let i = ids.iter().position(|&id| tree.name(id) == name).unwrap();
    let b = panel.view.bounds();
    // As the view draws them: 6px of padding, then the rows.
    let h = ProjectTreeStyle::default().row_height;
    Point::new(b.min_x() + 40.0, b.min_y() + 6.0 + h * i as f32 + h / 2.0)
}

#[test]
fn the_project_comes_through_the_queries_and_another_one_starts_anew() {
    let (root, mut panel) = project("open");
    assert_eq!(panel.root(), Some(root.as_path()));
    assert!(names(&mut panel).contains(&"src".to_owned()));
    // A folder opened by a click is kept when the same project's files change.
    let src = row(&mut panel, "src");
    assert_eq!(panel.click(src), None);
    assert!(names(&mut panel).contains(&"User.php".to_owned()));
    let queries = panel.queries.clone();
    panel.set_data(&ProjectView {
        project: Some(kernel::ProjectId::new()),
        queries,
        events: kernel::EventBus::new(),
        indexing: false,
    });
    assert!(names(&mut panel).contains(&"User.php".to_owned()));
    // No project: nothing shown.
    panel.set_data(&ProjectView {
        project: Some(kernel::ProjectId::new()),
        queries: None,
        events: kernel::EventBus::new(),
        indexing: false,
    });
    assert!(panel.tree().is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_file_clicked_or_found_is_opened_where_typed() {
    let (root, mut panel) = project("open-file");
    let index = row(&mut panel, "index.php");
    assert_eq!(
        panel.click(index),
        Some(ToolPanelAction::View(ViewerAction::OpenFile {
            path: root.join("index.php"),
            line: None,
            column: None
        }))
    );
    // The search takes the keyboard, finds as typed, opens at the place written.
    panel.click(Point::new(60.0, 20.0));
    assert!(panel.wants_text());
    panel.text_input("User.php:12");
    assert!(panel.search.is_active());
    // Found on the search's thread: shown once it comes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ToolPanel::poll(&mut panel) {
        assert!(std::time::Instant::now() < deadline, "the search didn't end");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(panel.edit_key(ViewerKey::Enter));
    assert_eq!(
        panel.take_action(),
        Some(ToolPanelAction::View(ViewerAction::OpenFile {
            path: root.join("src/User.php"),
            line: Some(12),
            column: Some(1)
        }))
    );
    // Taken: the search is empty and the file shown in the tree.
    assert!(!panel.search.is_active());
    assert_eq!(panel.selection(), Some(root.join("src/User.php")));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_new_entry_is_checked_made_and_opened() {
    let (root, mut panel) = project("new");
    assert!(panel.header_action("new-file").is_none());
    assert!(panel.wants_text());
    // Taken: refused, nothing made.
    panel.text_input("index.php");
    panel.edit_key(ViewerKey::Enter);
    assert!(panel.view.pending().is_some_and(|p| p.error));
    // A word erased with ⌥⌫, a free name typed.
    for _ in 0..4 {
        panel.edit_key_with(ViewerKey::Backspace, KeyReach { word: true, extend: false });
    }
    assert!(panel.view.pending().is_some_and(|p| p.input.text().is_empty()));
    panel.text_input("new.php");
    assert!(panel.view.pending().is_some_and(|p| !p.error));
    panel.edit_key(ViewerKey::Enter);
    assert!(root.join("new.php").is_file());
    assert!(panel.view.pending().is_none());
    assert_eq!(panel.selection(), Some(root.join("new.php")));
    assert!(matches!(
        panel.take_action(),
        Some(ToolPanelAction::View(ViewerAction::OpenFile { path, .. })) if path == root.join("new.php")
    ));
    // Esc gives up another one.
    panel.header_action("new-file");
    panel.edit_key(ViewerKey::Escape);
    assert!(panel.view.pending().is_none() && !panel.wants_text());
    // The keyboard going away keeps a name typed (as ↵), gives up an empty or a taken one.
    panel.header_action("new-file");
    panel.text_input("kept.php");
    panel.focus_lost();
    assert!(root.join("kept.php").is_file(), "kept, as ↵");
    assert!(panel.view.pending().is_none());
    panel.header_action("new-file");
    panel.focus_lost();
    assert!(panel.view.pending().is_none(), "nothing typed: given up");
    panel.header_action("new-file");
    panel.text_input("index.php");
    panel.focus_lost();
    assert!(panel.view.pending().is_none(), "taken: given up, nothing made");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_header_locates_expands_collapses_and_hides() {
    let (root, mut panel) = project("header");
    let groups: Vec<u8> = panel.header().unwrap().iter().map(|b| b.group).collect();
    assert_eq!(groups, [0, 0, 1, 1, 2]);
    // Locate: the active file shown.
    let path = root.join("src/User.php");
    let line_of = |_: usize| 0;
    panel.update(&PanelContext {
        path: Some(&path),
        file: None,
        sigil: None,
        cursor: 0,
        line_of: &line_of,
        header: None,
        problems: &[],
    });
    panel.header_action("locate");
    assert_eq!(panel.selection(), Some(path));
    panel.header_action("collapse");
    assert_eq!(names(&mut panel)[1..], ["src", "index.php"]);
    assert_eq!(
        panel.header_action("hide"),
        Some(ToolPanelAction::View(ViewerAction::RunCommand(TOGGLE.into())))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_right_click_or_the_keyboard_says_what_commands_act_on() {
    let (root, mut panel) = project("context");
    let src = row(&mut panel, "src");
    let context = panel.context_at(src).unwrap();
    assert_eq!(context.place, MenuPlace::TreeFolder);
    assert_eq!(context.path(), Some(root.join("src").as_path()));
    // Selected: the keyboard's commands act on it too.
    assert_eq!(panel.context(), Some(context));
    let index = row(&mut panel, "index.php");
    assert_eq!(panel.context_at(index).unwrap().place, MenuPlace::TreeFile);
    // Under the rows (the empty room): what's selected; nothing selected, the root.
    let bounds = panel.view.bounds();
    let empty = Point::new(bounds.min_x() + 20.0, bounds.max_y() - 4.0);
    assert_eq!(panel.context_at(empty).unwrap().path(), Some(root.join("index.php").as_path()));
    panel.view.select(None);
    let context = panel.context_at(empty).unwrap();
    assert_eq!((context.place, context.path()), (MenuPlace::TreeFolder, Some(root.as_path())));
    // While the search has the keyboard, no row is what they act on.
    panel.click(Point::new(60.0, 20.0));
    assert_eq!(panel.context(), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_entry_dragged_onto_a_folder_asks_to_move_it() {
    let (root, mut panel) = project("drag");
    let index = row(&mut panel, "index.php");
    panel.click(index);
    // A few pixels: not a drag yet.
    assert!(!panel.drag(Point::new(index.x + 2.0, index.y)));
    let src = row(&mut panel, "src");
    assert!(panel.drag(src));
    assert!(panel.release());
    assert_eq!(
        panel.take_action(),
        Some(ToolPanelAction::Move { from: root.join("index.php"), into: root.join("src") })
    );
    // Onto where it already is: nothing.
    let index = row(&mut panel, "index.php");
    panel.click(index);
    panel.drag(Point::new(index.x, index.y + 40.0));
    panel.release();
    assert_eq!(panel.take_action(), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_tree_s_keys_and_the_module_s_requests_reach_its_entries() {
    let (root, mut panel) = project("keys");
    // Nothing selected: the editor's commands stay the editor's.
    assert!(!panel.command("copy"));
    let index = row(&mut panel, "index.php");
    panel.click(index);
    panel.take_action();
    for (key, command) in [
        ("copy", "tree.copy"),
        ("cut", "tree.cut"),
        ("paste", "tree.paste"),
        ("delete", "tree.delete"),
    ] {
        assert!(panel.command(key));
        assert_eq!(
            panel.take_action(),
            Some(ToolPanelAction::View(ViewerAction::RunCommand(command.into())))
        );
    }
    // Delete and ⌫ (a Mac's "delete") on the rows: Delete…, which asks first.
    for key in [ViewerKey::Delete, ViewerKey::Backspace] {
        assert!(panel.edit_key(key));
        assert_eq!(
            panel.take_action(),
            Some(ToolPanelAction::View(ViewerAction::RunCommand("tree.delete".into())))
        );
    }
    // The module's commands: a new folder typed in the selected file's folder; an entry shown.
    panel.set_data(&EntriesRequest::NewEntry { is_dir: true });
    assert!(panel.view.pending().is_some_and(|p| p.is_dir) && panel.wants_text());
    panel.edit_key(ViewerKey::Escape);
    panel.set_data(&EntriesRequest::Reveal { path: root.join("src/User.php"), expand: false });
    assert_eq!(panel.selection(), Some(root.join("src/User.php")));
    let _ = std::fs::remove_dir_all(root);
}

/// The panel follows the project's entries as they change (heard on the project's events,
/// applied on its next poll): a file made appears, one removed goes (its selection too);
/// the app's requests: Go to File's search, a folder shown open.
#[test]
fn the_panel_follows_the_disk_and_the_app_s_requests() {
    let (root, mut panel) = project("follow");
    let events = kernel::EventBus::new();
    let queries = panel.queries.clone();
    panel.set_data(&ProjectView {
        project: Some(kernel::ProjectId::new()),
        queries,
        events: events.clone(),
        indexing: false,
    });
    std::fs::write(root.join("new.php"), "").unwrap();
    events.publish(&workspace::events::EntriesChanged { paths: vec![root.join("new.php")] });
    assert!(panel.poll());
    assert!(names(&mut panel).contains(&"new.php".to_owned()));
    let new = row(&mut panel, "new.php");
    panel.click(new);
    std::fs::remove_file(root.join("new.php")).unwrap();
    events.publish(&workspace::events::EntriesChanged { paths: vec![root.join("new.php")] });
    assert!(panel.poll());
    assert!(!names(&mut panel).contains(&"new.php".to_owned()));
    assert_eq!(panel.selection(), None);
    assert!(!panel.poll(), "nothing new");
    // Go to File: the search has the keyboard; a folder revealed, opened.
    panel.set_data(&EntriesRequest::FocusSearch);
    assert!(panel.wants_text() && panel.keys == Keys::Search);
    panel.set_data(&EntriesRequest::Reveal { path: root.join("src"), expand: true });
    assert!(names(&mut panel).contains(&"User.php".to_owned()));
    assert!(!panel.wants_text());
    let _ = std::fs::remove_dir_all(root);
}

/// Many folders changed at once (a checkout): read on the listing's thread, put in the tree
/// at a later poll; meanwhile a small change waits its turn (never put over by the older
/// listing).
#[test]
fn many_changed_folders_are_read_in_the_background() {
    let (root, mut panel) = project("many");
    let events = kernel::EventBus::new();
    let queries = panel.queries.clone();
    panel.set_data(&ProjectView {
        project: Some(kernel::ProjectId::new()),
        queries,
        events: events.clone(),
        indexing: false,
    });
    let dirs: Vec<PathBuf> = (0..LIST_NOW + 2).map(|i| root.join(format!("d{i}"))).collect();
    for dir in &dirs {
        std::fs::create_dir_all(dir).unwrap();
    }
    // Shown (read once, open), then a file made in each.
    events.publish(&workspace::events::EntriesChanged { paths: dirs.clone() });
    assert!(panel.poll());
    for dir in &dirs {
        panel.set_data(&EntriesRequest::Reveal { path: dir.clone(), expand: true });
    }
    let made: Vec<PathBuf> = dirs.iter().map(|d| d.join("new.php")).collect();
    for file in &made {
        std::fs::write(file, "").unwrap();
    }
    events.publish(&workspace::events::EntriesChanged { paths: made.clone() });
    panel.poll();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while names(&mut panel).iter().filter(|n| *n == "new.php").count() < made.len() {
        assert!(std::time::Instant::now() < deadline, "not put in the tree");
        std::thread::sleep(std::time::Duration::from_millis(10));
        panel.poll();
    }
    assert_eq!(panel.in_flight, 0);
    let _ = std::fs::remove_dir_all(root);
}

/// Typing while the tree's rows have the keyboard starts its search (what it held
/// replaced); a space alone doesn't, nor does typing while a field has the keys.
#[test]
fn typing_on_the_rows_starts_the_search() {
    let (root, mut panel) = project("type-search");
    assert_eq!(panel.keys, Keys::Tree);
    assert!(!panel.text_on_rows(" "), "a space alone");
    assert!(panel.text_on_rows("u"));
    assert!(panel.wants_text() && panel.keys == Keys::Search);
    assert!(panel.text_input("s"));
    assert_eq!(panel.search.input.text(), "us");
    assert!(!panel.text_on_rows("x"), "the field has the keys: its text_input takes it");
    panel.keys = Keys::Tree;
    assert!(panel.text_on_rows("m"));
    assert_eq!(panel.search.input.text(), "m", "a new search");
    let _ = std::fs::remove_dir_all(root);
}
