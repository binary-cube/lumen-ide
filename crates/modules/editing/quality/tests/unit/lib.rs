use super::*;

/// A project's runs are found by its identity while it's open; its entry in the table goes
/// with its subscriptions.
#[test]
fn a_closed_project_leaves_nothing_in_the_table() {
    let root = std::env::temp_dir();
    let events = kernel::EventBus::default();
    let id = ProjectId::new();
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id,
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: None,
        language_of: Arc::new(|_: &Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let subscriptions = QualityService.attach(&context);
    assert!(runs_of(id).is_some());
    drop(subscriptions);
    assert!(runs_of(id).is_none());
    assert!(PROJECTS.get(id).is_none(), "its entry went too");
}

/// A checker's problems go out as the workbench's diagnostics, kept under the tool's name,
/// each with its own tool and rule.
#[test]
fn problems_go_out_as_diagnostics() {
    let problem = code_quality::Problem {
        path: "src/A.php".into(),
        line: 3,
        column: Some(2),
        severity: code_quality::Severity::Warning,
        message: "Bad".into(),
        rule: Some("rule.id".into()),
        tool: "PHPStan".into(),
        fixable: true,
    };
    let found = diagnostics_found("phpstan", &Target::File("src/A.php".into()), vec![problem]);
    assert_eq!(found.source, "phpstan");
    assert_eq!(found.scope, workbench_model::DiagnosticsScope::File("src/A.php".into()));
    let d = &found.diagnostics[0];
    assert_eq!((d.line, d.column, d.severity), (3, Some(2), workbench_model::Severity::Warning));
    assert_eq!((d.source.as_str(), d.rule.as_deref()), ("PHPStan", Some("rule.id")));
    let all = diagnostics_found("phpstan", &Target::Project, Vec::new());
    assert_eq!(all.scope, workbench_model::DiagnosticsScope::Project);
}

/// A formatter running is offered "Stop" (asked first); its owner stops it — the program
/// and what it started — and it's gone from the part; git-like owners aren't asked.
#[cfg(unix)]
#[test]
#[allow(clippy::disallowed_methods)]
fn a_running_program_is_stopped_from_the_task_manager() {
    use std::time::{Duration, Instant};
    use workbench_model::ResourceReporter as _;
    let spec = process::Run::shell("sleep 30")
        .timeout(Duration::from_secs(60))
        .listed_in(Arc::clone(&runner::RUNNING));
    // Its own program (others' tests run theirs alongside).
    let before = runner::RUNNING.pids();
    let run = std::thread::spawn(move || process::run(&spec));
    let deadline = Instant::now() + Duration::from_secs(10);
    let (part, pid) = loop {
        // Nothing running: no part.
        if let Some(part) = Programs.report().pop()
            && let Some(&pid) = part.processes.iter().find(|p| !before.contains(p))
        {
            break (part, pid);
        }
        assert!(Instant::now() < deadline, "it started");
        std::thread::sleep(Duration::from_millis(10));
    };
    let stop = part.actions.iter().find(|a| a.target == ActionTarget::Process(pid)).unwrap();
    assert!(stop.confirm, "asked first");
    assert_eq!(stop.kind, ActionKind::Stop);
    let named = PartRef { owner: part.owner.clone(), name: part.name.clone(), project: None };
    assert_eq!(
        Programs.act(&named, stop.id, ActionTarget::Process(u32::MAX)),
        ActOutcome::Unsupported
    );
    assert_eq!(Programs.act(&named, stop.id, stop.target), ActOutcome::Done);
    assert!(matches!(run.join().unwrap(), Err(process::RunError::Stopped)));
    let left = Programs.report();
    assert!(left.iter().all(|p| !p.processes.contains(&pid)), "its tree gone");
}

/// Nothing running: no row in the Task Manager (it would only say so).
#[test]
fn nothing_running_shows_no_row() {
    use workbench_model::ResourceReporter as _;
    if runner::RUNNING.pids().is_empty() {
        assert!(Programs.report().is_empty());
    }
}
