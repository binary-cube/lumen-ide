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

/// The grid is the tab's control: a press reaches it through the controls with its
/// modifiers (⇧ extends the choice), the drag and the release follow it.
#[test]
fn a_press_with_shift_reaches_the_grid_through_the_controls() {
    let manual = Manual::new();
    let (mut view, _) = view(&manual, 10, "SELECT * FROM users;", 10);
    view.poll();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    // Drawn, its rows read, drawn again with them.
    for _ in 0..2 {
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 800.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
        manual.run_all();
        view.poll();
    }
    // Under the bar, the grid's tools and its header: its rows.
    let first = Point::new(200.0, 200.0);
    let below = Point::new(200.0, 300.0);
    view.pointer_down(first, 1, PressMods::default());
    view.release();
    let one = view.grid().selection().count();
    let shift = PressMods { shift: true, ..PressMods::default() };
    view.pointer_down(below, 1, shift);
    view.release();
    let extended = view.grid().selection().count();
    assert_eq!(one, 1, "a cell chosen");
    assert!(extended > one, "⇧ extends the choice: {extended}");
    view.pointer_down(below, 1, PressMods::default());
    view.release();
    assert_eq!(view.grid().selection().count(), 1, "without ⇧: one cell again");
}

/// The grid has the keys, the text and the wheel without a press (the tab is the grid),
/// through the controls: a press on the bar leaves them to it; what it has selected is
/// the copy's.
#[test]
fn the_grid_has_the_keys_and_the_wheel_through_the_controls() {
    let manual = Manual::new();
    let (mut view, _) = view(&manual, 200, "SELECT * FROM users;", 200);
    view.poll();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    for _ in 0..2 {
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 400.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
        manual.run_all();
        view.poll();
    }
    assert!(view.scroll(0.0, 60.0), "the wheel reaches the grid without a press");
    assert!(view.selected_text().is_none(), "nothing chosen yet");
    view.pointer_down(Point::new(200.0, 200.0), 1, PressMods::default());
    view.release();
    let first = view.selected_text();
    assert!(first.is_some(), "the chosen cell is the copy's");
    // A press on the bar beside its buttons: the grid keeps the keys.
    view.pointer_down(Point::new(5.0, 5.0), 1, PressMods::default());
    assert!(view.edit_key(ViewerKey::Down), "↓ moves in the grid");
    assert!(view.selected_text().is_some());
    assert_ne!(view.selected_text(), first, "another cell chosen");
}
