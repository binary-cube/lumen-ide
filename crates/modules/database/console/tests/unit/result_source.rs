//! A result for the grid: its rows read from the runner's chunks, the next page asked when
//! the grid wants rows past them, sorted and filtered here once every row is here — or by
//! the engine while more are left —, a write never refreshed.

use super::*;
use crate::fake::{Manual, Scripted};
use crate::runner::{RunRequest, RunSettings};
use database::console::{PlanInput, RunMode, RunPlan};
use database::{DataSourceId, console::ConsoleId};
use ui_components::data_grid::SortKey;

fn run_one(
    manual: &Manual,
    rows: u64,
    text: &str,
    limit: u32,
) -> (Runner, Arc<Mutex<Vec<String>>>) {
    let runner = Runner::new(ConsoleId::new(), DataSourceId::new(), manual.runtime());
    let (connector, ran) = Scripted::connector(manual.clock(), rows, false, Default::default());
    let statements = match database::console::plan(&PlanInput {
        text,
        mode: RunMode::Script,
        flavor: database::sql::Flavor::Sqlite,
        ..PlanInput::default()
    }) {
        RunPlan::Run(s) => s,
        _ => Vec::new(),
    };
    assert!(runner.run(RunRequest {
        statements,
        document: text.into(),
        settings: RunSettings { fetch_limit: limit, ..RunSettings::default() },
        connector,
        source_name: "shop".into(),
        detached: false,
    }));
    manual.run_all();
    (runner, ran)
}

fn names(source: &ResultSource, rows: u64) -> Vec<String> {
    (0..rows).map(|r| source.cell(r, 1).display()).collect()
}

#[test]
fn a_complete_result_is_sorted_and_filtered_here() {
    let manual = Manual::new();
    let (runner, ran) = run_one(&manual, 4, "SELECT * FROM users;", 10);
    let id = runner.result_tabs()[0].id;
    let mut source = ResultSource::new(runner, id, manual.runtime());
    assert!(source.complete());
    assert_eq!(source.total(), Total::Known(4));
    assert_eq!(source.columns()[0].name, "id");
    assert_eq!(source.row_state(3), RowState::Ready);
    assert_eq!(source.row_state(4), RowState::Missing);
    let query = GridQuery {
        sort: vec![SortKey { column: "id".into(), descending: true }],
        filter: Some(GridFilter::Compare {
            column: "id".into(),
            op: Compare::Greater,
            value: ui_components::data_grid::CellValue::Int(1),
        }),
    };
    source.request(&query, 0..10);
    assert_eq!(source.status(), SourceStatus::Loading, "worked out in the background");
    manual.run_all();
    assert!(source.poll());
    assert_eq!(source.total(), Total::Known(3));
    assert_eq!(names(&source, 3), ["user 4", "user 3", "user 2"]);
    assert!(!source.by_server());
    assert_eq!(ran.lock().unwrap().len(), 1, "nothing run again");
    // Cleared: its own order back.
    source.request(&GridQuery::default(), 0..10);
    assert_eq!(names(&source, 2), ["user 1", "user 2"]);
}

#[test]
fn rows_past_the_first_page_are_asked_for() {
    let manual = Manual::new();
    let (runner, ran) = run_one(&manual, 5, "SELECT * FROM users;", 2);
    let id = runner.result_tabs()[0].id;
    let mut source = ResultSource::new(runner, id, manual.runtime());
    assert_eq!(source.total(), Total::AtLeast(2), "more left");
    assert_eq!(source.row_state(2), RowState::Pending);
    source.request(&GridQuery::default(), 0..2);
    // Shown: its table's structure read once (whether its rows can be edited), nothing else.
    assert_eq!(manual.jobs(), 1);
    manual.run_all();
    source.request(&GridQuery::default(), 0..2);
    assert_eq!(manual.jobs(), 0, "only when rows past them are wanted");
    source.request(&GridQuery::default(), 0..4);
    source.request(&GridQuery::default(), 0..4);
    manual.run_all();
    assert!(source.poll());
    assert_eq!(source.total(), Total::AtLeast(4));
    assert_eq!(ran.lock().unwrap().len(), 2, "asked once");
    source.request(&GridQuery::default(), 0..6);
    manual.run_all();
    source.poll();
    assert_eq!(source.total(), Total::Known(5));
    assert_eq!(names(&source, 5)[4], "user 5");
}

#[test]
fn while_more_are_left_the_engine_sorts() {
    let manual = Manual::new();
    let (runner, ran) = run_one(&manual, 5, "SELECT * FROM users;", 2);
    let id = runner.result_tabs()[0].id;
    let mut source = ResultSource::new(runner, id, manual.runtime());
    let query = GridQuery {
        sort: vec![SortKey { column: "name".into(), descending: false }],
        filter: None,
    };
    source.request(&query, 0..2);
    manual.run_all();
    source.poll();
    assert!(source.by_server());
    let last = ran.lock().unwrap().last().cloned().unwrap();
    assert!(last.contains("ORDER BY \"name\" ASC LIMIT 3"), "{last}");
}

#[test]
fn a_write_is_never_refreshed() {
    let manual = Manual::new();
    let text = "INSERT INTO users SELECT * FROM old RETURNING *;";
    let (runner, ran) = run_one(&manual, 3, text, 2);
    let id = runner.result_tabs()[0].id;
    let mut source = ResultSource::new(runner, id, manual.runtime());
    assert_eq!(source.total(), Total::Known(2), "its rest lost");
    source.refresh();
    manual.run_all();
    assert!(source.take_notice().is_some());
    assert_eq!(ran.lock().unwrap().len(), 1);
    // A pure query is: run again.
    let (runner, ran) = run_one(&manual, 3, "SELECT * FROM users;", 10);
    let id = runner.result_tabs()[0].id;
    let mut source = ResultSource::new(runner, id, manual.runtime());
    source.refresh();
    manual.run_all();
    assert!(source.take_notice().is_none());
    assert_eq!(ran.lock().unwrap().len(), 2);
}
