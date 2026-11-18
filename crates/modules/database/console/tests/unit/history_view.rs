//! The history window: its statements newest first, searched (what matched lit, how many
//! found), the one chosen inserted into the console (never run), run again apart from the
//! console's text, deleted; read in the background the first time.

use super::*;
use crate::fake::{FakeAccess, FakeHost, Manual, Project};
use database::history::{HistoryStore, MemoryHistory};
use database::sql::{Flavor, ParamStyles};
use workbench_model::EditorHost as _;

/// The statement chosen waits in one place for the whole process (`PICKED_NOW`, as the app
/// has one window's modal at a time): the tests here take turns, or one's choice is taken
/// by another's command.
static TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Setup {
    _turn: std::sync::MutexGuard<'static, ()>,
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<crate::Consoles>,
    host: FakeHost,
    _registered: kernel::Subscription,
}

fn setup(name: &str) -> Setup {
    let turn = TURNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let project = Project::new(name);
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let host = FakeHost::new(consoles.project(), &project.root);
    let registered = crate::consoles::register(Arc::clone(&consoles));
    Setup { _turn: turn, project, manual, access, consoles, host, _registered: registered }
}

fn record(s: &Setup, text: &str, at: i64) -> HistoryEntry {
    let mut e = HistoryEntry::new(
        s.access.shop,
        text,
        Flavor::Sqlite,
        &ParamStyles::of(Flavor::Sqlite),
        at,
    );
    e.duration_ms = Some(28);
    e.rows = Some(9);
    s.consoles.history().record(e.clone());
    e
}

/// The console open in the host, the window opened by its command, its view made with it.
fn window(s: &mut Setup, text: &str) -> (database::console::ConsoleSpec, HistoryView) {
    let spec = s.consoles.create(s.access.shop, text).unwrap();
    s.host.open_file(&spec.path(&s.project.root));
    assert!(crate::commands::history(&mut s.host));
    let request = s.host.histories.last().unwrap().clone();
    assert_eq!((request.console, request.source), (spec.id, s.access.shop));
    assert_eq!(request.source_name, "shop");
    let mut view = HistoryView::default();
    view.set_data(&request);
    (spec, view)
}

#[test]
fn the_statements_are_listed_newest_first_and_searched() {
    let mut s = setup("history-list");
    record(&s, "SELECT * FROM orders WHERE total > 10", 1);
    record(&s, "SELECT * FROM users", 2);
    record(&s, "DELETE FROM orders WHERE id = 3", 3);
    let (_, mut view) = window(&mut s, "");
    let texts = |v: &HistoryView| v.shown().iter().map(|e| e.text.clone()).collect::<Vec<_>>();
    assert_eq!(
        texts(&view),
        [
            "DELETE FROM orders WHERE id = 3",
            "SELECT * FROM users",
            "SELECT * FROM orders WHERE total > 10"
        ]
    );
    assert_eq!(view.chosen().map(|e| e.text.as_str()), Some("DELETE FROM orders WHERE id = 3"));
    // Typed: every word, any case; the first found chosen.
    assert!(view.text_input("ORDERS where"));
    assert_eq!(texts(&view).len(), 2);
    assert!(view.edit_key(ViewerKey::Down));
    assert_eq!(view.chosen().map(|e| e.at_ms), Some(1));
    view.set_query("nothing like it");
    assert!(view.shown().is_empty() && view.chosen().is_none());
    view.set_query("");
    assert_eq!(texts(&view).len(), 3);
    // Run again elsewhere: the list follows (the same statement moved up).
    record(&s, "SELECT * FROM users", 4);
    assert!(view.refresh());
    assert_eq!(texts(&view)[0], "SELECT * FROM users");
    assert_eq!(texts(&view).len(), 3);
}

#[test]
fn enter_inserts_the_statement_into_the_console_never_running_it() {
    let mut s = setup("history-insert");
    record(&s, "SELECT * FROM users", 1);
    let (spec, mut view) = window(&mut s, "SELECT 1;");
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    let (path, text) = s.host.file.clone().unwrap();
    assert_eq!(path, spec.path(&s.project.root));
    assert_eq!(text, "SELECT 1;\n\nSELECT * FROM users;\n");
    let selected = s.host.selected.clone().unwrap();
    assert_eq!(&text[selected], "SELECT * FROM users;");
    assert!(s.consoles.runner_of(spec.id).is_none(), "nothing runs");
}

#[test]
fn run_again_runs_the_statement_apart_from_the_console_s_text() {
    let mut s = setup("history-run");
    record(&s, "UPDATE users SET name = 'zoe' WHERE id = 3", 1);
    let (spec, mut view) = window(&mut s, "SELECT 1;");
    assert!(view.command("run"));
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let last = runner.snapshot().last.unwrap();
    assert!(last.ok, "{last:?}");
    assert!(runner.snapshot().marks.is_empty(), "nothing marked in the console's text");
    assert_eq!(s.host.file.clone().unwrap().1, "SELECT 1;", "the console's text untouched");
    assert_eq!(
        s.project.look("SELECT name FROM users WHERE id = 3"),
        [[database::Value::Text("zoe".into())]]
    );
    assert_eq!(s.consoles.history().entries(s.access.shop).unwrap().len(), 1, "moved up");
}

#[test]
fn a_statement_is_deleted_from_the_history() {
    let mut s = setup("history-delete");
    let store = Arc::new(MemoryHistory::default());
    s.consoles.history().set_store(Arc::clone(&store) as _);
    record(&s, "SELECT 1", 1);
    record(&s, "SELECT 2", 2);
    let (_, mut view) = window(&mut s, "");
    // ⌦ in the field with text after the caret deletes a character, not a statement.
    view.set_query("sel");
    view.edit_key(ViewerKey::Home);
    assert!(view.edit_key(ViewerKey::Delete));
    assert_eq!(view.query(), "el");
    view.set_query("");
    assert!(view.edit_key(ViewerKey::Delete), "at the field's end: the statement chosen");
    assert_eq!(view.shown().iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["SELECT 1"]);
    s.manual.run_all();
    assert_eq!(store.blocks(s.access.shop).len(), 1, "the file written again");
    assert!(view.delete());
    assert!(view.shown().is_empty());
    s.manual.run_all();
    assert!(store.blocks(s.access.shop).is_empty());
}

/// Clear History asks first in the footer ("Cancel" has the keys: ↵ and Esc keep it), then
/// deletes every statement of the source, its file too.
#[test]
fn the_whole_history_is_cleared_once_confirmed() {
    let mut s = setup("history-clear");
    let store = Arc::new(MemoryHistory::default());
    s.consoles.history().set_store(Arc::clone(&store) as _);
    record(&s, "SELECT 1", 1);
    record(&s, "SELECT 2", 2);
    let (_, mut view) = window(&mut s, "");
    s.manual.run_all();
    assert_eq!(store.blocks(s.access.shop).len(), 2);
    // Asked, then kept: ↵ on "Cancel", then Esc (the window stays open).
    assert!(view.command("clear"));
    assert!(view.asking_clear());
    assert!(view.key_hints().is_empty(), "the question's buttons say what the keys do");
    assert!(view.text_input("x"), "typing goes nowhere while it asks");
    assert_eq!(view.query(), "");
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(!view.asking_clear() && !view.closed());
    assert_eq!(view.shown().len(), 2);
    assert!(view.command("clear"));
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(!view.asking_clear());
    assert_eq!(view.take_action(), None, "Esc answered the question, the window stays");
    // Confirmed: Tab to "Clear History", ↵.
    assert!(view.command("clear"));
    assert!(view.edit_key(ViewerKey::Tab));
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(!view.asking_clear() && !view.closed());
    assert!(view.shown().is_empty());
    s.manual.run_all();
    assert!(store.blocks(s.access.shop).is_empty(), "its file deleted too");
    // Nothing left: nothing asked.
    assert!(!view.command("clear"));
    assert!(!view.asking_clear());
}

#[test]
fn the_history_is_read_in_the_background_the_first_time() {
    let mut s = setup("history-read");
    let store = Arc::new(MemoryHistory::default());
    let e = HistoryEntry::new(s.access.shop, "SELECT 42", Flavor::Sqlite, &ParamStyles::all(), 7);
    store.append(s.access.shop, &e).unwrap();
    s.consoles.history().set_store(Arc::clone(&store) as _);
    let (_, mut view) = window(&mut s, "");
    assert!(view.loading() && view.is_loading());
    s.manual.run_all();
    assert!(view.poll());
    assert!(!view.loading());
    assert_eq!(view.shown()[0].text, "SELECT 42");
}

#[test]
fn its_detail_says_when_how_long_and_how_many() {
    let mut s = setup("history-detail");
    s.consoles.set_runtime(s.manual.runtime().with_wall(|| 1_793_440_823_698).frozen());
    let mut e = record(&s, "SELECT * FROM users", 1_793_440_823_698);
    e.schema = Some("main".into());
    let (_, view) = window(&mut s, "");
    let detail = view.detail(&e);
    assert!(detail.contains("28 ms"), "{detail}");
    assert!(detail.contains(&i18n::tr!("console.history_rows", count = 9)), "{detail}");
    assert!(detail.ends_with("main"), "{detail}");
    assert!(detail.starts_with(&s.consoles.runtime().time_text(e.at_ms)), "{detail}");
    let failed = HistoryEntry { outcome: Outcome::Failed, ..e };
    assert!(view.detail(&failed).contains(&t("console.history_failed")));
}

fn paint(view: &mut dyn Viewer, text: &mut text_layout::TextSystem) -> render_api::Scene {
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1600.0, HEIGHT * 2.0),
        scale: 2.0,
        text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    scene
}

#[test]
fn it_draws_many_statements_only_where_they_re_seen() {
    let mut s = setup("history-paint");
    for i in 0..500 {
        record(&s, &format!("SELECT {i} FROM orders\nWHERE id = :id"), i);
    }
    let (_, mut view) = window(&mut s, "");
    let mut text = text_layout::TextSystem::new();
    paint(&mut view, &mut text);
    let rows = view.hits.iter().filter(|(_, h)| matches!(h, Hit::Row(_))).count();
    assert!(rows > 3 && rows < 20, "{rows} rows drawn");
    // The last one chosen: brought into view.
    view.edit_key(ViewerKey::Up);
    paint(&mut view, &mut text);
    assert!(view.scroll > 0.0);
    assert!(view.hits.iter().any(|(_, h)| *h == Hit::Row(499)));
    // Searched, empty, loading: drawn too.
    view.set_query("orders 42");
    paint(&mut view, &mut text);
    view.set_query("nothing");
    paint(&mut view, &mut text);
}
