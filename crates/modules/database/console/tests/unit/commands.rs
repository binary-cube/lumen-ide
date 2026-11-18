//! The console's commands through a host holding the console's file: Run on the cursor,
//! the selection or the script; which statement asked when none is under the cursor; a
//! read-only source's write refused; a source without SQL refused; "Jump to Query
//! Console…" making the first console (a table's query in it) or listing the source's;
//! the tab closing with a transaction open asking first; Explain Analyze of a write asked.

use super::*;
use crate::fake::{FakeAccess, FakeHost, Manual, Project};
use database::TableRef;
use database::console::TxMode;
use std::sync::Mutex;
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

/// The console's file open in the host, its cursor at `cursor`.
fn open(s: &mut Setup, text: &str, cursor: usize) -> ConsoleSpec {
    let spec = s.consoles.create(s.access.shop, text).unwrap();
    s.host.open_file(&spec.path(&s.project.root));
    s.host.cursor = cursor;
    // The list written.
    s.manual.run_all();
    spec
}

/// A SQL file of the project (39n) open in the host, its cursor at `cursor`.
fn open_project_file(s: &mut Setup, name: &str, text: &str, cursor: usize) -> std::path::PathBuf {
    let path = s.project.root.join(name);
    std::fs::write(&path, text).unwrap();
    s.host.open_file(&path);
    s.host.cursor = cursor;
    path
}

/// A project's SQL file attached to nothing: Run (⌘↵) asks which source first,
/// then runs on it — the file now attached, a console's commands enabled on it;
/// detached, it's attached to nothing again. Nothing runs before a source is chosen.
#[test]
fn run_on_a_project_s_sql_file_asks_for_its_source_first() {
    let mut s = setup("attach-run", |a| a.archive = Some(DataSourceId::new()));
    open_project_file(&mut s, "report.sql", "DELETE FROM orders WHERE id = 1;", 5);
    assert!(active(&s.host).is_none(), "attached to nothing");
    let run_handler = OnSqlFile(|h| run(h, false));
    assert!(run_handler.enabled(&s.host), "Run asks for a source");
    assert!(!OnConsole(commands_explain).enabled(&s.host), "Explain waits for one");
    assert!(AttachCommand.enabled(&s.host) && !DetachCommand.enabled(&s.host));
    assert!(run_handler.run(&mut s.host));
    assert_eq!(s.host.chooses.len(), 1);
    let names: Vec<&str> = s.host.chooses[0].items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(names, ["shop", "archive"], "the sources speaking SQL");
    assert_eq!(s.manual.jobs(), 0, "nothing runs before it's chosen");
    s.host.pick(0);
    s.manual.run_all();
    let (_, spec) = active(&s.host).expect("attached");
    assert!(spec.is_attached());
    assert_eq!((spec.source, spec.name.as_str()), (s.access.shop, "report.sql"));
    let last = s.consoles.runner_of(spec.id).unwrap().snapshot().last.unwrap();
    assert_eq!(last.affected, Some(1));
    assert_eq!(s.project.look("SELECT count(*) FROM orders"), [[database::Value::Int(1)]]);
    // Attached: ⌘↵ runs at once.
    assert!(run_handler.run(&mut s.host));
    assert_eq!(s.host.chooses.len(), 1, "not asked again");
    s.manual.run_all();
    // Attach again: the source it's on marked; another one chosen.
    assert!(AttachCommand.run(&mut s.host));
    let items = &s.host.chooses[1].items;
    assert_eq!(items[0].detail, i18n::t("console.attached_now"));
    s.host.pick(1);
    let (_, moved) = active(&s.host).unwrap();
    assert_eq!((moved.id, moved.source), (spec.id, s.access.archive.unwrap()));
    // Detached.
    assert!(DetachCommand.enabled(&s.host));
    assert!(DetachCommand.run(&mut s.host));
    assert!(active(&s.host).is_none() && !DetachCommand.enabled(&s.host));
}

/// A file whose transaction is open is neither detached nor attached elsewhere (said).
#[test]
fn an_open_transaction_keeps_its_file_attached() {
    let mut s = setup("attach-tx", |_| {});
    let path = open_project_file(&mut s, "w.sql", "DELETE FROM orders WHERE id = 1;", 3);
    let spec = s.consoles.attach(&path, s.access.shop).unwrap();
    s.consoles.update(spec.id, |c| c.tx = TxMode::Manual);
    assert!(run(&mut s.host, false));
    s.manual.run_all();
    assert!(s.consoles.runner_of(spec.id).unwrap().in_transaction());
    assert!(!DetachCommand.run(&mut s.host));
    assert!(!AttachCommand.run(&mut s.host));
    assert_eq!(s.host.told.len(), 2, "{:?}", s.host.told);
    assert!(s.host.told.iter().all(|n| n.title == i18n::t("console.tx_open_attach")));
    assert!(active(&s.host).is_some(), "still attached");
}

/// No SQL file of the project (a console's, another kind): nothing to attach.
#[test]
fn only_a_project_s_sql_files_are_attached() {
    let mut s = setup("attach-not", |_| {});
    open(&mut s, "SELECT 1;", 0);
    assert!(!AttachCommand.enabled(&s.host), "a console has its source");
    open_project_file(&mut s, "notes.txt", "SELECT 1;", 0);
    assert!(!AttachCommand.enabled(&s.host) && !OnSqlFile(|h| run(h, false)).enabled(&s.host));
}

#[test]
fn run_runs_the_statement_under_the_cursor() {
    let mut s = setup("run", |_| {});
    let text = "SELECT 1;\nDELETE FROM orders WHERE id = 1;";
    let spec = open(&mut s, text, text.len());
    assert!(active(&s.host).is_some());
    assert!(run(&mut s.host, false));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let last = runner.snapshot().last.unwrap();
    assert!(last.ok && last.total == 1, "{last:?}");
    assert_eq!(last.affected, Some(1));
    assert_eq!(s.project.look("SELECT count(*) FROM orders"), [[database::Value::Int(1)]]);
    // The whole script.
    assert!(run(&mut s.host, true));
    s.manual.run_all();
    assert_eq!(runner.snapshot().last.unwrap().total, 2);
    assert_eq!(s.access.connections(), 1, "the console's own connection, kept");
}

#[test]
fn a_selection_runs_what_s_selected() {
    let mut s = setup("selection", |_| {});
    let text = "SELECT 1;\nSELECT 2;\nSELECT 3;";
    let spec = open(&mut s, text, 0);
    s.host.selection = Some(10..text.len());
    assert!(run(&mut s.host, false));
    s.manual.run_all();
    let snapshot = s.consoles.runner_of(spec.id).unwrap().snapshot();
    assert_eq!(snapshot.last.unwrap().total, 2);
    let marked: Vec<&str> = snapshot.marks.iter().map(|m| &text[m.range.clone()]).collect();
    assert_eq!(marked, ["SELECT 2", "SELECT 3"]);
}

/// No statement under the cursor: which one, asked.
#[test]
fn which_statement_is_asked_when_none_is_under_the_cursor() {
    let mut s = setup("choose", |_| {});
    let text = "SELECT 1;\n\n\n\nSELECT 2;";
    let spec = open(&mut s, text, 12);
    assert!(run(&mut s.host, false));
    assert_eq!(s.host.chooses.len(), 1);
    assert_eq!(s.host.chooses[0].items.len(), 2);
    assert_eq!(s.manual.jobs(), 0, "nothing runs before it's chosen");
    s.host.pick(1);
    s.manual.run_all();
    let marks = s.consoles.runner_of(spec.id).unwrap().snapshot().marks;
    assert_eq!(&text[marks[0].range.clone()], "SELECT 2");
    // Nothing at all: said.
    let empty = s.consoles.create(s.access.shop, "-- nothing").unwrap();
    s.host.open_file(&empty.path(&s.project.root));
    assert!(!run(&mut s.host, false));
    assert_eq!(s.host.told.last().unwrap().title, i18n::t("console.nothing_to_run"));
}

#[test]
fn a_read_only_source_refuses_writes_before_they_re_sent() {
    let mut s = setup("read-only", |a| a.read_only = true);
    open(&mut s, "DELETE FROM users;", 0);
    assert!(!run(&mut s.host, false));
    assert_eq!(s.manual.jobs(), 0);
    assert_eq!(s.access.connections(), 0, "nothing sent");
    assert_eq!(s.project.look("SELECT count(*) FROM users"), [[database::Value::Int(3)]]);
    assert!(!s.host.told.is_empty());
}

/// A source without SQL has no console: refused, with why.
#[test]
fn a_source_without_sql_is_refused() {
    let mut s = setup("no-sql", |_| {});
    let request = JumpRequest {
        project: s.consoles.project(),
        source: s.access.people,
        schema: None,
        table: None,
        new: false,
        list: false,
        database: None,
    };
    assert!(!open_for(&mut s.host, &s.consoles, request));
    assert!(s.host.opened.is_empty());
    assert_eq!(s.host.told.last().unwrap().title, i18n::tr!("console.no_sql", name = "people"));
}

/// Asked on another database of the source's server (the "Database" window's): a console
/// made there; asked there again, only the consoles opened there are listed.
#[test]
fn a_console_asked_on_another_database_opens_there() {
    let mut s = setup("on-database", |_| {});
    let project = s.consoles.project();
    let on = |database: Option<&str>, new| JumpRequest {
        project,
        source: s.access.shop,
        schema: Some("sales".into()),
        table: None,
        new,
        list: false,
        database: database.map(str::to_owned),
    };
    // One on the source's own database first.
    assert!(open_for(&mut s.host, &s.consoles, on(None, true)));
    assert!(open_for(&mut s.host, &s.consoles, on(Some("analytics"), false)));
    assert!(s.host.chooses.is_empty(), "none there yet: made at once");
    let specs = s.consoles.consoles_of(s.access.shop);
    let there: Vec<_> =
        specs.iter().filter(|c| c.database.as_deref() == Some("analytics")).collect();
    assert_eq!(there.len(), 1, "{specs:?}");
    assert_eq!(there[0].schema.as_deref(), Some("sales"));
    // Asked there again: its consoles only (and a new one).
    assert!(open_for(&mut s.host, &s.consoles, on(Some("analytics"), false)));
    let list = s.host.chooses.last().map(|l| l.items.len());
    let overlay = s.host.overlays;
    assert!(overlay || list == Some(2), "the new one and the one there: {list:?}");
}

/// Asked on a table, with no console yet: the first one made at once, opened, the table's
/// rows in it with its name selected; asked again: the source's consoles listed, a new one
/// first.
#[test]
fn jump_makes_the_first_console_then_lists_them() {
    let mut s = setup("jump", |_| {});
    let project = s.consoles.project();
    database::console::ask_jump(JumpRequest {
        project,
        source: s.access.shop,
        schema: None,
        table: Some(TableRef::new("users")),
        new: false,
        list: false,
        database: None,
    });
    assert!(jump(&mut s.host, false));
    assert!(s.host.chooses.is_empty(), "none yet: made at once");
    let (path, text) = s.host.file.clone().unwrap();
    assert_eq!(text, "SELECT * FROM users;");
    assert_eq!(s.host.selected.clone().map(|r| text[r].to_owned()), Some("users".into()));
    assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("console.sql"));
    // From the console's own tab: its source's consoles, a new one first.
    assert!(jump(&mut s.host, false));
    let list = s.host.chooses.last().unwrap();
    let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, [i18n::t("console.new_console").as_str(), "console"]);
    s.host.pick(0);
    let (path, _) = s.host.file.clone().unwrap();
    assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("console_1.sql"));
    // "New Query Console": a new one whatever the source has.
    assert!(jump(&mut s.host, true));
    let (path, _) = s.host.file.clone().unwrap();
    assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("console_2.sql"));
    // From the palette with nothing asked: the Database window's selection.
    s.host.file = None;
    *s.access.selected.lock().unwrap() = Some(JumpRequest {
        project,
        source: s.access.shop,
        schema: None,
        table: None,
        new: false,
        list: false,
        database: None,
    });
    assert!(jump(&mut s.host, false));
    assert_eq!(s.host.chooses.last().unwrap().items.len(), 4);
}

/// An existing empty console opened on a table gets its rows' query.
#[test]
fn an_empty_console_opened_on_a_table_gets_its_query() {
    let mut s = setup("existing", |_| {});
    let spec = s.consoles.create(s.access.shop, "").unwrap();
    let request = JumpRequest {
        project: s.consoles.project(),
        source: s.access.shop,
        schema: Some("main".into()),
        table: Some(TableRef::in_schema("main", "Order Items")),
        new: false,
        list: false,
        database: None,
    };
    assert!(open_for(&mut s.host, &s.consoles, request));
    s.host.pick(1);
    let (_, text) = s.host.file.clone().unwrap();
    assert_eq!(text, "SELECT * FROM main.\"Order Items\";");
    assert_eq!(s.consoles.spec(spec.id).unwrap().schema.as_deref(), Some("main"));
}

/// A tab closing with a transaction open: asked (Commit, Roll Back or keep it), then rolled
/// back and closed.
#[test]
fn a_tab_closing_with_a_transaction_open_asks_then_rolls_back() {
    let _alone = closing_alone();
    let mut s = setup("close", |_| {});
    let spec = open(&mut s, "BEGIN; DELETE FROM users WHERE id > 0;", 0);
    assert!(run(&mut s.host, true));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let _lease = runner.lease();
    assert!(runner.in_transaction());
    assert!(close(&mut s.host, spec.id));
    assert_eq!(s.host.confirms.len(), 1);
    let asked = s.host.confirms[0].clone();
    assert_eq!(asked.kind, crate::confirm::ConfirmKind::CloseTransaction);
    assert_eq!(asked.statements, ["DELETE FROM users WHERE id > 0"], "what Commit would keep");
    assert!(runner.in_transaction(), "nothing done before the answer");
    assert!(s.host.answer(crate::confirm::Answer::Rollback));
    assert!(!runner.in_transaction());
    assert_eq!(s.host.commands, [workbench_model::CLOSE_TAB]);
    assert!(s.consoles.take_allowed_close(spec.id), "the tab may close now");
    assert!(!s.consoles.take_allowed_close(spec.id), "once");
    s.manual.run_all();
    assert_eq!(s.project.look("SELECT count(*) FROM users"), [[database::Value::Int(3)]]);
}

/// Committed on the way out: what it wrote is kept.
#[test]
fn a_tab_closing_may_commit_first() {
    let _alone = closing_alone();
    let mut s = setup("close-commit", |_| {});
    let spec = open(&mut s, "BEGIN; DELETE FROM users WHERE id = 1;", 0);
    assert!(run(&mut s.host, true));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let lease = runner.lease();
    assert!(close(&mut s.host, spec.id));
    assert!(s.host.answer(crate::confirm::Answer::Commit));
    assert_eq!(s.host.commands, [workbench_model::CLOSE_TAB]);
    // The tab goes (its lease with it) while the commit runs: it still commits.
    drop(lease);
    s.manual.run_all();
    assert!(!runner.in_transaction());
    assert_eq!(s.project.look("SELECT count(*) FROM users"), [[database::Value::Int(2)]]);
}

#[test]
fn explain_analyze_of_a_write_asks_first() {
    let mut s = setup("analyze", |_| {});
    open(&mut s, "DELETE FROM users WHERE id = 1;", 0);
    explain(&mut s.host, true);
    assert_eq!(s.host.confirms.len(), 1);
    assert_eq!(s.host.confirms[0].kind, crate::confirm::ConfirmKind::Analyze);
    assert!(s.host.confirms[0].note.is_some(), "what isn't undone");
    open(&mut s, "SELECT * FROM users;", 0);
    explain(&mut s.host, false);
    assert_eq!(s.host.confirms.len(), 1, "a plan alone runs nothing");
}

/// Explain Analyze of a read on production is asked once per console's session (it loads
/// the server): confirmed, it runs and isn't asked again; a write is asked every time.
#[test]
fn profiling_a_read_on_production_is_asked_once_per_console() {
    let mut s = setup("profile-production", |a| a.production = true);
    let spec = open(&mut s, "SELECT * FROM users;", 0);
    assert!(explain(&mut s.host, true));
    assert_eq!(s.host.confirms.len(), 1);
    let asked = s.host.confirms[0].clone();
    assert_eq!(asked.kind, crate::confirm::ConfirmKind::Profile);
    assert_eq!(asked.once, Some((spec.id, PROFILE_ON_PRODUCTION)));
    assert!(asked.production && asked.needs_tick());
    assert!(asked.reasons[0].contains("300"), "its time limit said: {:?}", asked.reasons);
    assert!(s.host.answer(crate::confirm::Answer::Go));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    assert!(runner.latest_profile().is_some_and(|p| p.profile.depth.runs()));
    // Again: not asked.
    assert!(explain(&mut s.host, true));
    assert_eq!(s.host.confirms.len(), 1, "asked once");
    s.manual.run_all();
    assert_eq!(runner.profile_memory().0, 2);
    // A plan alone: nothing asked; a write: always, as Explain Analyze's.
    assert!(explain(&mut s.host, false));
    assert_eq!(s.host.confirms.len(), 1);
    s.manual.run_all();
    open(&mut s, "DELETE FROM users WHERE id = 1;", 0);
    explain(&mut s.host, true);
    assert_eq!(s.host.confirms.len(), 2);
    assert_eq!(s.host.confirms[1].kind, crate::confirm::ConfirmKind::Analyze);
}

/// What can't run inside the transaction that would undo it isn't profiled as it runs:
/// refused before anything is sent, in a toast and in the Output.
#[test]
fn what_cant_be_undone_isnt_profiled_as_it_runs() {
    let mut s = setup("profile-vacuum", |_| {});
    let spec = open(&mut s, "VACUUM;", 0);
    assert!(!explain(&mut s.host, true));
    assert!(s.host.confirms.is_empty());
    assert_eq!(s.host.told.len(), 1);
    let runner = s.consoles.runner(spec.id, spec.source);
    assert!(runner.log().text().contains("VACUUM"), "{}", runner.log().text());
    assert_eq!(s.access.connections(), 0, "nothing connected");
}

/// A read-only source never gets Explain Analyze of a write, even undone.
#[test]
fn a_read_only_source_refuses_analyzing_a_write() {
    let mut s = setup("analyze-ro", |a| a.read_only = true);
    let spec = open(&mut s, "DELETE FROM users WHERE id = 1;", 0);
    assert!(!explain(&mut s.host, true));
    assert!(s.host.confirms.is_empty());
    assert_eq!(s.host.told.len(), 1);
    let runner = s.consoles.runner_of(spec.id).unwrap();
    assert!(runner.log().text().contains("DELETE FROM users"), "said in the Output");
}

/// Dangers are confirmed before anything is sent: given up, nothing runs; confirmed, it
/// runs as asked.
#[test]
fn a_dangerous_statement_is_confirmed_first() {
    let mut s = setup("danger", |_| {});
    let spec = open(&mut s, "DELETE FROM orders;", 0);
    assert!(run(&mut s.host, false));
    assert_eq!(s.host.confirms.len(), 1);
    let asked = s.host.confirms[0].clone();
    assert_eq!(asked.kind, crate::confirm::ConfirmKind::Run);
    assert_eq!(asked.statements, ["DELETE FROM orders"]);
    assert_eq!(asked.reasons.len(), 1);
    assert!(asked.reasons[0].contains("orders"), "{:?}", asked.reasons);
    assert!(!asked.production && !asked.needs_tick());
    assert!(s.consoles.runner_of(spec.id).is_none_or(|r| !r.busy()), "nothing sent yet");
    s.manual.run_all();
    assert_eq!(s.project.look("SELECT count(*) FROM orders"), [[database::Value::Int(2)]]);
    // Given up (the modal closed without an answer): nothing.
    let mut view = crate::confirm::ConfirmView::default();
    ui_components::Viewer::set_data(&mut view, &asked);
    view.cancel();
    drop(view);
    assert!(!s.host.answer(crate::confirm::Answer::Go), "forgotten");
    s.manual.run_all();
    assert_eq!(s.project.look("SELECT count(*) FROM orders"), [[database::Value::Int(2)]]);
    // Asked again and confirmed: it runs.
    assert!(run(&mut s.host, false));
    assert!(s.host.answer(crate::confirm::Answer::Go));
    s.manual.run_all();
    assert_eq!(s.project.look("SELECT count(*) FROM orders"), [[database::Value::Int(0)]]);
}

/// On production every write in Tx: Auto is confirmed (with "I understand"); new consoles
/// start in Manual, where nothing is permanent before Commit and nothing is asked.
#[test]
fn production_writes_are_confirmed_and_consoles_start_manual() {
    let mut s = setup("production", |a| a.production = true);
    let spec = open(&mut s, "UPDATE users SET name = 'x' WHERE id = 1;", 0);
    assert_eq!(spec.tx, TxMode::Manual, "production: Manual by default");
    assert!(run(&mut s.host, false));
    assert!(s.host.confirms.is_empty(), "waits for Commit: nothing asked");
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    assert!(runner.in_transaction());
    assert!(runner.finish_transaction(false));
    s.manual.run_all();
    // In Auto it's asked, "I understand" first.
    s.consoles.update(spec.id, |c| c.tx = TxMode::Auto);
    assert!(run(&mut s.host, false));
    assert_eq!(s.host.confirms.len(), 1);
    let asked = &s.host.confirms[0];
    assert!(asked.production && asked.needs_tick());
    assert_eq!(asked.reasons.len(), 1, "a write made permanent at once: {:?}", asked.reasons);
    // A read on production: nothing asked.
    let read = open(&mut s, "SELECT * FROM users;", 0);
    s.consoles.update(read.id, |c| c.tx = TxMode::Auto);
    assert!(run(&mut s.host, false));
    assert_eq!(s.host.confirms.len(), 1);
    s.manual.run_all();
    // Elsewhere, Auto.
    let mut other = setup("not-production", |_| {});
    assert_eq!(open(&mut other, "", 0).tx, TxMode::Auto);
}

/// A read-only source refuses a write before it's sent: said in a toast and in the Output.
#[test]
fn a_read_only_source_refuses_a_write_in_the_output_too() {
    let mut s = setup("read-only", |a| a.read_only = true);
    let spec = open(&mut s, "UPDATE users SET name = 'x' WHERE id = 1;", 0);
    assert!(!run(&mut s.host, false));
    assert!(s.host.confirms.is_empty(), "no modal: refused");
    assert_eq!(s.host.told.len(), 1);
    let runner = s.consoles.runner_of(spec.id).unwrap();
    assert!(runner.log().text().contains("UPDATE users"), "{}", runner.log().text());
    assert_eq!(s.access.connections(), 0, "nothing connected");
}

/// A question asked once per console's session (39h2's profile on production) isn't asked
/// again once confirmed; another console's is.
#[test]
fn a_question_asked_once_is_remembered_by_its_console() {
    use crate::confirm::{Answer, ConfirmKind, ConfirmRequest, ask};
    let mut s = setup("once", |_| {});
    let spec = open(&mut s, "SELECT 1;", 0);
    let runner = s.consoles.runner(spec.id, spec.source);
    let ran = Arc::new(Mutex::new(0));
    let question = || {
        let mut request = ConfirmRequest::new(ConfirmKind::Profile, "console", "shop");
        request.production = true;
        request.once = Some((spec.id, "profile-production"));
        request
    };
    let count = |ran: &Arc<Mutex<i32>>| {
        let ran = Arc::clone(ran);
        Box::new(move |_: &mut dyn WorkbenchHost, a: Answer| {
            assert_eq!(a, Answer::Go);
            *ran.lock().unwrap() += 1;
        }) as crate::confirm::OnAnswer
    };
    assert!(ask(&mut s.host, question(), count(&ran)));
    assert_eq!(s.host.confirms.len(), 1);
    assert_eq!(*ran.lock().unwrap(), 0);
    assert!(s.host.answer(Answer::Go));
    assert_eq!(*ran.lock().unwrap(), 1);
    assert!(runner.acknowledged("profile-production"));
    assert!(ask(&mut s.host, question(), count(&ran)));
    assert_eq!(s.host.confirms.len(), 1, "not asked again");
    assert_eq!(*ran.lock().unwrap(), 2);
}

/// The bar's ⚙ choices are saved as settings.
#[test]
fn the_bar_s_settings_change() {
    let mut s = setup("settings", |_| {});
    assert!(save_setting(&mut s.host, SettingChange::FetchLimit(1000)));
    assert_eq!(crate::settings::fetch_limit(), 1000);
    crate::settings::configure_fetch(500);
}

#[test]
fn names_are_quoted_when_they_need_it() {
    let flavor = database::sql::Flavor::Sqlite;
    assert_eq!(quote("users", flavor), "users");
    assert_eq!(quote("Users", flavor), "\"Users\"");
    assert_eq!(quote("a \"b\"", flavor), "\"a \"\"b\"\"\"");
}

/// A run shows the Services window (its Output and results are there).
#[test]
fn a_run_shows_the_services_window() {
    let mut s = setup("show-services", |_| {});
    open(&mut s, "SELECT 1;", 0);
    assert!(run(&mut s.host, false));
    assert_eq!(s.host.shown, [workbench_model::SERVICES_TOOL]);
    s.manual.run_all();
}

/// "Open in Services": the console's session made (nothing connected) with its Output to
/// show, and the Services window shown; outside a console it does nothing.
#[test]
fn open_in_services_shows_the_console_s_session() {
    let mut s = setup("services", |_| {});
    assert!(!open_in_services(&mut s.host), "no console open");
    assert!(s.host.shown.is_empty());
    let spec = open(&mut s, "SELECT 1;", 0);
    assert!(s.consoles.runner_of(spec.id).is_none(), "no session before it's asked");
    assert!(open_in_services(&mut s.host));
    assert_eq!(s.host.shown, [workbench_model::SERVICES_TOOL]);
    let runner = s.consoles.runner_of(spec.id).expect("its session, in the Services tree");
    assert_eq!(runner.take_reveal(), Some(None), "its Output shown");
    assert_eq!(s.access.connections(), 0, "nothing connected for it");
}

// ─── Parameters (39h) ───

/// Every form of parameter is asked for, written in by SQLite's quoting, and runs.
#[test]
fn every_form_of_parameter_is_asked_then_written_in() {
    for (text, value) in [
        ("SELECT name FROM users WHERE id = ?;", "2"),
        ("SELECT name FROM users WHERE id = ?1;", "2"),
        ("SELECT name FROM users WHERE id = $1;", "2"),
        ("SELECT name FROM users WHERE id = :id;", "2"),
        ("SELECT name FROM users WHERE id = @id;", "2"),
        ("SELECT name FROM users WHERE id = $id;", "2"),
        ("SELECT name FROM users WHERE name = :name;", "bob"),
    ] {
        let mut s = setup("param-forms", |_| {});
        let spec = open(&mut s, text, 0);
        assert!(run(&mut s.host, false), "{text}: asked");
        assert_eq!(s.host.params.len(), 1, "{text}");
        let asked = &s.host.params[0];
        assert_eq!(asked.entries.len(), 1);
        assert_eq!(asked.statements, [text.trim_end_matches(';')]);
        assert_eq!(asked.places.len(), 1);
        assert!(s.host.give_texts(&[value]));
        s.manual.run_all();
        let runner = s.consoles.runner_of(spec.id).unwrap();
        let last = runner.snapshot().last.unwrap();
        assert!(last.ok && last.rows == Some(1), "{text}: {last:?}");
        let log = runner.log().text();
        let sent = if value == "bob" { "name = 'bob'" } else { "id = 2" };
        assert!(log.contains(sent), "{text}: what ran is in the Output: {log}");
        // The history keeps the statement as written (its parameters, never their values).
        let kept = s.consoles.history().entries(s.access.shop).unwrap();
        assert_eq!(
            kept.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            [text.trim_end_matches(';')]
        );
        assert!(!kept[0].params.is_empty() && !kept[0].text.contains(value), "{text}");
    }
}

/// Closed without values: nothing runs.
#[test]
fn parameters_given_up_run_nothing() {
    let mut s = setup("param-cancel", |_| {});
    let spec = open(&mut s, "DELETE FROM users WHERE id = :id;", 0);
    run(&mut s.host, false);
    assert_eq!(s.host.params.len(), 1);
    let mut view = crate::params::ParamsView::default();
    ui_components::Viewer::set_data(&mut view, &s.host.params[0].clone());
    view.cancel();
    assert!(!s.host.give_texts(&["1"]), "forgotten");
    s.manual.run_all();
    assert!(s.consoles.runner_of(spec.id).is_none_or(|r| r.snapshot().last.is_none()));
    assert_eq!(s.project.look("SELECT count(*) FROM users"), [[database::Value::Int(3)]]);
}

/// The values are kept for the console's session (in memory: the console's list keeps the
/// names only), asked again with them; one not to remember is forgotten.
#[test]
fn values_are_remembered_in_memory_only() {
    let mut s = setup("param-memory", |_| {});
    let text = "SELECT * FROM users WHERE id = :id AND name <> :secret;";
    let spec = open(&mut s, text, 0);
    run(&mut s.host, false);
    let mut entries = s.host.params[0].entries.clone();
    entries[0].text = "1".into();
    entries[1].text = "hunter2-personal".into();
    entries[1].remember = false;
    assert!(s.host.give(entries));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let kept = runner.remembered_params();
    assert_eq!(kept.len(), 1);
    assert_eq!((kept[0].text.as_str(), kept[0].kind), ("1", crate::params::ValueKind::Text));
    assert_eq!(s.consoles.spec(spec.id).unwrap().params, [":id", ":secret"]);
    // Asked again: the kept value is there, the other empty.
    run(&mut s.host, false);
    let again = &s.host.params[1].entries;
    assert_eq!((again[0].text.as_str(), again[1].text.as_str()), ("1", ""));
    // Nothing of the values on disk.
    s.manual.run_all();
    for entry in walk(&s.project.root) {
        let Ok(content) = std::fs::read_to_string(&entry) else { continue };
        assert!(!content.contains("hunter2"), "{entry:?}");
        if entry.extension().is_some_and(|e| e == "toml") {
            assert!(!content.contains("\"1\""), "{entry:?}: {content}");
        }
    }
    let kept = s.consoles.history().entries(s.access.shop).unwrap();
    assert!(!kept.is_empty());
    assert!(!kept.iter().any(|h| h.text.contains("hunter2")));
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() { out.extend(walk(&path)) } else { out.push(path) }
    }
    out
}

/// An engine's error in a statement with values written in is placed in the user's text.
#[test]
fn an_error_after_values_lands_in_the_user_s_text() {
    let mut s = setup("param-error", |_| {});
    let text = "SELECT * FROM users\nWHERE id = :id AND name = :name AND ((;";
    let spec = open(&mut s, text, 0);
    run(&mut s.host, false);
    assert!(s.host.give_texts(&["1", "a much longer value than its name"]));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let error = runner.snapshot().last.unwrap().error.unwrap();
    // SQLite places the syntax error at `;`: line 2, column 39 of the user's text (not of
    // the text sent, longer by the values).
    let place = i18n::tr!("console.error_at", message = "", line = 2, column = 39);
    assert!(error.ends_with(place.trim_start()), "{error}");
}

/// Explain asks the values too; its plan opens in a Plan tab; the dangers' confirmation shows
/// what's sent.
#[test]
fn explain_asks_values_and_shows_its_plan() {
    let mut s = setup("param-explain", |_| {});
    let spec = open(&mut s, "SELECT * FROM users WHERE id = :id;", 0);
    explain(&mut s.host, false);
    assert_eq!(s.host.params.len(), 1);
    assert_eq!(s.host.params[0].purpose, crate::params::Purpose::Explain);
    assert!(s.host.give_texts(&["2"]));
    s.manual.run_all();
    let runner = s.consoles.runner_of(spec.id).unwrap();
    let tabs = runner.result_tabs();
    assert_eq!(tabs.len(), 1);
    assert!(tabs[0].plan);
    assert_eq!(tabs[0].title, i18n::tr!("profile.view.title_of", table = "users"));
    assert!(runner.log().text().contains("EXPLAIN SELECT * FROM users WHERE id = 2"));
    // A dangerous statement with values: the confirmation shows it as sent.
    open(&mut s, "DELETE FROM users WHERE id > :min OR 1 = 1;", 0);
    run(&mut s.host, false);
    assert!(s.host.give_texts(&["0"]));
    assert_eq!(
        s.host.confirms.last().unwrap().statements,
        ["DELETE FROM users WHERE id > 0 OR 1 = 1"]
    );
}

/// The bar's `(p)`: the values edited and kept, nothing run; a console without parameters
/// says so.
#[test]
fn the_parameters_are_edited_without_running() {
    let mut s = setup("param-edit", |_| {});
    let spec = open(&mut s, "SELECT 1;\nSELECT * FROM users WHERE id = :id;", 0);
    assert!(edit_params(&mut s.host));
    assert_eq!(s.host.params[0].purpose, crate::params::Purpose::Edit);
    assert!(s.host.give_texts(&["3"]));
    assert_eq!(s.manual.run_all(), 1, "the list written, nothing run");
    let runner = s.consoles.runner_of(spec.id).unwrap();
    assert!(runner.snapshot().last.is_none());
    assert_eq!(runner.remembered_params()[0].text, "3");
    open(&mut s, "SELECT 1;", 0);
    assert!(!edit_params(&mut s.host));
    assert_eq!(s.host.params.len(), 1);
}

fn commands_explain(host: &mut dyn WorkbenchHost) -> bool {
    explain(host, false)
}

/// One test at a time among those closing a console (what's asked is the process's).
fn closing_alone() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
