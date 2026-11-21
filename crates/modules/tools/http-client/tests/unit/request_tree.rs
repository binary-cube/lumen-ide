//! The tree: folders opened and closed, a request chosen, the search narrowing it and Esc
//! clearing it, the keys walking it; names typed on their rows, kept when the keyboard
//! goes; its menus.

use super::*;
use crate::collections::{Listed, RequestFile};

fn tree(within: &str) -> RequestTree {
    let listed = |label: &str, method: &str, line| Listed {
        label: label.into(),
        method: method.into(),
        line,
        hash: 0,
    };
    let listing = Listing {
        files: vec![RequestFile {
            path: "http/users/users.http".into(),
            requests: vec![listed("login", "POST", 1), listed("me", "GET", 5)],
        }],
        folders: vec!["http".into(), "http/users".into(), "http/users/admin".into()],
    };
    let mut t =
        if within.is_empty() { RequestTree::project() } else { RequestTree::collection(within) };
    t.listing = Arc::new(listing);
    t.filtered();
    t
}

fn key(t: &mut RequestTree, k: ViewerKey) -> bool {
    t.edit_key(k, KeyReach::default())
}

#[test]
fn folders_open_and_close_and_a_request_is_chosen() {
    let mut t = tree("");
    assert_eq!(t.rows.len(), 5, "http, users, admin, two requests");
    t.activate(1, false, false);
    assert_eq!(t.rows.len(), 2, "users closed");
    t.activate(1, false, false);
    // A request: asked for (no project here: nothing to open it with).
    t.activate(3, false, false);
    assert!(t.take_action().is_none());
    // A double click on a folder: its collection opened.
    t.activate(2, false, true);
    match t.take_action() {
        Some(TreeAction::OpenCollection(path)) => assert_eq!(path, "http/users"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_search_narrows_and_esc_clears_it() {
    let mut t = tree("");
    assert!(t.text_input("log"));
    assert_eq!(t.rows.len(), 3, "http, users, login");
    assert!(key(&mut t, ViewerKey::Escape));
    assert_eq!(t.rows.len(), 5);
    assert!(!key(&mut t, ViewerKey::Escape), "nothing to clear");
}

/// A line break typed or pasted into the search is a space (the shared field's one rule).
#[test]
fn a_line_break_in_the_search_is_a_space() {
    let mut t = tree("");
    assert!(t.text_input("lo\ng"));
    assert_eq!(t.filter.text(), "lo g");
}

#[test]
fn the_rows_are_walked_with_the_keys() {
    let mut t = tree("");
    assert!(key(&mut t, ViewerKey::Down));
    assert_eq!(t.selected, Some(0));
    assert!(key(&mut t, ViewerKey::Left));
    assert_eq!(t.rows.len(), 1, "← closes the folder");
    assert!(key(&mut t, ViewerKey::Right));
    assert_eq!(t.rows.len(), 5, "→ opens it");
    assert!(key(&mut t, ViewerKey::End));
    assert_eq!(t.selected, Some(4));
    assert!(key(&mut t, ViewerKey::Left));
    assert_eq!(t.selected, Some(1), "← on a request: to its folder");
    assert!(key(&mut t, ViewerKey::Home));
    assert_eq!(t.selected, Some(0));
}

#[test]
fn a_collection_is_named_on_its_row_and_kept_when_the_keyboard_goes() {
    let mut t = tree("");
    t.menu_row = Some(1);
    assert!(t.menu_command("row-new-collection"));
    assert_eq!(t.slots[2], Slot::Naming(2), "a row of its own, first in its folder");
    assert!(t.wants_text());
    assert!(t.text_input("a/b"));
    assert!(key(&mut t, ViewerKey::Enter));
    assert!(t.naming.is_some() && t.naming_error, "not a name: asked again, marked");
    assert!(t.take_action().is_some(), "told why");
    assert!(key(&mut t, ViewerKey::Escape));
    assert!(t.naming.is_none() && t.slots.len() == 5, "Esc gives it up");
    // Renamed on its own row, its name to be typed over.
    t.menu_row = Some(1);
    assert!(t.menu_command("row-rename"));
    assert_eq!(t.slots[1], Slot::Naming(1));
    assert_eq!(t.naming.as_ref().unwrap().1.selected_text(), "users");
    // Nothing typed and the keyboard gone: nothing done.
    t.focus_lost();
    assert!(t.naming.is_none());
    // From the empty area: a new collection in the project's collections' folder.
    t.menu_row = None;
    assert!(t.menu_command("row-new-collection"));
    assert_eq!((t.slots[0], t.slots[1]), (Slot::Row(0), Slot::Naming(1)), "first in `http`");
}

#[test]
fn a_collection_s_tree_makes_what_s_new_in_it() {
    let mut t = tree("http/users");
    assert_eq!(t.rows.len(), 3, "admin, two requests");
    t.new_collection(None);
    assert_eq!(
        t.naming.as_ref().map(|(n, _)| n.clone()),
        Some(Naming::NewCollection { parent: "http/users".into() })
    );
    assert_eq!(t.slots[0], Slot::Naming(0), "at its top");
}

#[test]
fn menus_differ_by_row() {
    let mut t = tree("");
    t.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    t.scale = 1.0;
    let ids = |entries: Vec<ViewMenuEntry>| -> Vec<String> {
        entries
            .iter()
            .filter_map(|e| match e {
                ViewMenuEntry::Item { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect()
    };
    let folder = ids(t.menu_at(Point::new(40.0, 6.0 + 26.0 + 10.0), true).unwrap());
    assert!(
        folder.contains(&"row-open-collection".to_owned())
            && folder.contains(&"row-rename".to_owned())
    );
    let request = ids(t.menu_at(Point::new(40.0, 6.0 + 3.0 * 26.0 + 10.0), true).unwrap());
    assert!(
        request.contains(&"row-delete".to_owned()) && !request.contains(&"row-rename".to_owned())
    );
    let empty = ids(t.menu_at(Point::new(40.0, 380.0), true).unwrap());
    assert!(empty.contains(&"row-new-collection".to_owned()));
}

/// A request dragged over a folder (or one of its requests) would go into that folder: the
/// one lit, with what's in it; over its own folder nothing moves, nothing is lit.
#[test]
fn a_dragged_request_lights_the_folder_it_would_go_into() {
    let mut t = tree("");
    t.scale = 1.0;
    t.list = Rect::new(0.0, 0.0, 300.0, 400.0);
    let h = t.row_h();
    let row = |i: usize| Point::new(100.0, 6.0 + (i as f32 + 0.5) * h);
    let login = t.rows.iter().position(|r| matches!(r, Row::Request { request: 0, .. })).unwrap();
    t.press(row(login), 1);
    assert!(t.drag(Point::new(100.0, row(login).y + 30.0)) || t.dragged_to.is_none());
    // Over `admin` (a folder of its own): it goes there.
    t.dragged_to = Some(row(2));
    assert_eq!(t.drop_folder().as_deref(), Some("http/users/admin"));
    assert!(t.in_folder(2, "http/users/admin") && !t.in_folder(1, "http/users/admin"));
    // Over the other request of its own file: nothing moves.
    t.dragged_to = Some(row(login + 1));
    assert_eq!(t.drop_folder(), None);
    // Over `http`: everything under it is in it.
    t.dragged_to = Some(row(0));
    assert_eq!(t.drop_folder().as_deref(), Some("http"));
    assert!((0..t.rows.len()).all(|i| t.in_folder(i, "http")));
}

/// A project as the queries port tells it: its root, a fixed list of its files (the
/// project's index, not up to date with a file just made).
struct Project {
    root: PathBuf,
    files: Vec<String>,
}

impl ProjectQueries for Project {
    fn paths(&self, _: &workspace::PathQuery<'_>) -> workspace::PathResults {
        workspace::PathResults::default()
    }
    fn symbols(&self, _: &code_intelligence::SymbolQuery<'_>) -> Vec<code_intelligence::SymbolHit> {
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
    fn language_of(&self, _: &std::path::Path) -> String {
        "http".to_owned()
    }
    fn generation(&self) -> u64 {
        1
    }
    fn list_dir(&self, _: &std::path::Path) -> std::io::Result<Vec<workspace::DirEntry>> {
        Ok(Vec::new())
    }
    fn file_icon(&self, _: &str) -> (&'static str, String) {
        ("file", String::new())
    }
}

/// A request made at the project's root (outside the collections' folder) shows at once,
/// though the project's list of files doesn't have its file yet (built again meanwhile).
#[test]
fn a_request_made_at_the_root_shows_before_the_project_lists_it() {
    let root = std::env::temp_dir().join(format!("http-made-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("requests.http"), "### A request\nGET https://example.com\n").unwrap();
    let mut t = RequestTree::project();
    t.set_project(Some(Arc::new(Project { root: root.clone(), files: Vec::new() })), None);
    *t.made.lock().unwrap() = Some(Made::Request("requests.http".into(), 0));
    t.refresh();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !t.rows.iter().any(|r| matches!(r, Row::Request { .. })) {
        assert!(std::time::Instant::now() < deadline, "the request never showed");
        t.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// The search is the app's search field: ⌘A selects it, ⌘X cuts it (every row again), ⌘Z
/// brings it back; a press on its × empties it and lists every row; a name being typed has
/// the keys alone (the search's caret goes).
#[test]
fn the_search_edits_and_empties_as_the_app_s_field() {
    let mut t = tree("");
    assert!(t.text_input("log"));
    assert!(t.edit_command("select_all"));
    assert_eq!(t.selected_text().as_deref(), Some("log"), "⌘A: all of it");
    assert!(t.edit_command("cut"));
    assert_eq!(t.rows.len(), 5, "⌘X: every row");
    assert!(t.edit_command("undo"));
    assert_eq!(t.rows.len(), 3, "⌘Z: narrowed again");
    // Drawn where its field is, its × empties it.
    t.field = Rect::new(0.0, 0.0, 240.0, 30.0);
    let palette = ui_components::palette::Palette::dark();
    let ctx = ds::Ctx::new(&palette, 1.0);
    let (mut layer, mut text) = (Layer::default(), TextSystem::new());
    let icons = icons::Icons::builtin();
    t.filter.paint(&mut layer, &mut text, &icons, &ctx, (t.field, true), "");
    // × is centered this far from the field's right edge (the shared field's room for it).
    let clear = Point::new(t.field.max_x() - 17.0, t.field.min_y() + 15.0);
    assert_eq!(t.cursor_at(clear), Some(ViewerCursor::Pointer));
    t.press(clear, 1);
    assert!(t.filter.text().is_empty());
    assert_eq!(t.rows.len(), 5, "× empties it: every row");
    // A name typed: the search no longer has the keys.
    t.new_collection(None);
    assert!(!t.typing && t.naming.is_some());
    assert!(t.edit_command("select_all"), "⌘A: the name's");
}
