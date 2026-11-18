//! A result's tab: its export (every row, in the order shown: from the rows here, or a pure
//! query run again for its rest; a write's only what it brought), its end told.

use super::*;
use crate::fake::{Manual, Scripted};
use crate::runner::{RunRequest, RunSettings};
use database::DataSourceId;
use database::console::{ConsoleId, PlanInput, RunMode, RunPlan};

fn view(
    manual: &Manual,
    rows: u64,
    text: &str,
    limit: u32,
) -> (ResultView, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
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
    let id = runner.result_tabs()[0].id;
    let source = ResultSource::new(runner, id, manual.runtime());
    (ResultView::new(source, Some(limit), None), ran)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "console-export-{name}-{}-{}",
        std::process::id(),
        kernel::id::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_complete_result_is_exported_from_its_rows() {
    let manual = Manual::new();
    let (mut view, ran) = view(&manual, 3, "SELECT * FROM users;", 10);
    let dir = scratch("complete");
    let file = dir.join("users.csv");
    // The file asked for, then chosen.
    assert!(view.command("grid:export:csv"));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::PickSavePath { ref name, .. })) if name == "users.csv"
    ));
    assert!(view.command(&format!("picked-save:{}", file.display())));
    assert!(view.exporting());
    manual.run_all();
    assert!(view.poll());
    assert!(!view.exporting());
    let told = view.take_notifications();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].kind, workbench_model::NotificationKind::Success);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "id,name\n1,user 1\n2,user 2\n3,user 3\n");
    assert_eq!(ran.lock().unwrap().len(), 1, "nothing run again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_query_with_more_left_is_run_again_for_its_export() {
    let manual = Manual::new();
    let (mut view, ran) = view(&manual, 5, "SELECT * FROM users;", 2);
    let dir = scratch("rerun");
    let file = dir.join("all.json");
    view.start_export(ExportFormat::find(None, "csv").unwrap(), Target::Clipboard);
    let told = view.take_notifications();
    assert_eq!(told.len(), 1, "the clipboard takes only what's here");
    view.start_export(ExportFormat::find(None, "json").unwrap(), Target::File(file.clone()));
    manual.run_all();
    view.poll();
    let last = ran.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last, "SELECT * FROM (\nSELECT * FROM users\n) AS console_result");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("user 5"), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_write_exports_what_it_brought() {
    let manual = Manual::new();
    let (mut view, ran) = view(&manual, 5, "INSERT INTO u SELECT * FROM v RETURNING *;", 2);
    view.start_export(ExportFormat::find(None, "csv").unwrap(), Target::Clipboard);
    manual.run_all();
    view.poll();
    let told = view.take_notifications();
    assert_eq!(told.len(), 2, "said partial, then copied: {told:?}");
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(text))) => {
            assert_eq!(text, "id,name\n1,user 1\n2,user 2");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(ran.lock().unwrap().len(), 1);
}

#[test]
fn a_result_is_exported_as_a_workbook_or_by_any_extractor() {
    let manual = Manual::new();
    let (mut view, _) = view(&manual, 3, "SELECT * FROM users;", 10);
    let dir = scratch("xlsx");
    let file = dir.join("users.xlsx");
    assert!(view.command("grid:export:xlsx"));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::PickSavePath { ref name, .. })) if name == "users.xlsx"
    ));
    assert!(view.command(&format!("picked-save:{}", file.display())));
    manual.run_all();
    view.poll();
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    // A workbook never goes to the clipboard; SQL UPDATEs do, through the source's dialect.
    assert!(!view.command("grid:export-copy:xlsx"));
    assert!(view.command("grid:export-copy:sql-where"));
    manual.run_all();
    view.poll();
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(text))) => {
            assert!(text.starts_with("(\"id\" = 1 AND"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_extractors_commands_reach_the_app() {
    let manual = Manual::new();
    let (mut view, _) = view(&manual, 3, "SELECT * FROM users;", 10);
    assert!(view.command("grid:extractors"));
    assert!(view.grid().extractors_open());
    view.grid_mut().toggle_skip(true);
    view.grid_mut().toggle_skip(true);
    view.command("grid:noop");
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::RunCommand(ref id)) if id == extractors::SAVE_COMMAND
    ));
}
