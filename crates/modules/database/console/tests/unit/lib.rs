use super::*;

/// The module's commands: their keys where they work (⌘↵ in a SQL editor, never over the
/// grid's), its threads, its bar, its settings, its project service.
#[test]
fn the_module_contributes_its_bar_commands_and_keys() {
    let all = commands();
    let run = all.iter().find(|c| c.0 == RUN).unwrap();
    assert_eq!(run.2, ["CmdOrCtrl+Enter"]);
    assert_eq!(run.4.when(), When::EditorLanguage("sql"));
    let jump = all.iter().find(|c| c.0 == JUMP).unwrap();
    assert_eq!(jump.2, ["CmdOrCtrl+Shift+F10"]);
    assert_eq!(jump.4.when(), When::Everywhere);
    let stop = all.iter().find(|c| c.0 == STOP).unwrap();
    assert_eq!(stop.4.when(), When::EditorLanguage("sql"));
    assert_eq!(JUMP.0, database::console::JUMP_COMMAND);
}

/// A console's rows in the editor's menu: Execute, Execute Script and Explain Plan first
/// (their own section), Go to Declaration (its names resolve through the schema), Jump to
/// Query Console and Open in Services among the navigation's;
/// Attach / Detach Data Source on a project's SQL file; Jump (enabled everywhere) only while
/// Explain is (in a console or an attached file).
#[test]
fn its_editor_menu_rows_come_first_and_only_in_a_console() {
    use workbench_model::{MenuPlace, MenuSection, MenuWhen};
    let items = menu_items();
    let rows: Vec<_> = items.iter().map(|i| (i.command, i.section, i.when)).collect();
    assert_eq!(
        rows,
        [
            (RUN, MenuSection::Run, MenuWhen::Always),
            (RUN_SCRIPT, MenuSection::Run, MenuWhen::Always),
            (EXPLAIN, MenuSection::Run, MenuWhen::Always),
            (ATTACH, MenuSection::Run, MenuWhen::Always),
            (DETACH, MenuSection::Run, MenuWhen::Always),
            (workbench_model::GOTO_DECLARATION, MenuSection::Navigate, MenuWhen::With(EXPLAIN)),
            (JUMP, MenuSection::Navigate, MenuWhen::With(EXPLAIN)),
            (OPEN_IN_SERVICES, MenuSection::Navigate, MenuWhen::Always),
        ]
    );
    assert!(items.iter().all(|i| i.places == [MenuPlace::Editor]));
    assert!(MenuSection::Run < MenuSection::New, "the first section");
    // Each command but Jump is a console's own (enabled only on one).
    let commands = commands();
    // Go to Declaration is the app's (shown while Execute is enabled).
    for item in
        items.iter().filter(|i| i.command != JUMP && i.command != workbench_model::GOTO_DECLARATION)
    {
        let (.., handler) = commands.iter().find(|c| c.0 == item.command).unwrap();
        assert_eq!(
            handler.when(),
            command_model::When::EditorLanguage("sql"),
            "{:?}",
            item.command
        );
    }
}

/// A console's query is an operation of its own (the Task Manager's, the question before
/// closing): its first line, its whole text, where it runs, since when; its Stop is the
/// console's (the engine told to cancel); once it ends, its outcome is kept.
#[test]
fn a_running_query_is_an_operation_stopped_as_the_console_s_stop() {
    use crate::fake::{FakeAccess, Manual, Project};
    use database::console::{ConsoleAccess, ConsoleId, Overrides, PlanInput, RunMode, RunPlan};
    use workbench_model::{ActionTarget, ActivityKind, Outcome, PartRef, StopImpact};
    let project = Project::new("closing-query");
    let manual = Manual::new();
    let access = FakeAccess::new(&project);
    let runner = runner::Runner::new(ConsoleId::new(), access.shop, manual.runtime());
    let id = kernel::ProjectId::new();
    runner.set_context(runner::RunnerContext {
        project: Some(id),
        name: "reports.sql".into(),
        ..runner::RunnerContext::default()
    });
    let _lease = runner.lease();
    let text = "SELECT *\n  FROM users;";
    let flavor = database::sql::Flavor::Sqlite;
    let input = PlanInput { text, mode: RunMode::Script, flavor, ..PlanInput::default() };
    let statements = match database::console::plan(&input) {
        RunPlan::Run(s) | RunPlan::Choose(s) => s,
        RunPlan::Nothing => Vec::new(),
    };
    assert!(runner.run(runner::RunRequest {
        statements,
        document: text.into(),
        settings: runner::RunSettings::default(),
        connector: access.connector(access.shop, &Overrides::default()).unwrap(),
        source_name: "shop".into(),
        detached: false,
    }));
    let mine = |part: &ResourceUse| {
        part.activities.iter().find(|a| a.detail.contains("reports.sql")).cloned()
    };
    let mut part = ResourceUse::new("Consoles", MODULE_ID);
    part.project = Some(id);
    runner::QUERIES.report(&mut part);
    let query = mine(&part).expect("listed");
    assert_eq!(query.kind, ActivityKind::Query);
    assert_eq!(query.subject, "SELECT *", "its first line");
    assert!(query.text.as_deref().unwrap().contains("FROM users"), "its whole text");
    assert_eq!(query.detail, "shop · reports.sql", "its source and console");
    assert!(query.running() && query.stoppable);
    // The question before closing sees it too (the same operation, nothing kept twice).
    let work = workbench_model::running::of_parts(std::slice::from_ref(&part));
    let work = work.iter().find(|w| w.detail.contains("reports.sql")).unwrap();
    assert_eq!(work.impact, StopImpact::LosesWork);
    // Stop, from the Task Manager: the console's Stop.
    let stop = part.actions.iter().find(|a| a.target == ActionTarget::Activity(query.id)).unwrap();
    let named = PartRef { owner: MODULE_ID.into(), name: "Consoles".into(), project: Some(id) };
    assert_eq!(
        ConsolesResources.act(&named, stop.id, stop.target),
        workbench_model::ActOutcome::Done
    );
    manual.run_all();
    assert!(runner.snapshot().last.unwrap().cancelled, "cancelled, as the console's Stop");
    let mut part = ResourceUse::new("Consoles", MODULE_ID);
    part.project = Some(id);
    runner::QUERIES.report(&mut part);
    let ended = mine(&part).unwrap();
    assert_eq!(ended.ended.map(|(_, o)| o), Some(Outcome::Stopped), "kept with its outcome");
    assert!(!part.actions.iter().any(|a| a.target == ActionTarget::Activity(query.id)));
}

/// A query that ends: Done, with the rows it brought.
#[test]
fn a_finished_query_is_kept_done_with_its_rows() {
    use crate::fake::{FakeAccess, Manual, Project};
    use database::console::{ConsoleAccess, ConsoleId, Overrides, PlanInput, RunMode, RunPlan};
    let project = Project::new("finished-query");
    let manual = Manual::new();
    let access = FakeAccess::new(&project);
    let runner = runner::Runner::new(ConsoleId::new(), access.shop, manual.runtime());
    let id = kernel::ProjectId::new();
    runner.set_context(runner::RunnerContext {
        project: Some(id),
        name: "done.sql".into(),
        ..runner::RunnerContext::default()
    });
    let _lease = runner.lease();
    let text = "SELECT * FROM users;";
    let flavor = database::sql::Flavor::Sqlite;
    let input = PlanInput { text, mode: RunMode::Script, flavor, ..PlanInput::default() };
    let RunPlan::Run(statements) = database::console::plan(&input) else { panic!() };
    assert!(runner.run(runner::RunRequest {
        statements,
        document: text.into(),
        settings: runner::RunSettings::default(),
        connector: access.connector(access.shop, &Overrides::default()).unwrap(),
        source_name: "shop".into(),
        detached: false,
    }));
    manual.run_all();
    let mut part = ResourceUse::new("Consoles", MODULE_ID);
    part.project = Some(id);
    runner::QUERIES.report(&mut part);
    let query = part.activities.iter().find(|a| a.detail.contains("done.sql")).unwrap();
    assert_eq!(query.ended.as_ref().map(|(_, o)| o.clone()), Some(workbench_model::Outcome::Done));
    assert_eq!(query.done, 3, "the rows it brought");
}
