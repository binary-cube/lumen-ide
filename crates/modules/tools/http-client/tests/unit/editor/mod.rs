//! The visual editor: its request read on a worker, a change written back into its file a
//! moment after (and the next requests kept), a new one written at the end, typing in the
//! URL and the tables, `{{` offering the environment's names, Cancel while it runs.

use super::*;
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("http-editor-{name}-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wait(editor: &mut RequestEditor, ready: impl Fn(&RequestEditor) -> bool) {
    let started = Instant::now();
    while !ready(editor) && started.elapsed() < Duration::from_secs(10) {
        editor.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn editor(dir: &std::path::Path, text: &str, index: Option<usize>) -> (RequestEditor, PathBuf) {
    let file = dir.join("api.http");
    std::fs::write(&file, text).unwrap();
    let runs = Runs::of(Some(kernel::ProjectId::new()), Some(dir));
    let mut editor = RequestEditor::default();
    editor.set_data(&EditorData { runs, file: file.clone(), index });
    wait(&mut editor, |e| e.form.is_some());
    (editor, file)
}

#[test]
fn a_change_is_written_into_its_file_and_the_rest_kept() {
    let dir = scratch("write");
    let text =
        "@host = e.com\n\n### One\nGET https://{{host}}/1\n\n### Two\nGET https://{{host}}/2\n";
    let (mut editor, file) = editor(&dir, text, Some(0));
    assert_eq!(editor.form.as_ref().unwrap().url.text(), "https://{{host}}/1");
    editor.focus = Some(Focus::Url);
    editor.form.as_mut().unwrap().url.select_all();
    assert!(editor.text_input("https://{{host}}/one?page=2"));
    assert_eq!(editor.form.as_ref().unwrap().params.len(), 1, "its query as a parameter");
    // Written a moment after.
    editor.changed = Some(Instant::now() - SAVE_AFTER);
    editor.poll();
    wait(&mut editor, |e| e.text.contains("/one"));
    let written = std::fs::read_to_string(&file).unwrap();
    let parsed = http_requests::parse(&written);
    assert_eq!(parsed.requests.len(), 2);
    assert_eq!(parsed.requests[0].url, "https://{{host}}/one?page=2");
    assert_eq!(parsed.requests[1].url, "https://{{host}}/2", "the next one kept");
    assert_eq!(parsed.variables.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_request_goes_at_the_end_of_its_file() {
    let dir = scratch("new");
    let (mut editor, file) = editor(&dir, "### One\nGET https://e.com/1\n", None);
    editor.save();
    wait(&mut editor, |e| e.index.is_some());
    assert_eq!(editor.index, Some(1));
    assert_eq!(http_requests::parse(&std::fs::read_to_string(&file).unwrap()).requests.len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_brace_pair_offers_the_environment_s_names() {
    let dir = scratch("suggest");
    std::fs::write(dir.join(http_requests::env::PUBLIC), r#"{"dev": {"host": "e.com"}}"#).unwrap();
    let (mut editor, _) = editor(&dir, "### One\nGET https://e.com/1\n", Some(0));
    if let Some(data) = &editor.data {
        data.runs.choose_environment(&data.file, Some("dev".into()));
    }
    wait(&mut editor, |e| e.environments.names().count() > 0);
    editor.focus = Some(Focus::Url);
    editor.form.as_mut().unwrap().url.select_all();
    editor.text_input("https://{{ho");
    assert!(editor.picker.is_open(), "suggestions open");
    assert_eq!(editor.suggestions[0].label, "host");
    assert_eq!(editor.suggestions[0].detail, "e.com");
    // ↵ takes the lit one.
    editor.edit_key_with(ViewerKey::Enter, KeyReach::default());
    assert_eq!(editor.form.as_ref().unwrap().url.text(), "https://{{host}}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tables_grow_as_they_are_typed_in_and_select_all_works() {
    let dir = scratch("table");
    let (mut editor, _) = editor(&dir, "### One\nGET https://e.com/1\n", Some(0));
    editor.press_hit(Hit::Tab(Tab::Headers), Point::new(0.0, 0.0), 1);
    let rows = |e: &RequestEditor| e.form.as_ref().unwrap().headers.len();
    assert_eq!(rows(&editor), 1, "an empty one to type in");
    editor.focus = Some(Focus::Cell(Table::Headers, 0, false));
    editor.text_input("Accept");
    assert_eq!(rows(&editor), 2, "a new empty one after it");
    assert!(editor.command("select_all"));
    assert_eq!(editor.selected_text().as_deref(), Some("Accept"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_body_s_brace_pair_offers_names_too() {
    let dir = scratch("body-suggest");
    std::fs::write(dir.join(http_requests::env::PUBLIC), r#"{"dev": {"host": "e.com"}}"#).unwrap();
    let (mut editor, _) = editor(
        &dir,
        "### One\nPOST https://e.com/1\nContent-Type: application/json\n\n{}\n",
        Some(0),
    );
    if let Some(data) = &editor.data {
        data.runs.choose_environment(&data.file, Some("dev".into()));
    }
    wait(&mut editor, |e| e.environments.names().count() > 0);
    editor.focus = Some(Focus::Area(Area::Body));
    editor.form.as_mut().unwrap().body.select_all();
    editor.text_input("{\"to\": \"{{ho");
    assert!(editor.picker.is_open());
    editor.edit_key_with(ViewerKey::Enter, KeyReach::default());
    assert_eq!(editor.form.as_ref().unwrap().body.text(), "{\"to\": \"{{host}}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_request_changed_in_its_file_meanwhile_is_read_again_not_written_over() {
    let dir = scratch("stale");
    let text = "### One\nGET https://e.com/1\n\n### Two\nGET https://e.com/2\n";
    let (mut editor, file) = editor(&dir, text, Some(1));
    // Meanwhile, in the text editor: the request changed, another added before it.
    let outside = "### Zero\nGET https://e.com/0\n\n### One\nGET https://e.com/1\n\n### Two\nGET https://e.com/2b\n";
    std::fs::write(&file, outside).unwrap();
    editor.focus = Some(Focus::Url);
    editor.form.as_mut().unwrap().url.select_all();
    editor.text_input("https://e.com/mine");
    editor.save();
    wait(&mut editor, |e| e.error.is_some());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), outside, "nothing written over");
    // Read again: the file's version of it shown.
    wait(&mut editor, |e| e.form.as_ref().is_some_and(|f| f.url.text() == "https://e.com/2b"));
    assert_eq!(editor.form.as_ref().unwrap().url.text(), "https://e.com/2b");
    // Moved only (unchanged): found where it is now, and that one written.
    let moved = "### Two\nGET https://e.com/2b\n\n### Zero\nGET https://e.com/0\n\n### One\nGET https://e.com/1\n";
    std::fs::write(&file, moved).unwrap();
    editor.load();
    wait(&mut editor, |e| e.index == Some(0));
    assert_eq!(editor.form.as_ref().unwrap().url.text(), "https://e.com/2b");
    editor.focus = Some(Focus::Url);
    editor.form.as_mut().unwrap().url.select_all();
    editor.text_input("https://e.com/two");
    editor.save();
    wait(&mut editor, |e| e.text.contains("/two"));
    let written = http_requests::parse(&std::fs::read_to_string(&file).unwrap());
    let urls: Vec<&str> = written.requests.iter().map(|r| r.url.as_str()).collect();
    assert_eq!(urls, ["https://e.com/two", "https://e.com/0", "https://e.com/1"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// The response's page starts past the splitter's handle (the system draws it over the
/// app's frame): the handle stays whole; a page clear of it is kept as it is.
#[test]
fn the_page_leaves_the_splitter_s_handle_whole() {
    use ui_components::splitter::Orientation;
    let page = Rect::new(500.0, 100.0, 400.0, 600.0);
    let handle = Rect::new(494.0, 380.0, 12.0, 40.0);
    let moved = past_handle(page, handle, Orientation::Vertical);
    assert_eq!((moved.min_x(), moved.max_x()), (506.0, 900.0));
    assert_eq!((moved.min_y(), moved.size.height), (100.0, 600.0));
    let stacked = past_handle(page, Rect::new(680.0, 94.0, 40.0, 12.0), Orientation::Horizontal);
    assert_eq!((stacked.min_y(), stacked.max_y()), (106.0, 700.0));
    assert_eq!(past_handle(page, Rect::new(10.0, 10.0, 12.0, 40.0), Orientation::Vertical), page);
}

/// A list opened from its field has the keys (↓ ↵ choose, what's typed searches it, the URL
/// keeps its text); once it's closed they're the field's again. Its suggestions leave them to
/// the field typed in.
#[test]
fn a_list_opened_from_its_field_has_the_keys_until_it_closes() {
    let dir = scratch("list-keys");
    let (mut editor, _) = editor(&dir, "### One\nGET https://e.com/1\n", Some(0));
    let method = Rect::new(0.0, 0.0, 80.0, 30.0);
    editor.hits = vec![(method, Hit::Method)];
    editor.focus = Some(Focus::Url);
    editor.pointer_down(Point::new(10.0, 10.0), 1, ui_components::PressMods::default());
    editor.release();
    assert!(editor.picker.is_open());
    assert_eq!(editor.controls.focused(), Some(LIST), "the list has the keys");
    assert!(editor.text_input("P"), "typed into its search");
    assert_eq!(editor.form.as_ref().unwrap().url.text(), "https://e.com/1");
    assert!(editor.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(!editor.picker.is_open());
    assert_ne!(editor.controls.focused(), Some(LIST), "the form's again");
    editor.pointer_down(Point::new(10.0, 10.0), 1, ui_components::PressMods::default());
    editor.release();
    assert!(editor.edit_key_with(ViewerKey::Down, KeyReach::default()));
    assert!(editor.edit_key_with(ViewerKey::Enter, KeyReach::default()));
    assert!(!editor.picker.is_open());
    assert_eq!(editor.form.as_ref().unwrap().method, "POST", "the next one chosen");
    // The field still has its caret: the keys reach it.
    assert!(editor.text_input("x"));
    assert!(editor.form.as_ref().unwrap().url.text().contains('x'));
    let _ = std::fs::remove_dir_all(dir);
}

/// The separator between the request and its response is held and dragged (the request's
/// share follows it, within its bounds), with its cursor; an open list over it takes the
/// mouse and the wheel instead.
#[test]
fn the_separator_is_dragged_and_an_open_list_is_over_it() {
    use ui_components::{Orientation, Splitter, SplitterStyle};
    let mut editor = RequestEditor::default();
    let mut splitter = Splitter::new(SplitterStyle::default(), Orientation::Vertical);
    splitter.set_bounds(Rect::new(497.0, 0.0, 6.0, 400.0), 1.0);
    editor.splitter = Some(splitter);
    editor.split_area = Rect::new(0.0, 0.0, 1000.0, 400.0);
    let on_line = Point::new(500.0, 50.0);
    editor.hover(on_line);
    assert_eq!(editor.cursor_at(on_line), Some(ViewerCursor::ResizeHorizontal));
    editor.pointer_down(on_line, 1, ui_components::PressMods::default());
    assert!(editor.drag(Point::new(700.0, 50.0)));
    editor.release();
    assert!((editor.share - 0.7).abs() < 0.01, "{}", editor.share);
    editor.picker.open(vec![PickerItem { label: "GET".into(), ..PickerItem::default() }], None);
    editor.hover(on_line);
    assert_ne!(editor.cursor_at(on_line), Some(ViewerCursor::ResizeHorizontal), "the list's");
    editor.pointer_down(on_line, 1, ui_components::PressMods::default());
    editor.release();
    assert!(!editor.picker.is_open(), "a press outside the list closed it");
    assert!((editor.share - 0.7).abs() < 0.01, "the separator untouched");
}

/// The method is the shared dropdown field (its frame, its chevron) with its value and
/// chevron in the method's color; a press on it opens the shared list, as before.
#[test]
fn the_method_is_the_shared_dropdown_in_its_color() {
    let dir = scratch("method-field");
    let (mut editor, _) = editor(&dir, "### One\nDELETE https://e.com/1\n", Some(0));
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    editor.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1000.0, 600.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let method = editor.hits.iter().find(|(_, h)| *h == Hit::Method).map(|(r, _)| *r).unwrap();
    let own = crate::format::method_color("DELETE", &palette);
    let inside = |p: Point| method.contains(p);
    let glyphs: Vec<_> =
        scene.layers().iter().flat_map(|l| l.glyphs.iter()).filter(|g| inside(g.origin)).collect();
    assert!(glyphs.iter().any(|g| icons::is_icon(g.key)), "its chevron");
    assert!(glyphs.iter().all(|g| g.color == own), "the method's color");
    let framed = scene
        .layers()
        .iter()
        .flat_map(|l| l.quads.iter())
        .any(|q| q.bounds == method && q.border_color == palette.input_border);
    assert!(framed, "the shared field's frame");
    editor.pointer_down(method.center(), 1, ui_components::PressMods::default());
    editor.release();
    assert!(editor.picker.is_open(), "its list opens");
    let _ = std::fs::remove_dir_all(dir);
}
