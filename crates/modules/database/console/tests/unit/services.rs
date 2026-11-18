//! The consoles in the Services window: a source › its consoles › their results, a
//! session's tabs (its Output, never closed; a tab per result, pinned or not), the last
//! result shown when a run ends, the views made when shown.

use super::*;
use crate::fake::{FakeAccess, Manual, Project, consoles};
use crate::runner::{RunRequest, RunSettings};
use database::console::{ConsoleAccess, Overrides, PlanInput, RunMode, RunPlan};

fn run(runner: &Runner, access: &FakeAccess, text: &str) {
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
        settings: RunSettings::default(),
        connector: access.connector(access.shop, &Overrides::default()).unwrap(),
        source_name: "shop".into(),
        detached: false,
    }));
}

#[test]
fn a_source_its_consoles_and_their_results() {
    let project = Project::new("services");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, "").unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let _lease = runner.lease();
    let mut services = ConsoleServices::new(Arc::clone(&consoles), Arc::new(|| {}));
    let names =
        |s: &ConsoleServices| -> Vec<String> { s.nodes().iter().map(|n| n.name.clone()).collect() };
    assert_eq!(names(&services), ["shop", "console"], "a session before it ran");
    let source = &services.nodes()[0];
    assert_eq!(source.color.as_deref(), Some("green"));
    let console = services.console_node(spec.id).unwrap();
    assert_eq!(services.nodes()[1].parent, Some(source.id));
    let tabs = services.tabs(console);
    assert_eq!(tabs.len(), 1);
    assert!(!tabs[0].closable, "the Output stays");

    run(
        &runner,
        &access,
        "SELECT * FROM users;\nSELECT * FROM orders o JOIN users u ON u.id = o.user_id;",
    );
    manual.run_all();
    let generation = services.generation();
    assert!(services.poll());
    assert!(services.generation() > generation);
    assert_eq!(
        names(&services)[2..],
        ["main.users".to_owned(), i18n::tr!("console.result_title", number = 2)]
    );
    let result = services.nodes()[2].id;
    assert_eq!(services.nodes()[2].parent, Some(console));
    assert!(!services.nodes()[1].detail.is_empty(), "its time");
    // The last result shown.
    let last = services.nodes()[3].id;
    assert_eq!(services.take_reveal(), Some((console, Some(last))));
    assert_eq!(services.take_reveal(), None, "once");
    // A result's node has the session's tabs (its own shown by the window).
    let tabs = services.tabs(result);
    assert_eq!(tabs.iter().map(|t| t.id).collect::<Vec<_>>(), [tabs[0].id, result, last]);
    // Views made when asked.
    assert!(services.open_tab(tabs[0].id).is_some(), "the Output's log");
    let mut view = services.open_tab(result).unwrap();
    assert!(view.status().is_some_and(|s| s.contains("main.users")));
    // Pinned, it stays through the next run; the other is replaced.
    services.pin_tab(result, true);
    run(&runner, &access, "SELECT 1 AS one;");
    manual.run_all();
    services.poll();
    let titles: Vec<String> = services.tabs(console).into_iter().map(|t| t.title).collect();
    assert_eq!(titles[1], "main.users");
    assert_eq!(titles.len(), 3);
    assert!(services.tabs(console)[1].pinned);
    // Closed: gone with its rows.
    services.close_tab(result);
    assert_eq!(services.tabs(console).len(), 2);
    let _ = view.poll();
}

#[test]
fn a_failed_run_shows_its_output() {
    let project = Project::new("services-failed");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, "").unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let _lease = runner.lease();
    let mut services = ConsoleServices::new(Arc::clone(&consoles), Arc::new(|| {}));
    run(&runner, &access, "SELECT * FROM nowhere;");
    manual.run_all();
    services.poll();
    let console = services.console_node(spec.id).unwrap();
    assert_eq!(services.take_reveal(), Some((console, services.output_tab(spec.id))));
    assert_eq!(services.nodes()[1].state, ServiceState::Error);
}

/// A console with its transaction open is marked `TX` in the tree, until it ends.
#[test]
fn an_open_transaction_is_marked_in_the_tree() {
    let project = Project::new("services-tx");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, "").unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let _lease = runner.lease();
    let mut services = ConsoleServices::new(Arc::clone(&consoles), Arc::new(|| {}));
    run(&runner, &access, "BEGIN; DELETE FROM orders WHERE id = 1;");
    manual.run_all();
    services.poll();
    let node = services.console_node(spec.id).unwrap();
    let state = |s: &ConsoleServices| s.nodes().iter().find(|n| n.id == node).unwrap().state;
    assert_eq!(state(&services), ServiceState::TxOpen);
    runner.finish_transaction(true);
    manual.run_all();
    services.poll();
    assert_eq!(state(&services), ServiceState::Idle);
}

/// A console's file renamed: its node is the new name's, the same node (its results kept).
#[test]
fn a_renamed_console_s_node_follows() {
    let project = Project::new("services-rename");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let _follow = consoles.follow();
    let spec = consoles.create(access.shop, "").unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let _lease = runner.lease();
    run(&runner, &access, "SELECT * FROM users;");
    manual.run_all();
    let mut services = ConsoleServices::new(Arc::clone(&consoles), Arc::new(|| {}));
    let node = services.console_node(spec.id).unwrap();
    let before = services.nodes().len();
    let from = spec.path(&project.root);
    let to = from.with_file_name("users.sql");
    std::fs::rename(&from, &to).unwrap();
    consoles.events().publish(&workspace::events::FileRenamed { from, to });
    assert!(services.poll());
    assert_eq!(services.console_node(spec.id), Some(node), "the same node");
    let shown = services.nodes().iter().find(|n| n.id == node).unwrap();
    assert_eq!(shown.name, "users");
    assert_eq!(services.nodes().len(), before, "its results kept");
}

/// "Database Consoles": a source's menu lists its consoles (the popover, even with none); a
/// console's opens it, renames or deletes it through the module's commands.
#[test]
fn a_node_s_menu_opens_renames_and_deletes_its_console() {
    use ui_components::ViewerAction;
    let project = Project::new("services-menu");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let spec = consoles.create(access.shop, "").unwrap();
    let _lease = consoles.runner(spec.id, spec.source).lease();
    let mut services = ConsoleServices::new(Arc::clone(&consoles), Arc::new(|| {}));
    let source = services.nodes()[0].id;
    let console = services.console_node(spec.id).unwrap();
    let ids = |entries: Vec<ui_components::ViewMenuEntry>| -> Vec<String> {
        entries
            .into_iter()
            .filter_map(|e| match e {
                ui_components::ViewMenuEntry::Item { id, .. } => Some(id),
                _ => None,
            })
            .collect()
    };
    assert_eq!(ids(services.menu(source)), ["consoles"]);
    assert_eq!(ids(services.menu(console)), ["open", "rename", "delete"]);
    assert!(services.command(source, "consoles"));
    assert_eq!(services.take_action(), Some(ViewerAction::RunCommand(crate::JUMP.0.into())));
    let asked = database::console::take_jump().unwrap();
    assert!(asked.list && asked.source == access.shop);
    assert!(services.command(console, "open"));
    assert_eq!(
        services.take_action(),
        Some(ViewerAction::OpenFile { path: spec.path(consoles.root()), line: None, column: None })
    );
    assert!(services.command(console, "rename"));
    assert!(matches!(services.take_action(), Some(ui_components::ViewerAction::WithHost(_))));
    assert!(services.command(console, "delete"));
    assert!(matches!(services.take_action(), Some(ui_components::ViewerAction::WithHost(_))));
    assert!(!services.command(console, "nothing"));
}

/// An attached SQL file of the project (39n) is a session of its source like a console, with
/// a file's icon; its menu opens it or detaches it (never renamed or deleted from here).
#[test]
fn an_attached_file_s_session_opens_and_detaches() {
    use ui_components::ViewerAction;
    let project = Project::new("services-attached");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let path = project.root.join("report.sql");
    std::fs::write(&path, "SELECT 1;").unwrap();
    let spec = consoles.attach(&path, access.shop).unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let lease = runner.lease();
    let mut services = ConsoleServices::new(Arc::clone(&consoles), Arc::new(|| {}));
    let names: Vec<String> = services.nodes().iter().map(|n| n.name.clone()).collect();
    assert_eq!(names, ["shop", "report.sql"]);
    let node = services.console_node(spec.id).unwrap();
    assert_eq!(services.nodes()[1].icon, "file-code");
    let ids: Vec<String> = services
        .menu(node)
        .into_iter()
        .filter_map(|e| match e {
            ui_components::ViewMenuEntry::Item { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["open", "detach"]);
    assert!(services.command(node, "open"));
    assert_eq!(
        services.take_action(),
        Some(ViewerAction::OpenFile { path: path.clone(), line: None, column: None })
    );
    assert!(services.command(node, "detach"));
    assert!(consoles.attached_of(&path).is_none());
    drop(lease);
    services.poll();
    assert!(services.nodes().is_empty(), "its session gone with the attachment");
    manual.run_all();
}
