//! `jit-dominates` — compiling the query (PostgreSQL's JIT) took a large part of its time:
//! worth it for long analytic queries, not for short ones the planner overestimated.
//!
//! - **Evidence:** JIT's total at least [`Thresholds::jit_ms`](super::Thresholds) (50 ms)
//!   and at least `jit_share` (30 %) of the execution.
//! - **Severity:** `Warning`.
//! - **Suggestion:** `SET jit = off` for the session (or a higher `jit_above_cost`, said).

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity, suggest};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "jit-dominates";

pub struct JitDominates;

impl Rule for JitDominates {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.statement.jit.is_some_and(|j| j.total_ms.is_some())
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let s = &cx.profile().statement;
        let (Some(jit), Some(execution)) = (s.jit.and_then(|j| j.total_ms), s.execution_ms) else {
            return Vec::new();
        };
        if jit < t.jit_ms || execution <= 0.0 || jit < execution * t.jit_share {
            return Vec::new();
        }
        let share = (jit / execution).min(1.0);
        vec![
            Finding::new(ID, Severity::Warning)
                .impact(share)
                .with(E::Jit, jit, Unit::Millis)
                .with(E::Execution, execution, Unit::Millis)
                .with(E::Share, share * 100.0, Unit::Percent)
                .suggest(suggest::setting("jit", "off")),
        ]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/jit_dominates.rs"]
mod tests;
