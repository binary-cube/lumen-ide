use std::time::{Duration, Instant};

use lsp_client::fake::Behaviour;
use workbench_model::{ActOutcome, ActionKind, ActionTarget, PartRef, ResourceReporter};

use super::*;
use crate::harness::{Harness, Project, wait};

#[test]
fn each_server_is_a_part_of_its_project_with_its_figures_and_actions() {
    let _h = Harness::new(Behaviour::default());
    let project = Project::open("report");
    let doc = project.open_doc("a.ts", "typescript", "total");
    project.open_doc("b.tsx", "tsx", "total");
    wait("ready", || {
        parts(Instant::now()).iter().any(|p| {
            p.project == Some(project.id) && p.detail.contains(&i18n::t("lsp.state_ready"))
        })
    });
    let client = project.servers().client_of(doc).unwrap();
    client.hover(doc, 1, 0..0).wait(Duration::from_secs(5));
    let part = parts(Instant::now()).into_iter().find(|p| p.project == Some(project.id)).unwrap();
    assert_eq!(part.name, "fake");
    assert_eq!(part.owner, OWNER);
    assert_eq!(part.group, workbench_model::resources::LANGUAGE_SERVERS);
    // In memory, it has no process.
    assert!(part.processes.is_empty());
    let fact = |key: &str| {
        part.facts.iter().find(|(l, _)| *l == i18n::t(key)).map(|(_, v)| v.clone()).unwrap()
    };
    assert_eq!(fact("lsp.fact_languages"), "TSX, TypeScript");
    assert_eq!(fact("lsp.fact_documents"), "2");
    assert_eq!(fact("lsp.fact_requests"), "1");
    assert_ne!(fact("lsp.fact_latency"), i18n::t("lsp.none"));
    let kinds: Vec<ActionKind> = part.actions.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [ActionKind::Restart, ActionKind::Stop, ActionKind::ShowLog, ActionKind::Settings]
    );
    // Its actions.
    let reporter = Reporter;
    let part_ref = PartRef { owner: OWNER.into(), name: "fake".into(), project: Some(project.id) };
    assert_eq!(
        reporter.act(&part_ref, "log", ActionTarget::Part),
        ActOutcome::Command(SHOW_LOG.into())
    );
    assert_eq!(
        reporter.act(&part_ref, "settings", ActionTarget::Part),
        ActOutcome::Command("app.settings".into())
    );
    assert_eq!(reporter.act(&part_ref, "restart", ActionTarget::Part), ActOutcome::Done);
    assert_eq!(reporter.act(&part_ref, "stop", ActionTarget::Part), ActOutcome::Done);
    assert!(parts(Instant::now()).iter().all(|p| p.project != Some(project.id)));
    assert_eq!(reporter.act(&part_ref, "stop", ActionTarget::Part), ActOutcome::Unsupported);
}

#[test]
fn durations_and_language_names_read_well() {
    assert_eq!(language_name("typescript"), "TypeScript");
    assert_eq!(language_name("rust"), "rust");
    assert_eq!(duration_text(Duration::from_millis(42)), i18n::tr!("lsp.ms", count = 42));
    assert_eq!(
        duration_text(Duration::from_secs(125)),
        i18n::tr!("lsp.minutes", minutes = 2, seconds = 5)
    );
}

/// Before closing: a server's download running is the app's work (every project's), only
/// interrupted if stopped (it's downloaded again), stopped through its "Stop".
#[test]
fn a_download_running_is_asked_about_before_quitting() {
    use workbench_model::{ActivityKind, ActivityUnit};
    let id = crate::install::ACTIVITIES.start(
        None,
        ActivityKind::Download,
        "closing-test-server",
        ActivityUnit::Bytes,
        Some(100),
    );
    crate::install::ACTIVITIES.stoppable(id, std::sync::Arc::default());
    let running = Reporter.running();
    let work = running.iter().find(|w| w.title == "closing-test-server").unwrap();
    assert_eq!(work.project, None);
    assert_eq!(work.impact, workbench_model::StopImpact::Interrupts);
    let stop = work.stop.as_ref().unwrap();
    assert_eq!(Reporter.act(&stop.part, stop.action, stop.target), ActOutcome::Done);
    crate::install::ACTIVITIES.end(id, workbench_model::Outcome::Stopped);
    assert!(!Reporter.running().iter().any(|w| w.title == "closing-test-server"));
}
