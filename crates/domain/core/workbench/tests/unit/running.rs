// The tests make their own instants (the domain never reads the clock).
#![allow(clippy::disallowed_methods)]

use super::*;
use crate::resources::{Activities, Outcome};

/// Two projects, in the order they sort.
fn two() -> (ProjectId, ProjectId) {
    let (a, b) = (ProjectId::new(), ProjectId::new());
    if a < b { (a, b) } else { (b, a) }
}

fn work(owner: &str, project: Option<ProjectId>, impact: StopImpact) -> RunningWork {
    RunningWork::new(WorkKind::Program, format!("{owner} work"), owner, project, impact)
}

#[test]
fn operations_cost_by_their_kind() {
    assert_eq!(ActivityKind::Upload.stop_impact(), StopImpact::LosesWork);
    assert_eq!(ActivityKind::Export.stop_impact(), StopImpact::LosesWork);
    assert_eq!(ActivityKind::Download.stop_impact(), StopImpact::Interrupts);
    assert_eq!(ActivityKind::Index.stop_impact(), StopImpact::Background);
    assert_eq!(ActivityKind::History.stop_impact(), StopImpact::Background);
}

/// The default: the parts' running operations, the stoppable ones with the part's "Stop";
/// the ended ones not.
#[test]
fn a_reporter_s_running_operations_are_its_running_work() {
    let activities = Activities::new(Instant::now);
    let p = ProjectId::new();
    let upload =
        activities.start(Some(p), ActivityKind::Upload, "index.php", ActivityUnit::Bytes, Some(10));
    activities.advance(upload, 4);
    activities.stoppable(upload, std::sync::Arc::default());
    let done =
        activities.start(Some(p), ActivityKind::Download, "a.txt", ActivityUnit::Bytes, None);
    activities.end(done, Outcome::Done);
    let index = activities.start(Some(p), ActivityKind::Index, "src", ActivityUnit::Files, None);
    let mut part = ResourceUse::new("Remote: shop", "remote.projects");
    part.project = Some(p);
    activities.report(&mut part);

    let running = of_parts(&[part]);
    assert_eq!(running.len(), 2, "the ended one isn't");
    let first = &running[0];
    assert_eq!(first.title, "index.php");
    assert_eq!(first.kind, WorkKind::Operation(ActivityKind::Upload));
    assert_eq!(first.impact, StopImpact::LosesWork);
    assert_eq!(first.project, Some(p));
    assert_eq!(first.progress.and_then(|p| p.fraction()), Some(0.4));
    let stop = first.stop.as_ref().expect("stoppable");
    assert_eq!(stop.action, STOP_OPERATION);
    assert_eq!(stop.target, ActionTarget::Activity(upload));
    assert_eq!(stop.part.name, "Remote: shop");
    // The index runs too, but in the background (never asked about, see `to_ask`).
    assert_eq!(running[1].impact, StopImpact::Background);
    assert!(running[1].stop.is_none());
    let _ = index;
}

#[test]
fn a_scope_includes_its_projects_and_the_app_s_work_only_when_the_app_goes() {
    let (a, b) = two();
    let window = CloseScope::window(Some(a), false);
    assert!(window.includes(Some(a)));
    assert!(!window.includes(Some(b)));
    assert!(!window.includes(None), "other windows keep the app running");
    assert!(CloseScope::window(Some(a), true).includes(None), "the last window: the app goes");
    let bare = CloseScope::window(None, false);
    assert!(!bare.includes(Some(a)) && !bare.includes(None));
    let close_project = CloseScope::project(a);
    assert!(close_project.includes(Some(a)));
    assert!(!close_project.includes(Some(b)) && !close_project.includes(None));
    let quit = CloseScope::quit();
    assert!(quit.includes(Some(a)) && quit.includes(Some(b)) && quit.includes(None));
}

/// Only what closes, never background work; by project (the app's last), module, impact.
#[test]
fn what_is_asked_is_filtered_and_ordered() {
    let (a, b) = two();
    let items = vec![
        work("terminal", None, StopImpact::LosesWork),
        work("vcs.git", Some(b), StopImpact::Interrupts),
        work("remote", Some(a), StopImpact::Interrupts),
        work("indexing", Some(a), StopImpact::Background),
        work("remote", Some(a), StopImpact::LosesWork),
        work("database", Some(a), StopImpact::LosesWork),
    ];
    let only_a = to_ask(items.clone(), &CloseScope::project(a));
    let owners: Vec<(&str, StopImpact)> =
        only_a.iter().map(|w| (w.owner.as_str(), w.impact)).collect();
    assert_eq!(
        owners,
        [
            ("database", StopImpact::LosesWork),
            ("remote", StopImpact::LosesWork),
            ("remote", StopImpact::Interrupts),
        ]
    );
    let all = to_ask(items, &CloseScope::quit());
    assert_eq!(all.len(), 5, "the index isn't asked about");
    assert_eq!(all.last().map(|w| w.project), Some(None), "the app's last");
    assert!(to_ask(Vec::new(), &CloseScope::quit()).is_empty());
}

#[test]
fn work_is_grouped_by_project_then_module() {
    let (a, b) = two();
    let items = to_ask(
        vec![
            work("remote", Some(a), StopImpact::LosesWork),
            work("terminal", Some(a), StopImpact::LosesWork),
            work("remote", Some(a), StopImpact::Interrupts),
            work("remote", Some(b), StopImpact::LosesWork),
            work("lsp", None, StopImpact::Interrupts),
        ],
        &CloseScope::quit(),
    );
    let groups = grouped(items);
    let shape: Vec<(Option<ProjectId>, &str, usize)> =
        groups.iter().map(|g| (g.project, g.owner.as_str(), g.items.len())).collect();
    assert_eq!(
        shape,
        [
            (Some(a), "remote", 2),
            (Some(a), "terminal", 1),
            (Some(b), "remote", 1),
            (None, "lsp", 1)
        ]
    );
    let prompt = RunningPrompt {
        closing: Closing::App,
        groups,
        projects: Vec::new(),
        modules: Vec::new(),
        waiting: false,
    };
    assert_eq!(prompt.count(), 5);
}

#[test]
fn progress_without_a_total_has_no_fraction() {
    let p = WorkProgress { done: 5, total: None, unit: ActivityUnit::Files };
    assert_eq!(p.fraction(), None);
    let p = WorkProgress { done: 5, total: Some(0), unit: ActivityUnit::Files };
    assert_eq!(p.fraction(), None);
    let p = WorkProgress { done: 15, total: Some(10), unit: ActivityUnit::Files };
    assert_eq!(p.fraction(), Some(1.0));
}

/// A part's owner finds its module however its id is written.
#[test]
fn owners_resolve_to_their_module() {
    assert!(same_module("project-tree", "project.tree"));
    assert!(same_module("remote.projects", "remote.projects"));
    assert!(!same_module("project-tree", "project"));
    let prompt = RunningPrompt {
        closing: Closing::Project,
        groups: Vec::new(),
        projects: Vec::new(),
        modules: vec![("project-tree".into(), "Project".into())],
        waiting: false,
    };
    assert_eq!(prompt.module_name("project.tree"), Some("Project"));
    assert_eq!(prompt.module_name("terminal"), None);
}
