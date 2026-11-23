//! A console's result edited and submitted on a real (temporary) SQLite file: what can be
//! edited and why not, the changes shown first then run as shown (Tx: Auto committed at
//! once; Tx: Manual inside the console's transaction until its Commit), a conflict keeping
//! every change and the transaction, a read-only source refusing, production asking "I
//! understand" only for what's permanent at once, a big cell read whole, the row a key
//! points to and the result's table opened.

use std::sync::Mutex;

use super::*;
use crate::confirm::Answer;
use crate::consoles::Consoles;
use crate::fake::{FakeAccess, FakeHost, Manual, Project};
use crate::result_source::ResultSource;
use crate::result_view::ResultView;
use database::Value;
use database::console::{ConsoleSpec, TxMode};
use ui_components::Viewer as _;
use ui_components::data_grid::{CellRef, CellValue, Editing, GridQuery, GridSource, Total, Whole};
use workbench_model::EditorHost as _;

struct Setup {
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<Consoles>,
    host: FakeHost,
    _registered: kernel::Subscription,
}

fn setup(name: &str, access: impl FnOnce(&mut FakeAccess)) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let mut fake = FakeAccess::new(&project);
    access(&mut fake);
    let access = Arc::new(fake);
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let host = FakeHost::new(consoles.project(), &project.root);
    let registered = crate::consoles::register(Arc::clone(&consoles));
    Setup { project, manual, access, consoles, host, _registered: registered }
}

/// A console in `tx` mode, its file open, `text` run as a script.
fn console(s: &mut Setup, text: &str, tx: TxMode) -> (ConsoleSpec, Runner) {
    let spec = s.consoles.create(s.access.shop, text).unwrap();
    s.consoles.update(spec.id, |c| c.tx = tx);
    s.host.open_file(&spec.path(&s.project.root));
    s.manual.run_all();
    assert!(crate::commands::run(&mut s.host, true));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    (spec, runner)
}

/// The last result's source, shown (its table's structure read).
fn shown(s: &Setup, runner: &Runner) -> ResultSource {
    let tab = runner.result_tabs().last().cloned().expect("a result");
    let mut source = ResultSource::new(runner.clone(), tab.id, s.manual.runtime());
    source.request(&GridQuery::default(), 0..50);
    s.manual.run_all();
    source.poll();
    source
}

/// The source's changes asked to be submitted (the confirmation opened).
fn ask(s: &mut Setup, runner: &Runner, source: &ResultSource) -> bool {
    let asked = SubmitAsk {
        runner: runner.clone(),
        result: source.id(),
        set: source.change_set().expect("changes"),
        editable: Arc::clone(source.editable().expect("editable")),
    };
    submit(&mut s.host, asked)
}

fn names(s: &Setup) -> Vec<Vec<Value>> {
    s.project.look("SELECT id, name FROM users ORDER BY id")
}

fn row(id: i64, name: &str) -> Vec<Value> {
    vec![Value::Int(id), Value::Text(name.into())]
}

fn heard(consoles: &Consoles) -> (Arc<Mutex<Vec<String>>>, kernel::Subscription) {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&heard);
    let subscription = consoles.events().subscribe(move |e: &database::events::DataChanged| {
        into.lock().unwrap().push(e.table.to_string());
    });
    (heard, subscription)
}

#[test]
fn a_tables_rows_are_edited_and_submitted_at_once_in_auto() {
    let mut s = setup("edit-auto", |_| {});
    let (heard, _sub) = heard(&s.consoles);
    let (_spec, runner) = console(&mut s, "SELECT * FROM users;", TxMode::Auto);
    let mut source = shown(&s, &runner);
    assert_eq!(source.editing(), Editing::Allowed);
    assert_eq!(source.can_edit(), source.editing().allowed(), "can_edit agrees");
    assert!(source.columns()[0].key && !source.columns()[1].key);
    assert!(!source.columns()[1].nullable, "NOT NULL, as the table says");
    // A cell changed, a row added, a row deleted.
    source.set(0, 1, CellValue::Text("zoe".into())).unwrap();
    assert_eq!(source.cell(0, 1), CellRef::Text("zoe"));
    let new = source.insert(None).unwrap();
    source.set(new, 1, CellValue::Text("dan".into())).unwrap();
    assert!(source.is_default(new, 0), "its key left to the engine");
    source.delete(&[2]);
    assert_eq!(source.pending(), 3);
    assert!(ask(&mut s, &runner, &source));
    let confirm = s.host.confirms.last().unwrap().clone();
    assert_eq!(confirm.kind, crate::confirm::ConfirmKind::Submit);
    assert!(!confirm.pending && !confirm.needs_tick(), "not production");
    assert_eq!(confirm.statements.len(), 3);
    assert!(confirm.statements[0].starts_with("DELETE FROM \"users\" WHERE"), "{confirm:?}");
    assert!(confirm.statements[1].contains("SET \"name\" = 'zoe'"), "{confirm:?}");
    assert!(confirm.statements[1].contains("\"name\" = 'ana'"), "the value read checked");
    assert!(confirm.statements[2].starts_with("INSERT INTO \"users\""), "{confirm:?}");
    assert_eq!(s.manual.jobs(), 0, "nothing runs before it's confirmed");
    assert_eq!(names(&s).len(), 3);
    assert!(s.host.answer(Answer::Go));
    s.manual.run_all();
    assert!(source.poll());
    assert_eq!(source.take_submitted().map(|r| r.map(|a| a.affected)), Some(Ok(3)));
    assert_eq!(source.pending(), 0, "applied: nothing waits");
    assert_eq!(names(&s), [row(1, "zoe"), row(2, "bob"), row(3, "dan")]);
    assert_eq!(runner.tx(), database::console::TxState::Idle, "committed at once");
    assert_eq!(*heard.lock().unwrap(), ["users"], "its grids told");
    // Its rows read again (its query; nothing written run again).
    source.poll();
    assert_eq!(source.total(), Total::Known(3));
    assert_eq!(source.cell(2, 1), CellRef::Text("dan"));
    let output: Vec<String> = runner.log().lines().iter().map(|l| l.text.clone()).collect();
    assert!(output.iter().any(|l| l.starts_with("UPDATE \"users\"")), "{output:?}");
}

#[test]
fn what_can_t_be_edited_says_why() {
    let mut s = setup("edit-refused", |_| {});
    let cases = [
        ("SELECT name FROM users;", i18n::tr!("console.edit_key_missing", columns = "id")),
        ("SELECT count(*) FROM users;", i18n::tr!("console.edit_aggregate", name = "COUNT")),
        ("SELECT DISTINCT name FROM users;", i18n::t("console.edit_distinct")),
        (
            "SELECT u.name, o.total FROM users u JOIN orders o ON o.user_id = u.id;",
            i18n::t("console.edit_not_one_table"),
        ),
        (
            "UPDATE users SET name = name WHERE id = 1 RETURNING *;",
            i18n::t("console.edit_not_select"),
        ),
    ];
    for (text, why) in cases {
        let (_spec, runner) = console(&mut s, text, TxMode::Auto);
        assert!(!runner.result_tabs().is_empty(), "{text}: {:?}", runner.snapshot().last);
        let mut source = shown(&s, &runner);
        assert_eq!(source.editing(), Editing::ReadOnly(why), "{text}");
        assert_eq!(source.can_edit(), source.editing().allowed(), "can_edit agrees");
        assert!(source.set(0, 0, CellValue::Int(9)).is_err());
        assert_eq!(source.insert(None), None);
    }
    // A view's rows have no key.
    let (_spec, runner) = console(
        &mut s,
        "CREATE VIEW people AS SELECT * FROM users; SELECT * FROM people;",
        TxMode::Auto,
    );
    let source = shown(&s, &runner);
    assert_eq!(source.editing(), Editing::ReadOnly(i18n::t("console.edit_no_table_key")));
    assert_eq!(source.can_edit(), source.editing().allowed(), "can_edit agrees");
    // An expression's column isn't written; the others are.
    let (_spec, runner) = console(&mut s, "SELECT id, upper(name) FROM users;", TxMode::Auto);
    let mut source = shown(&s, &runner);
    assert_eq!(source.editing(), Editing::Allowed);
    assert_eq!(source.can_edit(), source.editing().allowed(), "can_edit agrees");
    assert!(!source.columns()[1].editable);
    assert!(source.set(0, 1, CellValue::Text("x".into())).is_err());
}

#[test]
fn a_read_only_source_s_result_is_never_edited() {
    let mut s = setup("edit-ro", |a| a.read_only = true);
    let (_spec, runner) = console(&mut s, "SELECT * FROM users;", TxMode::Auto);
    let mut view = {
        let tab = runner.result_tabs()[0].id;
        let source = ResultSource::new(runner.clone(), tab, s.manual.runtime());
        ResultView::new(source, Some(500), None)
    };
    view.poll();
    let source = view.grid_mut().source_mut();
    source.request(&GridQuery::default(), 0..10);
    s.manual.run_all();
    source.poll();
    assert_eq!(source.editing(), Editing::ReadOnly(i18n::t("console.edit_ro_source")));
    assert_eq!(source.can_edit(), source.editing().allowed(), "can_edit agrees");
    assert!(
        runner.result(runner.result_tabs()[0].id).unwrap().editing
            == crate::results::Editability::Unknown,
        "nothing read for it"
    );
}

#[test]
fn in_manual_the_changes_join_the_transaction_and_a_conflict_keeps_everything() {
    let mut s = setup("edit-manual", |a| a.production = true);
    let (heard, _sub) = heard(&s.consoles);
    let (spec, runner) = console(&mut s, "SELECT * FROM users;", TxMode::Manual);
    let tab = runner.result_tabs()[0].id;
    runner.pin(tab, true);
    let mut source = shown(&s, &runner);
    source.set(0, 1, CellValue::Text("x".into())).unwrap();
    assert!(ask(&mut s, &runner, &source));
    let confirm = s.host.confirms.last().unwrap().clone();
    assert!(confirm.production && confirm.pending, "{confirm:?}");
    assert!(!confirm.needs_tick(), "nothing permanent before Commit: no tick");
    assert!(s.host.answer(Answer::Go));
    s.manual.run_all();
    source.poll();
    assert!(source.take_submitted().unwrap().is_ok());
    assert!(
        matches!(runner.tx(), database::console::TxState::Open { writes: 1, .. }),
        "{:?}",
        runner.tx()
    );
    assert_eq!(runner.transaction_statements().len(), 1);
    assert!(heard.lock().unwrap().is_empty(), "told at Commit");
    // Another statement of the console changes a row the result shows as it was.
    assert!(crate::commands::run_text(
        &mut s.host,
        spec.id,
        "UPDATE users SET name = 'changed' WHERE id = 2;"
    ));
    s.manual.run_all();
    source.poll();
    assert_eq!(source.cell(1, 1), CellRef::Text("bob"), "pinned: as read");
    source.set(1, 1, CellValue::Text("y".into())).unwrap();
    assert!(ask(&mut s, &runner, &source));
    assert!(s.host.answer(Answer::Go));
    s.manual.run_all();
    source.poll();
    assert!(
        matches!(source.take_submitted(), Some(Err(database::DbError::Conflict(_)))),
        "the row changed since it was read"
    );
    assert_eq!(source.pending(), 1, "every change kept");
    assert!(runner.tx().is_open(), "the transaction kept (only the savepoint rolled back)");
    assert_eq!(runner.tx().pending(), 2);
    assert!(runner.finish_transaction(true));
    s.manual.run_all();
    assert_eq!(names(&s), [row(1, "x"), row(2, "changed"), row(3, "cid")]);
    assert!(heard.lock().unwrap().iter().any(|t| t == "users"), "told once committed");
}

#[test]
fn on_production_in_auto_i_understand_is_asked_first() {
    let mut s = setup("edit-prod", |a| a.production = true);
    let (_spec, runner) = console(&mut s, "SELECT * FROM users;", TxMode::Auto);
    let mut source = shown(&s, &runner);
    source.delete(&[0]);
    assert!(ask(&mut s, &runner, &source));
    let confirm = s.host.confirms.last().unwrap().clone();
    assert!(confirm.needs_tick(), "permanent at once on production");
    assert!(confirm.reasons.iter().any(|r| *r == i18n::tr!("console.submit_deletes", count = 1)));
    // Not answered (×, Esc): nothing runs.
    s.manual.run_all();
    assert_eq!(names(&s).len(), 3);
    assert_eq!(source.pending(), 1);
}

#[test]
fn a_row_found_by_its_row_id_is_edited() {
    let mut s = setup("edit-rowid", |_| {});
    let (_spec, runner) = console(
        &mut s,
        "CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('a'), ('b'); SELECT rowid, body FROM notes;",
        TxMode::Auto,
    );
    let mut source = shown(&s, &runner);
    assert_eq!(source.editing(), Editing::Allowed);
    assert_eq!(source.can_edit(), source.editing().allowed(), "can_edit agrees");
    assert!(source.columns()[0].key && !source.columns()[0].editable);
    source.set(1, 1, CellValue::Text("B".into())).unwrap();
    assert!(ask(&mut s, &runner, &source));
    assert!(s.host.confirms.last().unwrap().statements[0].contains("rowid = 2"));
    assert!(s.host.answer(Answer::Go));
    s.manual.run_all();
    source.poll();
    assert!(source.take_submitted().unwrap().is_ok());
    assert_eq!(
        s.project.look("SELECT body FROM notes ORDER BY rowid"),
        [[Value::Text("a".into())], [Value::Text("B".into())]]
    );
}

#[test]
fn a_big_cell_is_read_whole_before_it_s_edited() {
    let mut s = setup("edit-big", |_| {});
    let big = "x".repeat(crate::results::CELL_LIMIT * 2);
    let text = format!(
        "CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO docs VALUES (1, '{big}'); SELECT * FROM docs;"
    );
    let (_spec, runner) = console(&mut s, &text, TxMode::Auto);
    let mut source = shown(&s, &runner);
    assert!(matches!(source.cell(0, 1), CellRef::Text(t) if t.len() < big.len()), "kept cut");
    assert!(source.set(0, 1, CellValue::Text("y".into())).is_err(), "not before it's whole");
    assert_eq!(source.whole(0, 1), Whole::Coming);
    assert_eq!(source.whole(0, 0), Whole::Here, "a small cell is whole");
    s.manual.run_all();
    assert!(source.poll());
    assert_eq!(source.whole(0, 1), Whole::Here);
    assert!(matches!(source.cell(0, 1), CellRef::Text(t) if t == big), "whole now");
    source.set(0, 1, CellValue::Text("short".into())).unwrap();
    assert!(ask(&mut s, &runner, &source));
    assert!(s.host.answer(Answer::Go));
    s.manual.run_all();
    source.poll();
    assert!(source.take_submitted().unwrap().is_ok(), "the whole value compared, no conflict");
    assert_eq!(s.project.look("SELECT body FROM docs"), [[Value::Text("short".into())]]);
}

#[test]
fn a_key_s_cell_goes_to_its_row_and_the_table_opens() {
    let mut s = setup("edit-ref", |_| {});
    let (_spec, runner) = console(
        &mut s,
        "CREATE TABLE items (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id)); INSERT INTO items VALUES (1, 2); SELECT * FROM items;",
        TxMode::Auto,
    );
    let tab = runner.result_tabs().last().unwrap().id;
    let mut view =
        ResultView::new(ResultSource::new(runner, tab, s.manual.runtime()), Some(500), None);
    view.poll();
    view.grid_mut().source_mut().request(&GridQuery::default(), 0..10);
    s.manual.run_all();
    view.poll();
    assert!(view.command("grid:select:0,0"));
    assert_eq!(view.reference(), None, "not a key's cell");
    assert!(view.command("grid:select:0,1"));
    assert_eq!(
        view.reference(),
        Some((database::TableRef::new("users"), "id".into(), "id = 2".into()))
    );
    assert!(view.command("goto-ref"));
    assert_eq!(
        *s.access.rows_opened.lock().unwrap(),
        [(database::TableRef::new("users"), "id = 2".to_owned())]
    );
    assert!(matches!(view.take_action(), Some(ui_components::ViewerAction::WithHost(_))));
    assert!(view.command("open-table"));
    assert_eq!(s.access.opened.lock().unwrap().last().unwrap().0, database::TableRef::new("items"));
    assert_eq!(view.actions_view(), Some(VIEW));
}

#[test]
fn the_changes_kinds_are_in_the_order_they_run() {
    use database::{Change, ChangeSet, RowKey, TableRef};
    let mut set = ChangeSet::new(TableRef::new("t"));
    set.changes.push(Change::Insert { values: Vec::new() });
    set.changes.push(Change::Delete { key: RowKey::RowId(1) });
    assert_eq!(
        kinds(&set),
        [StatementKind::Write(WriteKind::Delete), StatementKind::Write(WriteKind::Insert)]
    );
}
