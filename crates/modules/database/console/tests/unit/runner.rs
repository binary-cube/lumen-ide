//! The runner on a real (temporary) SQLite file, its pool run by hand: nothing connects
//! before a run, the statements run in order with their marks, an error stops a script,
//! cancelling, transactions, the connection closed with the last bar and when idle.

use super::*;
use crate::fake::{FakeAccess, Manual, Project};
use database::console::{ConsoleAccess, Overrides, PlanInput, RunMode, RunPlan};

fn planned(text: &str, mode: RunMode) -> Vec<PlannedStatement> {
    let input = PlanInput { text, mode, flavor: sql::Flavor::Sqlite, ..PlanInput::default() };
    match database::console::plan(&input) {
        RunPlan::Run(s) | RunPlan::Choose(s) => s,
        RunPlan::Nothing => Vec::new(),
    }
}

fn request(access: &FakeAccess, text: &str, settings: RunSettings) -> RunRequest {
    RunRequest {
        statements: planned(text, RunMode::Script),
        document: text.into(),
        settings,
        connector: access.connector(access.shop, &Overrides::default()).unwrap(),
        source_name: "shop".into(),
        detached: false,
    }
}

fn setup(name: &str) -> (Project, Manual, FakeAccess, Runner) {
    let project = Project::new(name);
    let manual = Manual::new();
    let access = FakeAccess::new(&project);
    let runner = Runner::new(ConsoleId::new(), access.shop, manual.runtime());
    (project, manual, access, runner)
}

#[test]
fn nothing_connects_before_the_first_run() {
    let (_project, manual, access, runner) = setup("lazy");
    let lease = runner.lease();
    assert_eq!(manual.jobs(), 0);
    assert!(!runner.connected());
    assert_eq!(access.connections(), 0);
    drop(lease);
    assert_eq!(access.connections(), 0);
}

/// A script's statements one after the other on the console's connection: their marks, the
/// last one's rows, the Output's lines (connected, each statement with its schema, the rows).
#[test]
fn a_script_runs_its_statements_in_order_with_their_marks() {
    let (project, manual, access, runner) = setup("script");
    let _lease = runner.lease();
    let text = "UPDATE users SET name = 'ann' WHERE id = 1;\nSELECT * FROM users;";
    assert!(runner.run(request(&access, text, RunSettings::default())));
    assert!(runner.busy() && runner.snapshot().running.is_some());
    assert!(!runner.run(request(&access, text, RunSettings::default())), "one at a time");
    manual.run_all();
    let snapshot = runner.snapshot();
    assert!(!snapshot.busy && snapshot.running.is_none());
    assert!(snapshot.connected);
    assert_eq!(access.connections(), 1);
    let last = snapshot.last.unwrap();
    assert!(last.ok, "{last:?}");
    assert_eq!((last.done, last.total, last.rows), (2, 2, Some(3)));
    assert_eq!(snapshot.marks.len(), 2);
    assert!(snapshot.marks.iter().all(|m| m.state == MarkState::Done));
    assert_eq!(&text[snapshot.marks[1].range.clone()], "SELECT * FROM users");
    assert_eq!(project.look("SELECT name FROM users WHERE id = 1"), [[Value::Text("ann".into())]]);
    let lines: Vec<String> = runner.log().lines().iter().map(|l| l.plain()).collect();
    assert!(lines[0].contains("shop"), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("main> SELECT * FROM users")), "{lines:?}");
    // The next run reuses the connection.
    assert!(runner.run(request(&access, "SELECT 1;", RunSettings::default())));
    manual.run_all();
    assert_eq!(access.connections(), 1);
}

/// An error marks its statement and stops a script there (when asked), placed in the
/// document; the statements before stay done.
#[test]
fn an_error_stops_the_script_at_its_statement() {
    let (_project, manual, access, runner) = setup("error");
    let _lease = runner.lease();
    let text = "SELECT 1;\nSELEC oops;\nSELECT 2;";
    assert!(runner.run(request(&access, text, RunSettings::default())));
    manual.run_all();
    let snapshot = runner.snapshot();
    let last = snapshot.last.unwrap();
    assert!(!last.ok);
    assert_eq!(last.done, 1);
    let error = last.error.unwrap();
    assert!(error.contains("2"), "the line it's on: {error}");
    let states: Vec<MarkState> = snapshot.marks.iter().map(|m| m.state).collect();
    assert_eq!(states, [MarkState::Done, MarkState::Failed]);
    // Going on past an error when told.
    let settings = RunSettings { stop_on_error: false, ..RunSettings::default() };
    assert!(runner.run(request(&access, text, settings)));
    manual.run_all();
    let states: Vec<MarkState> = runner.snapshot().marks.iter().map(|m| m.state).collect();
    assert_eq!(states, [MarkState::Done, MarkState::Failed, MarkState::Done]);
}

/// Stopped before its statements ran: none runs, said as cancelled.
#[test]
fn a_run_stopped_runs_nothing_more() {
    let (_project, manual, access, runner) = setup("cancel");
    let _lease = runner.lease();
    assert!(!runner.stop(), "nothing runs");
    assert!(runner.run(request(&access, "SELECT 1; SELECT 2;", RunSettings::default())));
    assert!(runner.stop());
    manual.run_all();
    let last = runner.snapshot().last.unwrap();
    assert!(last.cancelled, "{last:?}");
    assert_eq!(last.done, 0);
}

/// Manual: the first write opens a transaction, kept until Commit / Rollback; a `BEGIN`
/// written by hand in Auto too.
#[test]
fn transactions_stay_open_until_committed_or_rolled_back() {
    let (project, manual, access, runner) = setup("tx");
    let _lease = runner.lease();
    let manual_tx = RunSettings { tx: TxMode::Manual, ..RunSettings::default() };
    assert!(runner.run(request(&access, "DELETE FROM orders;", manual_tx)));
    manual.run_all();
    assert!(runner.in_transaction());
    assert_eq!(project.look("SELECT count(*) FROM orders"), [[Value::Int(2)]], "not committed");
    assert!(runner.finish_transaction(false));
    manual.run_all();
    assert!(!runner.in_transaction());
    assert_eq!(project.look("SELECT count(*) FROM orders"), [[Value::Int(2)]]);
    // A BEGIN by hand, then Commit.
    let text = "BEGIN; DELETE FROM orders WHERE id = 1;";
    assert!(runner.run(request(&access, text, RunSettings::default())));
    manual.run_all();
    assert!(runner.in_transaction());
    assert!(runner.idle_due().is_none(), "never closed for idleness with a transaction open");
    assert!(runner.finish_transaction(true));
    manual.run_all();
    assert_eq!(project.look("SELECT count(*) FROM orders"), [[Value::Int(1)]]);
    assert!(!runner.finish_transaction(true), "none open");
}

/// The last bar gone, the connection closes (in the background); a transaction left open
/// goes with it (rolled back by the engine).
#[test]
fn the_connection_closes_with_the_last_bar() {
    let (project, manual, access, runner) = setup("close");
    let (one, two) = (runner.lease(), runner.lease());
    let text = "BEGIN; DELETE FROM users;";
    assert!(runner.run(request(&access, text, RunSettings::default())));
    manual.run_all();
    assert!(runner.connected() && runner.in_transaction());
    drop(one);
    assert!(runner.connected(), "another bar keeps it");
    drop(two);
    assert!(!runner.connected() && !runner.in_transaction());
    manual.run_all();
    assert_eq!(project.look("SELECT count(*) FROM users"), [[Value::Int(3)]], "rolled back");
    // Opened again by the next run.
    let _again = runner.lease();
    assert!(runner.run(request(&access, "SELECT 1;", RunSettings::default())));
    manual.run_all();
    assert_eq!(access.connections(), 2);
}

/// Unused for its idle time, the connection closes; the next run opens it again.
#[test]
fn an_idle_connection_closes() {
    let (_project, manual, access, runner) = setup("idle");
    let _lease = runner.lease();
    runner.set_idle(Some(Duration::from_secs(60)));
    assert!(runner.run(request(&access, "SELECT 1;", RunSettings::default())));
    manual.run_all();
    let due = runner.idle_due().unwrap();
    assert!(!runner.idle_at(due - Duration::from_secs(1)));
    assert!(runner.idle_at(due));
    runner.disconnect();
    manual.run_all();
    assert!(!runner.connected());
}

#[test]
fn the_databases_and_schemas_are_listed() {
    let (_project, manual, access, runner) = setup("places");
    let _lease = runner.lease();
    let connector = access.connector(access.shop, &Overrides::default()).unwrap();
    assert!(runner.list_places(connector, "shop".into()));
    manual.run_all();
    let places = runner.snapshot().places.unwrap();
    assert_eq!(places.schemas, ["main"]);
    assert_eq!(runner.snapshot().place.schema.as_deref(), Some("main"));
}

#[test]
fn a_plan_is_explained_into_the_output() {
    let (_project, manual, access, runner) = setup("explain");
    let _lease = runner.lease();
    let text = "SELECT * FROM users WHERE id = 1";
    let statement = planned(text, RunMode::Playground).remove(0);
    let connector = access.connector(access.shop, &Overrides::default()).unwrap();
    assert!(runner.explain(
        statement,
        false,
        connector,
        "shop".into(),
        sql::Flavor::Sqlite,
        Arc::from(text)
    ));
    manual.run_all();
    let last = runner.snapshot().last.unwrap();
    assert!(last.ok && last.plan_steps.is_some_and(|n| n >= 1), "{last:?}");
    assert!(runner.log().text().contains("EXPLAIN SELECT * FROM users"));
}

/// Explain keeps its profile (what's worked out of it with it), the Plan tab names it, the
/// Output says its summary; Explain Analyze on SQLite is the measured run, a write undone.
#[test]
fn a_profile_is_kept_and_summed_up_in_the_output() {
    let (project, manual, access, runner) = setup("profile");
    let _lease = runner.lease();
    let explain = |text: &str, analyze: bool| {
        let statement = planned(text, RunMode::Playground).remove(0);
        let connector = access.connector(access.shop, &Overrides::default()).unwrap();
        assert!(runner.explain(
            statement,
            analyze,
            connector,
            "shop".into(),
            sql::Flavor::Sqlite,
            Arc::from(text)
        ));
        manual.run_all();
    };
    let generation = runner.profiles_generation();
    explain("SELECT * FROM users", false);
    assert!(runner.profiles_generation() > generation);
    let estimated = runner.latest_profile().unwrap();
    assert!(!estimated.profile.depth.runs());
    assert_eq!(estimated.profile.engine.name, "SQLite");
    assert!(estimated.profile.statement.wall_ms.is_some(), "the runner's clock");
    let tab = runner.result_tabs().into_iter().find(|t| t.plan).unwrap();
    assert_eq!(runner.plan(tab.id).unwrap().profile, Some(estimated.profile.id));
    let output = runner.log().text();
    let summary = i18n::t("console.profile_summary");
    assert!(output.contains(summary.split('{').next().unwrap()), "{output}");

    explain("SELECT * FROM users", true);
    let measured = runner.latest_profile().unwrap();
    assert!(measured.profile.depth.runs());
    assert!(measured.profile.statement.counters.is_some());
    let users = match project.look("SELECT count(*) FROM users")[0][0] {
        database::Value::Int(n) => n as f64,
        _ => unreachable!(),
    };
    assert_eq!(measured.profile.nodes[0].actuals.rows, Some(users), "the scan's real rows");
    let plan = runner.plan(runner.result_tabs().into_iter().find(|t| t.plan).unwrap().id).unwrap();
    assert!(plan.analyze && plan.nodes[0].actual_rows == Some(users));
    let counters = i18n::t("console.profile_counters");
    let fullscan = counters.split('{').next().unwrap();
    assert!(runner.log().text().contains(fullscan), "the engine's counters said");

    // A write measured and undone.
    let before = project.look("SELECT count(*) FROM orders");
    explain("DELETE FROM orders", true);
    assert_eq!(project.look("SELECT count(*) FROM orders"), before, "undone");
    let undone = runner.latest_profile().unwrap();
    assert!(undone.profile.has_note(database::profile::ProfileNote::RolledBack));
    assert!(runner.log().text().contains(&i18n::t("console.profile_rolled_back")));
    let plan = runner.plan(runner.result_tabs().into_iter().find(|t| t.plan).unwrap().id).unwrap();
    assert!(plan.rolled_back);
    // In memory only: counted, cleared with the caches.
    let (count, bytes) = runner.profile_memory();
    assert_eq!(count, 3);
    assert!(bytes > 0 && runner.memory() >= bytes);
    runner.purge();
    assert_eq!(runner.profile_memory().0, 0);
}

/// A profile's analysis (39h3): the facts read on the console's connection, the findings
/// kept with the profile, the Output's count in its summary line, which links to the
/// profile's tab (39h4: the problems are said there, not a line each) — never run.
#[test]
fn a_profiles_problems_are_found_and_said() {
    let (project, manual, access, runner) = setup("analysis");
    let _lease = runner.lease();
    project.look(
        "INSERT INTO orders (user_id, total) \
         SELECT x % 500, x FROM (WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n \
         WHERE x < 5000) SELECT x FROM n)",
    );
    runner.set_analysis(crate::profile::Analysis { production: true, ..Default::default() });
    let text = "SELECT * FROM orders WHERE user_id = 7";
    let statement = planned(text, RunMode::Playground).remove(0);
    let connector = access.connector(access.shop, &Overrides::default()).unwrap();
    assert!(runner.explain(
        statement,
        true,
        connector,
        "shop".into(),
        sql::Flavor::Sqlite,
        Arc::from(text)
    ));
    manual.run_all();
    let report = runner.latest_profile().unwrap();
    let rules: Vec<&str> = report.findings.iter().map(|f| f.rule).collect();
    assert!(rules.contains(&"sqlite-scan"), "{rules:?}");
    let scan = report.findings.iter().find(|f| f.rule == "sqlite-scan").unwrap();
    let sql = "CREATE INDEX \"idx_orders_user_id\" ON \"orders\" (\"user_id\");";
    assert_eq!(
        scan.suggestion.as_ref().and_then(|s| s.sql()),
        Some(sql),
        "SQLite: never CONCURRENTLY"
    );
    assert_eq!(&text[scan.sql_range.clone().unwrap()], "user_id = 7");
    let output = runner.log().text();
    assert!(
        output.contains(&crate::profile::analysis::problems_text(&report.findings)),
        "{output}"
    );
    assert!(!output.contains(sql), "the problems are said in the profile's tab");
    let summary = runner
        .log()
        .lines()
        .into_iter()
        .find(|l| l.text.contains(&crate::profile::analysis::problems_text(&report.findings)))
        .unwrap();
    let tab = runner.result_tabs().into_iter().find(|t| t.plan).unwrap();
    assert_eq!(summary.link, Some(ui_components::ViewerAction::OpenLink(tab.id.link())));
    let set = runner.plan(tab.id).unwrap();
    assert!(set.report.as_ref().is_some_and(|r| Arc::ptr_eq(r, &report)), "the tab shows it");
    assert_eq!(set.document_hash, Some(ui_components::editor_toolbar::text_hash(text)));
    // Said, never run: the table has no such index.
    let indexes = project
        .look("SELECT count(*) FROM sqlite_master WHERE type = 'index' AND tbl_name = 'orders'");
    assert_eq!(indexes, [[Value::Int(0)]]);
}

/// The statements run in a session are counted by their fingerprint: the same query run
/// over and over is the analysis' N+1.
#[test]
fn runs_are_counted_by_fingerprint() {
    let (_project, manual, access, runner) = setup("fingerprints");
    let _lease = runner.lease();
    for i in 0..20 {
        assert!(runner.run(request(
            &access,
            &format!("SELECT * FROM users WHERE id = {i};"),
            RunSettings::default()
        )));
        manual.run_all();
    }
    let window = Duration::from_secs(60);
    assert_eq!(
        runner.recent_runs("select * from users where id = 99", sql::Flavor::Sqlite, window),
        20
    );
    let text = "SELECT * FROM users WHERE id = 5";
    let statement = planned(text, RunMode::Playground).remove(0);
    let connector = access.connector(access.shop, &Overrides::default()).unwrap();
    assert!(runner.explain(
        statement,
        false,
        connector,
        "shop".into(),
        sql::Flavor::Sqlite,
        Arc::from(text)
    ));
    manual.run_all();
    let report = runner.latest_profile().unwrap();
    assert!(report.findings.iter().any(|f| f.rule == "n-plus-one"), "{:?}", report.findings);
}

#[test]
fn times_read_as_milliseconds() {
    assert_eq!(millis(Duration::from_millis(411)), "411 ms");
    assert_eq!(millis(Duration::from_millis(1204)), "1 s 204 ms");
}

/// Passwords never reach the Output.
#[test]
fn passwords_are_hidden_in_the_output() {
    let (_project, manual, access, runner) = setup("secrets");
    let _lease = runner.lease();
    let text = "SELECT 'x' AS password;\nCREATE USER bob PASSWORD 'hunter2';";
    let mut request = request(&access, text, RunSettings::default());
    request.settings.stop_on_error = false;
    assert!(runner.run(request));
    manual.run_all();
    assert!(!runner.log().text().contains("hunter2"), "{}", runner.log().text());
}

// ─── Results (39f) ───

use crate::fake::{Scripted, Told};
use crate::results::More;
use std::sync::Mutex;

/// A run on a scripted session (`rows` rows a query, a cursor when `cursors`), the pool and
/// the clock by hand.
fn scripted(rows: u64, cursors: bool) -> (Manual, Runner, Arc<Mutex<Vec<String>>>, Connector) {
    let manual = Manual::new();
    let runner = Runner::new(ConsoleId::new(), DataSourceId::new(), manual.runtime());
    let (connector, ran) = Scripted::connector(manual.clock(), rows, cursors, Duration::ZERO);
    (manual, runner, ran, connector)
}

fn run_on(runner: &Runner, connector: &Connector, text: &str, settings: RunSettings) -> bool {
    runner.run(RunRequest {
        statements: planned(text, RunMode::Script),
        document: text.into(),
        settings,
        connector: Arc::clone(connector),
        source_name: "shop".into(),
        detached: false,
    })
}

fn limit(n: u32) -> RunSettings {
    RunSettings { fetch_limit: n, ..RunSettings::default() }
}

/// The time until the first row is the execution's, the rest the fetching's, both said in
/// the Output and kept with the result.
#[test]
fn the_output_says_execution_apart_from_fetching() {
    let (manual, runner, _ran, connector) = scripted(89, false);
    let _lease = runner.lease();
    assert!(run_on(&runner, &connector, "SELECT * FROM doctrine_migration_versions;", limit(500)));
    manual.run_all();
    let lines: Vec<String> = runner.log().lines().iter().map(|l| l.plain()).collect();
    let said = i18n::tr!(
        "console.rows_retrieved",
        count = 89,
        from = 1,
        total = "411 ms",
        exec = "43 ms",
        fetch = "368 ms"
    );
    assert!(lines.iter().any(|l| l.ends_with(&said)), "{lines:#?}");
    let tabs = runner.result_tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].title, "shop.public.doctrine_migration_versions");
    let result = runner.result(tabs[0].id).unwrap();
    assert_eq!(result.len, 89);
    assert_eq!(result.more, More::None);
    assert_eq!(result.execution, Duration::from_millis(43));
    assert_eq!(result.fetching, Duration::from_millis(368));
    assert_eq!(runner.take_reveal(), Some(Some(tabs[0].id)), "the result shown");
}

/// A script's queries each bring their result (named after their table, or `Result N`);
/// the next run replaces the unpinned ones, a pinned one stays; closed, its rows go.
#[test]
fn each_result_has_its_tab_and_only_pinned_ones_stay() {
    let (manual, runner, _ran, connector) = scripted(3, false);
    let _lease = runner.lease();
    let text =
        "SELECT * FROM users;\nSELECT id, name FROM users u JOIN orders o ON o.user_id = u.id;";
    assert!(run_on(&runner, &connector, text, limit(500)));
    manual.run_all();
    let titles: Vec<String> = runner.result_tabs().into_iter().map(|t| t.title).collect();
    assert_eq!(
        titles,
        ["shop.public.users".to_owned(), i18n::tr!("console.result_title", number = 2)]
    );
    let first = runner.result_tabs()[0].id;
    runner.pin(first, true);
    assert!(run_on(&runner, &connector, "SELECT * FROM orders;", limit(500)));
    manual.run_all();
    let tabs = runner.result_tabs();
    let titles: Vec<&str> = tabs.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["shop.public.users", "shop.public.orders"], "the pinned one stays");
    assert!(tabs[0].pinned && !tabs[1].pinned);
    let before = runner.result_memory().0;
    runner.close_result(tabs[1].id);
    assert_eq!(runner.result_tabs().len(), 1);
    assert!(runner.result_memory().0 < before, "its rows let go at once");
}

/// Without a cursor, a pure query's next page is read by running it again over its result
/// from the next row; the last page ends it.
#[test]
fn a_pure_query_s_next_page_runs_it_again() {
    let (manual, runner, ran, connector) = scripted(5, false);
    let _lease = runner.lease();
    assert!(run_on(&runner, &connector, "SELECT * FROM users;", limit(2)));
    manual.run_all();
    let id = runner.result_tabs()[0].id;
    assert_eq!(runner.result(id).unwrap().more, More::Rerun);
    assert!(runner.fetch_more(id));
    manual.run_all();
    let last = ran.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last, "SELECT * FROM (\nSELECT * FROM users\n) AS console_result LIMIT 3 OFFSET 2");
    let result = runner.result(id).unwrap();
    assert_eq!(result.len, 4);
    assert!(runner.fetch_more(id));
    manual.run_all();
    let result = runner.result(id).unwrap();
    assert_eq!((result.len, &result.more), (5, &More::None));
    assert!(!runner.fetch_more(id), "nothing left");
    let said = i18n::tr!(
        "console.rows_retrieved",
        count = 1,
        from = 5,
        total = "411 ms",
        exec = "43 ms",
        fetch = "368 ms"
    );
    assert!(runner.log().text().contains(&said), "{}", runner.log().text());
}

/// A cursor kept by the engine gives the next pages; the next statement closes it (the
/// rest then read by running the query again).
#[test]
fn a_cursor_gives_the_next_pages() {
    let (manual, runner, ran, connector) = scripted(5, true);
    let _lease = runner.lease();
    assert!(run_on(&runner, &connector, "SELECT * FROM users;", limit(2)));
    manual.run_all();
    let id = runner.result_tabs()[0].id;
    assert!(matches!(runner.result(id).unwrap().more, More::Cursor(_)));
    assert!(runner.fetch_more(id));
    manual.run_all();
    assert_eq!(ran.lock().unwrap().last().map(String::as_str), Some("FETCH c1 2"));
    assert_eq!(runner.result(id).unwrap().len, 4);
    runner.pin(id, true);
    assert!(run_on(&runner, &connector, "UPDATE users SET name = 'x' WHERE id = 1;", limit(2)));
    manual.run_all();
    assert_eq!(runner.result(id).unwrap().more, More::Rerun, "its cursor closed by the next");
}

/// A statement that wrote is never run again for the rows it left: its rest is lost, said.
#[test]
fn a_write_is_never_run_again() {
    let (manual, runner, ran, connector) = scripted(5, true);
    let _lease = runner.lease();
    let text = "INSERT INTO users (name) SELECT name FROM old_users RETURNING id, name;";
    assert!(run_on(&runner, &connector, text, limit(2)));
    manual.run_all();
    let id = runner.result_tabs()[0].id;
    let result = runner.result(id).unwrap();
    assert_eq!((result.len, &result.more, result.rerunnable), (2, &More::Lost, false));
    assert!(!runner.fetch_more(id));
    assert!(!runner.requery(id, crate::results::ServerView::default()));
    assert!(
        runner
            .export(crate::runner::ExportRequest {
                result: id,
                columns: Vec::new(),
                view: Default::default(),
                format: ui_components::data_grid::ExportFormat::find(None, "csv").unwrap(),
                sql: None,
                options: extractors::Options::default(),
                target: ui_components::data_grid::export::Target::Clipboard,
            })
            .is_none()
    );
    manual.run_all();
    assert_eq!(ran.lock().unwrap().len(), 1, "run once");
    assert!(runner.log().text().contains(&i18n::t("console.rows_left")), "{}", runner.log().text());
}

/// Sorted and filtered by the engine: the query run again wrapped in the order and filter.
#[test]
fn the_engine_sorts_and_filters_a_query_run_again() {
    let (manual, runner, ran, connector) = scripted(5, false);
    let _lease = runner.lease();
    assert!(run_on(&runner, &connector, "SELECT * FROM users;", limit(2)));
    manual.run_all();
    let id = runner.result_tabs()[0].id;
    let view = crate::results::ServerView {
        filter: Some(database::Filter::NotNull("name".into())),
        order: vec![database::Order { column: "name".into(), descending: true }],
    };
    assert!(runner.requery(id, view.clone()));
    manual.run_all();
    assert_eq!(
        ran.lock().unwrap().last().map(String::as_str),
        Some(
            "SELECT * FROM (\nSELECT * FROM users\n) AS console_result WHERE \"name\" IS NOT NULL \
             ORDER BY \"name\" DESC LIMIT 3"
        )
    );
    let result = runner.result(id).unwrap();
    assert_eq!((result.len, &result.view), (2, &view));
}

/// Past the console's memory, the oldest unpinned results let their rows go.
#[test]
fn the_results_memory_is_kept_within_its_limit() {
    let (manual, runner, _ran, connector) = scripted(400, false);
    let _lease = runner.lease();
    let settings =
        RunSettings { fetch_limit: 500, result_memory: 60_000, ..RunSettings::default() };
    let text = "SELECT * FROM a;\nSELECT * FROM b;\nSELECT * FROM c;";
    assert!(run_on(&runner, &connector, text, settings));
    manual.run_all();
    let (bytes, count) = runner.result_memory();
    assert_eq!(count, 3);
    assert!(bytes <= 60_000, "{bytes}");
    let tabs = runner.result_tabs();
    let first = runner.result(tabs[0].id).unwrap();
    let last = runner.result(tabs[2].id).unwrap();
    assert!(first.dropped && first.len == 0, "the oldest let its rows go");
    assert!(!last.dropped && last.len == 400, "the newest kept");
    // Shown again, a pure query's rows come back.
    assert!(runner.requery(first.id, first.view.clone()));
    manual.run_all();
    assert!(!runner.result(first.id).unwrap().dropped);
}

/// An error is a link to its place in the console and underlined there (the project's
/// diagnostics), gone at the next run; the bus hears the run and the tables it wrote.
#[test]
fn errors_link_to_their_place_and_runs_are_told() {
    let (project, manual, access, runner) = setup("events");
    let _lease = runner.lease();
    let events = kernel::EventBus::new();
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let (into, into2) = (Arc::clone(&heard), Arc::clone(&heard));
    let _a = events.subscribe(move |e: &workbench_model::DiagnosticsFound| {
        let d = e.diagnostics.first().map(|d| format!("{}:{:?}", d.line, d.column));
        into.lock().unwrap().push(format!("diagnostics {}", d.unwrap_or_default()));
    });
    let _b = events.subscribe(move |e: &database::events::DataChanged| {
        into2.lock().unwrap().push(format!("changed {}", e.table));
    });
    let file = project.root.join("console.sql");
    runner.set_context(RunnerContext {
        events: Some(events),
        file: Some(file.clone()),
        relative: Some("console.sql".into()),
        ..RunnerContext::default()
    });
    let text = "UPDATE users SET name = 'x' WHERE id = 1;\nSELECT * FORM users;";
    let sqlite = RunSettings { flavor: sql::Flavor::Sqlite, ..RunSettings::default() };
    assert!(runner.run(request(&access, text, sqlite)));
    manual.run_all();
    let lines = runner.log().lines();
    let error = lines.iter().find(|l| l.kind == LogKind::Error).unwrap();
    match &error.link {
        Some(ViewerAction::OpenFile { path, line, column }) => {
            assert_eq!(path, &file);
            assert_eq!(*line, Some(2));
            assert!(column.is_some_and(|c| c > 1), "{column:?}");
        }
        other => panic!("{other:?}"),
    }
    let heard_now = heard.lock().unwrap().clone();
    assert_eq!(heard_now[0], "changed users", "{heard_now:?}");
    assert!(heard_now.iter().any(|h| h.starts_with("diagnostics 1:Some")), "{heard_now:?}");
    assert!(runner.snapshot().error_mark.is_some());
    assert!(runner.run(request(&access, "SELECT 1;", RunSettings::default())));
    manual.run_all();
    assert!(heard.lock().unwrap().iter().any(|h| h == "diagnostics "), "cleared");
    assert!(runner.snapshot().error_mark.is_none());
    let _ = project;
}

/// DDL run says the source's schema is stale (a console's completion reads it again); a
/// query or a write doesn't.
#[test]
fn ddl_makes_the_source_s_schema_stale() {
    let (_project, manual, access, runner) = setup("ddl-stale");
    let _lease = runner.lease();
    let events = kernel::EventBus::new();
    let heard: Arc<Mutex<Vec<DataSourceId>>> = Arc::default();
    let into = Arc::clone(&heard);
    let _s = events.subscribe(move |e: &database::events::SchemaInvalidated| {
        into.lock().unwrap().push(e.source);
    });
    runner.set_context(RunnerContext { events: Some(events), ..RunnerContext::default() });
    let sqlite = RunSettings { flavor: sql::Flavor::Sqlite, ..RunSettings::default() };
    let text = "SELECT * FROM users; UPDATE users SET name = 'x' WHERE id = 1;";
    assert!(runner.run(request(&access, text, sqlite.clone())));
    manual.run_all();
    assert!(heard.lock().unwrap().is_empty());
    assert!(runner.run(request(&access, "CREATE TABLE notes (id INTEGER PRIMARY KEY);", sqlite)));
    manual.run_all();
    assert_eq!(*heard.lock().unwrap(), [runner.source()]);
}

/// What a session makes (a temporary table) and attaches is kept: never an unknown table
/// in its console's checks.
#[test]
fn what_a_session_makes_is_kept_for_the_checks() {
    let (_project, manual, access, runner) = setup("session-made");
    let _lease = runner.lease();
    let sqlite = RunSettings { flavor: sql::Flavor::Sqlite, ..RunSettings::default() };
    assert_eq!(runner.made(), (Vec::new(), false));
    let text = "CREATE TEMP TABLE scratch (a); SELECT * FROM scratch;";
    assert!(runner.run(request(&access, text, sqlite.clone())));
    manual.run_all();
    assert_eq!(runner.made(), (vec![database::TableRef::new("scratch")], false));
    assert!(runner.run(request(&access, "ATTACH DATABASE ':memory:' AS other;", sqlite)));
    manual.run_all();
    assert!(runner.made().1, "attached");
}

/// A write in a transaction is told once it's committed (the grids read what's committed).
#[test]
fn a_write_in_a_transaction_is_told_at_its_commit() {
    let (_project, manual, access, runner) = setup("commit-told");
    let _lease = runner.lease();
    let events = kernel::EventBus::new();
    let heard = Arc::new(Mutex::new(0));
    let into = Arc::clone(&heard);
    let _s = events.subscribe(move |_: &database::events::DataChanged| *into.lock().unwrap() += 1);
    runner.set_context(RunnerContext { events: Some(events), ..RunnerContext::default() });
    let manual_tx = RunSettings { tx: TxMode::Manual, ..RunSettings::default() };
    assert!(runner.run(request(&access, "DELETE FROM orders WHERE id = 1;", manual_tx)));
    manual.run_all();
    assert_eq!(*heard.lock().unwrap(), 0);
    assert!(runner.finish_transaction(true));
    manual.run_all();
    assert_eq!(*heard.lock().unwrap(), 1);
}

/// A long run ended while the Services window is hidden is told; shown, it isn't.
#[test]
fn a_long_run_is_told_when_the_window_is_hidden() {
    let manual = Manual::new();
    let runner = Runner::new(ConsoleId::new(), DataSourceId::new(), manual.runtime());
    let _lease = runner.lease();
    let (connector, _) = Scripted::connector(manual.clock(), 1, false, Duration::from_secs(11));
    let told = Arc::new(Told::default());
    let project = kernel::ProjectId::new();
    runner.set_context(RunnerContext {
        project: Some(project),
        name: "console".into(),
        notifier: told.clone(),
        ..RunnerContext::default()
    });
    assert!(run_on(&runner, &connector, "SELECT 1;", limit(10)));
    manual.run_all();
    let toasts = told.0.lock().unwrap().clone();
    assert_eq!(toasts.len(), 1);
    assert!(toasts[0].title.contains("console"), "{}", toasts[0].title);
    assert_eq!(toasts[0].actions[0].command, workbench_model::TOGGLE_SERVICES);
    ui_components::services::set_shown(project, true);
    assert!(run_on(&runner, &connector, "SELECT 1;", limit(10)));
    manual.run_all();
    assert_eq!(told.0.lock().unwrap().len(), 1, "seen: not told");
    ui_components::services::set_shown(project, false);
}

// ─── Transactions and safety (39g) ───

/// What the project's bus heard of the console's transaction.
fn transactions(
    runner: &Runner,
) -> (kernel::EventBus, Arc<Mutex<Vec<bool>>>, kernel::Subscription) {
    let events = kernel::EventBus::new();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&heard);
    let console = runner.console();
    let subscription = events.subscribe(move |e: &database::events::TransactionChanged| {
        assert_eq!(e.console, console);
        into.lock().unwrap().push(e.open);
    });
    runner.set_context(RunnerContext { events: Some(events.clone()), ..RunnerContext::default() });
    (events, heard, subscription)
}

/// The state machine followed through a run: Manual's first write opens the transaction,
/// writes are counted, Commit ends it; each opening and ending told once on the bus.
#[test]
fn the_transaction_is_followed_and_told() {
    let (_project, manual, access, runner) = setup("tx-state");
    let _lease = runner.lease();
    let (_events, heard, _s) = transactions(&runner);
    let manual_tx = RunSettings { tx: TxMode::Manual, ..RunSettings::default() };
    let text = "SELECT 1;\nDELETE FROM orders WHERE id = 1;\nSELECT 2;\nUPDATE users SET name = 'x' WHERE id = 2;";
    assert!(runner.run(request(&access, text, manual_tx.clone())));
    manual.run_all();
    let snapshot = runner.snapshot();
    assert!(
        matches!(snapshot.tx, TxState::Open { since_statement: 1, writes: 2 }),
        "{:?}",
        snapshot.tx
    );
    assert!(snapshot.in_transaction && snapshot.tx_since.is_some());
    assert_eq!(runner.transaction_statements().len(), 2);
    assert_eq!(*heard.lock().unwrap(), [true], "opened once");
    // Another run inside it: counted on.
    assert!(runner.run(request(&access, "DELETE FROM orders WHERE id = 2;", manual_tx)));
    manual.run_all();
    assert_eq!(runner.tx().pending(), 3);
    assert!(runner.finish_transaction(true));
    manual.run_all();
    assert_eq!(runner.tx(), TxState::Idle);
    assert!(runner.transaction_statements().is_empty());
    assert_eq!(*heard.lock().unwrap(), [true, false]);
    // In Auto nothing is left open, nothing told.
    assert!(runner.run(request(
        &access,
        "DELETE FROM users WHERE id = 3;",
        RunSettings::default()
    )));
    manual.run_all();
    assert_eq!(runner.tx(), TxState::Idle);
    assert_eq!(heard.lock().unwrap().len(), 2);
}

/// A transaction given up (the console closing) or lost with its connection is told ended.
#[test]
fn a_transaction_gone_with_its_connection_is_told_ended() {
    let (project, manual, access, runner) = setup("tx-gone");
    let lease = runner.lease();
    let (_events, heard, _s) = transactions(&runner);
    assert!(runner.run(request(
        &access,
        "BEGIN; DELETE FROM users WHERE id > 0;",
        RunSettings::default()
    )));
    manual.run_all();
    assert!(runner.tx().is_open(), "a BEGIN written by hand in Auto");
    runner.abandon();
    assert_eq!(runner.tx(), TxState::Idle);
    assert_eq!(*heard.lock().unwrap(), [true, false]);
    manual.run_all();
    assert_eq!(project.look("SELECT count(*) FROM users"), [[Value::Int(3)]]);
    drop(lease);
}

/// Never closed for idleness while a transaction or a cursor is open; a transaction open
/// for an hour is told, once.
#[test]
fn an_open_transaction_or_cursor_keeps_the_connection_and_an_old_one_is_told() {
    let (_project, manual, access, runner) = setup("tx-idle");
    let _lease = runner.lease();
    runner.set_idle(Some(Duration::from_secs(60)));
    let told = Arc::new(Told::default());
    runner.set_context(RunnerContext {
        name: "console".into(),
        notifier: told.clone(),
        ..RunnerContext::default()
    });
    let manual_tx = RunSettings { tx: TxMode::Manual, ..RunSettings::default() };
    assert!(runner.run(request(&access, "DELETE FROM orders WHERE id = 1;", manual_tx)));
    manual.run_all();
    let clock = manual.clock();
    let start = *clock.lock().unwrap();
    *clock.lock().unwrap() = start + Duration::from_secs(600);
    assert!(!runner.idle_at(start + Duration::from_secs(600)), "a transaction open");
    assert!(runner.idle_due().is_none());
    let due = runner.long_due().unwrap();
    assert!(!runner.tell_long_transaction(due - Duration::from_secs(1)));
    assert!(runner.tell_long_transaction(due));
    assert!(!runner.tell_long_transaction(due + Duration::from_secs(60)), "told once");
    assert_eq!(told.0.lock().unwrap().len(), 1);
    assert!(told.0.lock().unwrap()[0].title.contains("console"));
    assert!(runner.long_due().is_none());
    assert!(runner.finish_transaction(false));
    manual.run_all();
    assert!(runner.idle_due().is_some(), "closed for idleness again");

    // A cursor kept open by a query's result: never idle either.
    let (manual, runner, _ran, connector) = scripted(30, true);
    let _lease = runner.lease();
    runner.set_idle(Some(Duration::from_secs(60)));
    assert!(run_on(&runner, &connector, "SELECT * FROM t;", limit(10)));
    manual.run_all();
    assert!(runner.idle_due().is_none(), "its cursor holds the connection");
    assert!(!runner.idle_at(Instant::now() + Duration::from_secs(3600)));
}

/// A console's Manual transaction holds the SQLite file: a grid's write on it is refused
/// as `InUse` (the explorer says why: a console's transaction).
#[test]
fn a_console_transaction_locks_the_file_for_the_others() {
    let (project, manual, access, runner) = setup("tx-lock");
    let _lease = runner.lease();
    let manual_tx = RunSettings { tx: TxMode::Manual, ..RunSettings::default() };
    assert!(runner.run(request(&access, "UPDATE users SET name = 'x' WHERE id = 1;", manual_tx)));
    manual.run_all();
    assert!(runner.in_transaction());
    // Another program's connection, refused once its (short) wait for the lock is over.
    let target = database::Target {
        database: project.db().to_string_lossy().into_owned(),
        options: [("busy_timeout_ms".to_owned(), "50".to_owned())].into(),
        ..database::Target::default()
    };
    let mut other =
        database::Driver::connect(&db_sqlite::SqliteDriver, &target, &database::Secret::default())
            .unwrap();
    let asked = std::time::Instant::now();
    let refused = other.execute(
        "UPDATE users SET name = 'y' WHERE id = 2",
        &mut database::Collect::default(),
        &database::Cancel::default(),
    );
    assert!(matches!(refused, Err(DbError::InUse(_))), "{refused:?}");
    assert!(asked.elapsed() < db_sqlite::BUSY_TIMEOUT, "the target's wait, not the default");
    assert!(runner.finish_transaction(true));
    manual.run_all();
    let ok = other.execute(
        "UPDATE users SET name = 'y' WHERE id = 2",
        &mut database::Collect::default(),
        &database::Cancel::default(),
    );
    assert!(ok.is_ok(), "free once committed: {ok:?}");
}

/// A write refused before it's sent is said in the Output.
#[test]
fn a_refusal_is_said_in_the_output() {
    let (_project, _manual, _access, runner) = setup("refused");
    runner.refused("shop is read-only".into());
    assert!(runner.log().text().contains("shop is read-only"));
    assert_eq!(runner.take_reveal(), Some(None), "the Output shown");
}

/// What was confirmed once stays confirmed for the console's session.
#[test]
fn an_acknowledgement_is_kept_for_the_session() {
    let (_project, _manual, _access, runner) = setup("ack");
    assert!(!runner.acknowledged("profile-production"));
    runner.acknowledge("profile-production");
    runner.acknowledge("profile-production");
    assert!(runner.acknowledged("profile-production"));
    assert!(!runner.acknowledged("other"));
}

/// A plan replaces the unpinned plan, keeps a pinned one and the results; a run replaces the
/// unpinned plans too.
#[test]
fn plans_are_tabs_replaced_as_results_are() {
    let (_project, manual, access, runner) = setup("plans");
    let _lease = runner.lease();
    let explain = |text: &str| {
        let statement = planned(text, RunMode::Playground).remove(0);
        let connector = access.connector(access.shop, &Overrides::default()).unwrap();
        assert!(runner.explain(
            statement,
            false,
            connector,
            "shop".into(),
            sql::Flavor::Sqlite,
            Arc::from(text)
        ));
        manual.run_all();
    };
    explain("SELECT * FROM users");
    let first = runner.result_tabs();
    assert_eq!(first.len(), 1);
    assert!(first[0].plan && first[0].title.contains("users"));
    explain("SELECT * FROM orders");
    let second = runner.result_tabs();
    assert_eq!(second.len(), 1, "the unpinned plan replaced");
    runner.pin(second[0].id, true);
    explain("SELECT 1");
    assert_eq!(runner.result_tabs().len(), 2, "the pinned one kept");
    assert_eq!(runner.result_tabs()[1].title, i18n::t("profile.view.title"));
    assert!(runner.memory() > 0);
    // A failing plan: its error placed in the console's text, no tab.
    explain("SELECT * FROM users WHER id = 1");
    assert_eq!(runner.result_tabs().len(), 1);
    assert!(runner.snapshot().last.unwrap().error.is_some());
}

/// Each statement run is recorded in its source's history (as written, where it ran, how
/// long, its rows, how it ended); one the engine refuses too; a statement run apart from the
/// console's text (the history's, again) leaves no mark in the gutter.
#[test]
fn each_statement_run_is_recorded_in_the_source_s_history() {
    use database::history::{MemoryHistory, Outcome};
    let (_project, manual, access, runner) = setup("history");
    let store = Arc::new(MemoryHistory::default());
    let histories = Arc::new(crate::history::Histories::new(
        Arc::clone(&store) as Arc<dyn database::history::HistoryStore>,
        manual.runtime().with_wall(|| 1_793_440_823_698),
    ));
    runner.set_context(RunnerContext {
        history: Some(Arc::clone(&histories)),
        ..RunnerContext::default()
    });
    let _lease = runner.lease();
    let text = "UPDATE users SET name = 'ann' WHERE id = 1;\nSELECT * FROM users;\nSELEC oops;";
    let settings = RunSettings { stop_on_error: false, ..RunSettings::default() };
    assert!(runner.run(request(&access, text, settings.clone())));
    manual.run_all();
    let blocks = store.blocks(access.shop);
    let texts: Vec<&str> = blocks.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        texts,
        ["UPDATE users SET name = 'ann' WHERE id = 1", "SELECT * FROM users", "SELEC oops"]
    );
    assert_eq!((blocks[0].rows, blocks[0].outcome), (Some(1), Outcome::Ok));
    assert_eq!((blocks[1].rows, blocks[1].outcome), (Some(3), Outcome::Ok));
    assert_eq!(blocks[2].outcome, Outcome::Failed);
    // Stamped by the runner's clock (the system's here).
    assert!(
        blocks.iter().all(|e| e.console == Some(runner.console()) && e.at_ms > 1_700_000_000_000)
    );
    assert!(blocks.iter().all(|e| e.duration_ms.is_some()));
    assert_eq!(blocks[1].schema.as_deref(), Some("main"));
    // The same statement again moves up (a block appended, one entry).
    let mut again = request(&access, "SELECT * FROM users;", settings);
    again.detached = true;
    assert!(runner.run(again));
    manual.run_all();
    assert!(runner.snapshot().marks.is_empty(), "a detached run marks nothing");
    let newest: Vec<String> =
        histories.entries(access.shop).unwrap().into_iter().map(|e| e.text).collect();
    assert_eq!(newest[0], "SELECT * FROM users");
    assert_eq!(newest.len(), 3);
    assert_eq!(store.blocks(access.shop).len(), 4);
}
