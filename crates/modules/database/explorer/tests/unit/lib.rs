use std::path::Path;

use database::DataSourceId;
use kernel::{EventBus, ProjectId};

use super::*;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};

#[test]
fn contributes_its_threads_service_reporter_data_view_settings_and_keys() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(DatabaseExplorerModule));
    let (mut contributions, reports) = registry.activate(&Default::default());
    assert_eq!(reports[0].id.as_str(), MODULE_ID);
    assert_eq!(contributions.take::<Arc<dyn ProjectService>>().len(), 1);
    assert_eq!(contributions.take::<Arc<dyn ResourceReporter>>().len(), 1);
    let threads = contributions.take::<workbench_model::ModuleThreads>();
    assert_eq!(threads[0].0, &["db", "db-idle"]);
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    assert_eq!(
        viewers.iter().map(|v| v.id()).collect::<Vec<_>>(),
        [view::VIEW, preview::EXTENSION, schema::EXTENSION, sources::EXTENSION]
    );
    let settings = contributions.take::<Arc<dyn workbench_model::SettingsContribution>>();
    assert_eq!(settings.iter().map(|s| s.page()).collect::<Vec<_>>(), ["database"]);
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands.len(), ACTIONS.len() + 1 + 4);
    let (keyed, other): (Vec<_>, Vec<_>) =
        commands.iter().partition(|c| c.handler.when() == command_model::When::View(view::VIEW));
    assert_eq!(keyed.len(), ACTIONS.len());
    assert_eq!(
        other.iter().map(|c| c.spec.id).collect::<Vec<_>>(),
        [sources::DATA_SOURCES, window::TOGGLE, window::REFRESH, REFRESH_ALL, COLLAPSE]
    );
    // F5 refreshes the window only while it has the keys.
    let refresh = commands.iter().find(|c| c.spec.id == window::REFRESH).unwrap();
    assert_eq!(refresh.handler.when(), command_model::When::View(window::TOOL));
}

#[test]
fn the_task_manager_sees_a_project_once_it_has_opened_a_source() {
    let manual = Manual::new();
    let id = ProjectId::new();
    let sessions = Sessions::new(id, Path::new("/p"), EventBus::new(), manual.runtime(), None);
    let _guard = sessions::register(Arc::clone(&sessions));
    let mine = || SessionsResources.report().into_iter().filter(|p| p.project == Some(id)).count();
    assert_eq!(mine(), 0, "nothing opened: no part");
    let driver = FakeDriver::new(FakeDb::users(1), true);
    let (wake, _) = counted_wake();
    let client = sessions.attach(driver.source(DataSourceId::new())).client(wake);
    client.send(Request::Tables(Vec::new()));
    manual.run_all();
    let part = SessionsResources.report().into_iter().find(|p| p.project == Some(id)).unwrap();
    assert_eq!(part.owner, MODULE_ID);
    assert!(part.runs_on("db") && part.runs_on("db-idle"));
}

/// The Task Manager's actions on a project's databases: its idle sessions closed (not asked
/// first: they open again when used), an export stopped — it ends Stopped, its temporary
/// file gone; another project's never touched.
#[test]
fn idle_sessions_close_and_an_export_stops_from_the_task_manager() {
    use workbench_model::{ActOutcome, ActionKind, ActionTarget, Outcome, PartRef};
    let manual = Manual::new();
    let id = ProjectId::new();
    let sessions = Sessions::new(id, Path::new("/p"), EventBus::new(), manual.runtime(), None);
    let _guard = sessions::register(Arc::clone(&sessions));
    let driver = FakeDriver::new(FakeDb::users(3), true);
    let (wake, _) = counted_wake();
    let worker = sessions.attach(driver.source(DataSourceId::new()));
    let client = worker.client(wake);
    client.send(Request::Tables(Vec::new()));
    manual.run_all();
    assert_eq!(sessions.connected(), 1);
    let part = || SessionsResources.report().into_iter().find(|p| p.project == Some(id)).unwrap();
    let close = part().actions.into_iter().find(|a| a.target == ActionTarget::Part).unwrap();
    assert_eq!(close.kind, ActionKind::Disconnect);
    assert!(!close.confirm, "nothing is lost");
    let named = PartRef { owner: MODULE_ID.into(), name: part().name, project: Some(id) };
    let elsewhere = PartRef { project: Some(ProjectId::new()), ..named.clone() };
    assert_eq!(SessionsResources.act(&elsewhere, close.id, close.target), ActOutcome::Unsupported);
    assert_eq!(SessionsResources.act(&named, close.id, close.target), ActOutcome::Done);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while sessions.connected() > 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(sessions.connected(), 0, "its idle session closed");
    // An export running: stopped from its part.
    let target = std::env::temp_dir().join(format!("export-stop-{}.xlsx", std::process::id()));
    let request = export::ExportRequest {
        table: database::TableRef::new("users"),
        columns: Vec::new(),
        order: Vec::new(),
        filter: None,
        format: ui_components::data_grid::export::ExportFormat::Xlsx,
        sql: None,
        options: Default::default(),
        target: export::Target::File(target.clone()),
        total: Some(3),
    };
    let log: Arc<dyn export::ActivityLog> = sessions;
    let running =
        export::Export::start(&worker, manual.runtime(), request, Arc::new(|| {}), Some(log));
    let stop = part().actions.into_iter().find(|a| matches!(a.target, ActionTarget::Activity(_)));
    let stop = stop.expect("its Stop");
    assert_eq!((stop.kind, stop.confirm), (ActionKind::Stop, false), "not asked first");
    assert_eq!(SessionsResources.act(&named, stop.id, stop.target), ActOutcome::Done);
    manual.run_all();
    assert!(running.finished());
    let ActionTarget::Activity(op) = stop.target else { unreachable!() };
    let ended = part().activities.into_iter().find(|a| a.id == op).unwrap();
    assert_eq!(ended.ended.map(|(_, o)| o), Some(Outcome::Stopped));
    assert!(!target.exists(), "the target untouched");
}
