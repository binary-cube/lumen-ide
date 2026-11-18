//! `planning-dominates` — planning the query took at least as long as running it: many
//! partitions or joins to weigh, a query planned again each time.
//!
//! - **Evidence:** the engine's planning time at least its execution time and at least
//!   [`Thresholds::planning_ms`](super::Thresholds) (10 ms). A fast query's 0.2 ms of planning
//!   is nothing.
//! - **Severity:** `Info`.
//! - **Suggestion:** none to write (fewer partitions touched, fewer joins, a prepared
//!   statement: the text says them).

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "planning-dominates";

pub struct PlanningDominates;

impl Rule for PlanningDominates {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.statement.planning_ms.is_some()
            && report.profile.statement.execution_ms.is_some()
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let s = &cx.profile().statement;
        let (Some(planning), Some(execution)) = (s.planning_ms, s.execution_ms) else {
            return Vec::new();
        };
        if planning < cx.thresholds.planning_ms || planning < execution {
            return Vec::new();
        }
        let share = planning / (planning + execution).max(f64::MIN_POSITIVE);
        vec![
            Finding::new(ID, Severity::Info)
                .impact(share)
                .with(E::Planning, planning, Unit::Millis)
                .with(E::Execution, execution, Unit::Millis),
        ]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/planning_dominates.rs"]
mod tests;
