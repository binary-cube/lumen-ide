//! `workers-not-launched` — a parallel step got fewer workers than planned: the server had
//! none free (`max_parallel_workers`, `max_worker_processes` in use), so it ran slower than
//! the planner thought.
//!
//! - **Evidence:** a `Gather` / `Gather Merge` with `Workers Launched` below
//!   `Workers Planned`.
//! - **Severity:** `Warning`.
//! - **Suggestion:** none to write (the server's settings, or a busy moment: the text says
//!   which).

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "workers-not-launched";

pub struct WorkersNotLaunched;

impl Rule for WorkersNotLaunched {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.depth.runs() && report.profile.nodes.iter().any(|n| n.op.gathers())
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if !node.op.gathers() {
                continue;
            }
            let Some(p) = &node.parallel else { continue };
            let (Some(planned), Some(launched)) = (p.planned, p.launched) else { continue };
            if launched >= planned {
                continue;
            }
            out.push(
                Finding::new(ID, Severity::Warning)
                    .node(id)
                    .impact(cx.inclusive_share(id))
                    .text(E::Step, cx.step(id))
                    .with(E::Planned, f64::from(planned), Unit::Count)
                    .with(E::Launched, f64::from(launched), Unit::Count)
                    .at(cx.node_range(id)),
            );
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/workers_not_launched.rs"]
mod tests;
