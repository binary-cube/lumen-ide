//! `trigger-time` — the triggers of a write took a large part of its time.
//!
//! - **Evidence:** the triggers' times adding up to at least
//!   [`Thresholds::trigger_share`](super::Thresholds) (30 %) of the statement's execution.
//! - **Severity:** `Info`.
//! - **Suggestion:** none to write: the slowest trigger named with its time and calls.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "trigger-time";

pub struct TriggerTime;

impl Rule for TriggerTime {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        !report.profile.statement.triggers.is_empty()
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let s = &cx.profile().statement;
        let Some(execution) = s.execution_ms.filter(|e| *e > 0.0) else { return Vec::new() };
        let total: f64 = s.triggers.iter().filter_map(|t| t.ms).sum();
        if total < execution * cx.thresholds.trigger_share {
            return Vec::new();
        }
        let Some(slowest) = s
            .triggers
            .iter()
            .filter(|t| t.ms.is_some())
            .max_by(|a, b| a.ms.unwrap_or(0.0).total_cmp(&b.ms.unwrap_or(0.0)))
        else {
            return Vec::new();
        };
        let share = (total / execution).min(1.0);
        let mut f = Finding::new(ID, Severity::Info)
            .impact(share)
            .text(E::Trigger, slowest.name.clone())
            .with(E::Time, slowest.ms.unwrap_or(0.0), Unit::Millis)
            .with(E::Share, share * 100.0, Unit::Percent)
            .with(E::Execution, execution, Unit::Millis);
        if let Some(calls) = slowest.calls {
            f = f.with(E::Calls, calls as f64, Unit::Count);
        }
        if let Some(r) = &slowest.relation {
            f = f.text(E::Table, r.clone());
        }
        vec![f]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/trigger_time.rs"]
mod tests;
