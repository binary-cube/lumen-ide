//! The HTTP client in the Task Manager: a part per project with runs kept (how many, how
//! many going, what their responses hold), and each request still going as work a closing
//! would stop.

use workbench_model::running::{RunningWork, StopImpact, WorkKind};
use workbench_model::{ResourceReporter, ResourceUse};

use crate::runs::{RunState, Runs};

/// The module's id, as the Task Manager names its parts.
const OWNER: &str = "tools.http_client";

#[derive(Debug, Default)]
pub struct Reporter;

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        Runs::all()
            .iter()
            .filter(|runs| !runs.runs().is_empty())
            .map(|runs| {
                let all = runs.runs();
                let going = all.iter().filter(|r| matches!(r.state, RunState::Running)).count();
                let mut part = ResourceUse::new(i18n::t("tasks.http"), OWNER);
                part.detail =
                    i18n::tr!("tasks.http_detail", kept = all.len() as i64, going = going as i64);
                part.memory = Some(runs.memory());
                runs.traffic.report(&mut part);
                part.threads = vec![
                    crate::job::THREAD,
                    crate::chain_view::THREAD,
                    crate::response_view::THREAD,
                    crate::window::THREAD,
                ];
                part.group = workbench_model::resources::SERVICES;
                part.project = runs.project();
                part
            })
            .collect()
    }

    fn running(&self) -> Vec<RunningWork> {
        Runs::all()
            .iter()
            .flat_map(|runs| {
                let project = runs.project();
                runs.runs().into_iter().filter(|r| matches!(r.state, RunState::Running)).map(
                    move |r| {
                        let mut work = RunningWork::new(
                            WorkKind::Network,
                            r.label,
                            OWNER,
                            project,
                            StopImpact::Interrupts,
                        );
                        work.since = Some(r.started);
                        work
                    },
                )
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/tasks.rs"]
mod tests;

/// Its figures for a profile recording (`contracts::profiling`): the runs kept and the
/// requests going (what they sent and received is on its Task Manager row) — numbers only,
/// never a request's address or content.
#[derive(Debug, Default)]
pub struct Profile;

impl contracts::profiling::ProfileSource for Profile {
    fn owner(&self) -> &'static str {
        OWNER
    }

    fn read(&self) -> Vec<contracts::profiling::Reading> {
        use contracts::profiling::{Reading, ReadingKind};
        let (mut kept, mut going) = (0u64, 0u64);
        for runs in Runs::all() {
            let (k, g) = runs.counts();
            kept = kept.saturating_add(k as u64);
            going = going.saturating_add(g as u64);
        }
        vec![
            Reading::new("runs-kept", ReadingKind::Level, kept),
            Reading::new("requests-going", ReadingKind::Level, going),
        ]
    }
}
