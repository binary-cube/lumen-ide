//! The runs in the Task Manager: a part per project with runs, a request going as work.

use super::*;
use crate::runs::{Run, Runs};

#[test]
fn a_project_with_runs_has_its_part_and_a_request_going_is_work() {
    let project = kernel::ProjectId::new();
    let runs = Runs::of(Some(project), None);
    let mine =
        |parts: Vec<ResourceUse>| parts.into_iter().filter(|p| p.project == Some(project)).count();
    assert_eq!(mine(Reporter.report()), 0, "no runs: no part");
    runs.begin(Run {
        id: kernel::id::new_uuid(),
        number: 0,
        file: "/p/a.http".into(),
        label: "Log in".into(),
        method: "POST".into(),
        line: 0..3,
        line_number: 0,
        hash: 0,
        state: RunState::Running,
        started: std::time::Instant::now(),
        cancel: kernel::work::Cancel::new(),
    });
    assert_eq!(mine(Reporter.report()), 1);
    let going: Vec<_> =
        Reporter.running().into_iter().filter(|w| w.project == Some(project)).collect();
    assert_eq!(going.len(), 1);
    assert_eq!(going[0].title, "Log in");
}

/// Its figures for a profile recording: numbers only (the runs kept, the requests going),
/// by the module's id; nothing of a request.
#[test]
fn its_profile_figures_are_numbers_only() {
    use contracts::profiling::{ProfileSource, ReadingKind};
    let source = Profile;
    assert_eq!(source.owner(), OWNER);
    let read = source.read();
    let names: Vec<&str> = read.iter().map(|r| r.what).collect();
    assert_eq!(names, ["runs-kept", "requests-going"]);
    assert!(read.iter().all(|r| r.kind == ReadingKind::Level));
}
