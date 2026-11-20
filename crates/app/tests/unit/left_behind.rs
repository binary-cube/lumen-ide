use super::*;

use std::time::Duration;

fn record(project: Option<&str>) -> process_registry::Record {
    process_registry::Record {
        pid: 10,
        started: 1,
        kind: "terminal".into(),
        program: "zsh".into(),
        project: project.map(str::to_owned),
    }
}

/// A program is asked about in its project's window; one of no project in the first.
#[test]
fn a_program_is_asked_about_where_it_belongs() {
    let shop = record(Some("/work/shop/src"));
    assert!(belongs_here(&shop, Some(Path::new("/work/shop")), false));
    assert!(!belongs_here(&shop, Some(Path::new("/work/blog")), true));
    assert!(!belongs_here(&shop, None, true));
    let nowhere = record(None);
    assert!(belongs_here(&nowhere, None, true));
    assert!(belongs_here(&nowhere, Some(Path::new("/work/shop")), true));
    assert!(!belongs_here(&nowhere, Some(Path::new("/work/shop")), false));
}

/// The question names the program, its project, how long and whether it works.
#[test]
fn the_question_says_what_runs_where_and_how() {
    let row = process_registry::Row {
        kind: process::launched::Kind::Agent,
        program: "claude".into(),
        project: Some("shop".into()),
        running_for: Duration::from_secs(3 * 3_600),
        cpu: 42.4,
        working: true,
    };
    let text = message(&row);
    assert!(text.contains("**claude**"), "{text}");
    assert!(text.contains("**shop**"), "{text}");
    assert!(text.contains("3"), "{text}");
    assert!(text.contains("42%"), "{text}");
    let idle = process_registry::Row { project: None, working: false, cpu: 0.1, ..row };
    let text = message(&idle);
    assert!(!text.contains("**shop**") && !text.contains('%'), "{text}");
}

/// In the Task Manager: a row each, under its group, named by its program and pid, saying
/// what it is, its project and how long it has run; its processes measured there; Stop
/// (asked first).
#[test]
fn what_was_left_is_listed_in_the_task_manager_with_stop() {
    let found = process_registry::Found {
        record: record(Some("/work/shop")),
        tree: vec![(10, 1), (11, 2)],
        cpu: 0.0,
    };
    let parts = Reporter::parts(&[found], 1 + 2 * 3_600);
    assert_eq!(parts.len(), 1);
    let part = &parts[0];
    assert_eq!(part.name, "zsh (10)");
    assert_eq!(part.group, workbench_model::resources::LEFT_BEHIND);
    assert!(part.detail.contains("shop") && part.detail.contains('2'), "{}", part.detail);
    assert_eq!(part.processes, [10, 11], "it and what it started");
    assert_eq!(part.actions.len(), 1);
    assert!(part.actions[0].confirm, "a stop is asked first");
    // Nothing a closing would stop.
    assert!(workbench_model::ResourceReporter::running(&Reporter).is_empty());
}

/// A closed project's programs: those that ran in its folder or below, unless another open
/// project is nearer (a project inside it keeps its own); none of another project's.
#[test]
fn a_closed_project_owns_only_what_ran_nearest_to_it() {
    use std::path::PathBuf;
    let p = |s: &str| Some(PathBuf::from(s));
    let programs = vec![
        (1, 10, p("/work/shop")),
        (2, 20, p("/work/shop/tools")),
        (3, 30, p("/work/shop/admin")),
        (4, 40, p("/work/shop/admin/src")),
        (5, 50, p("/work/shopping")),
        (6, 60, None),
    ];
    let others = [PathBuf::from("/work/shop/admin"), PathBuf::from("/work/other")];
    let owned = owned_by(programs.clone(), Path::new("/work/shop"), &others);
    assert_eq!(owned, [(1, 10), (2, 20)]);
    // The inner project closing: its own only.
    let outer = [PathBuf::from("/work/shop")];
    let owned = owned_by(programs, Path::new("/work/shop/admin"), &outer);
    assert_eq!(owned, [(3, 30), (4, 40)]);
}
