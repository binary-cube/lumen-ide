//! `hot-spot` — where to start: the node with the most time of its own (or, on a plan only
//! estimated, the most cost). Always the first finding (`Info`), whenever a node weighs
//! anything; nothing to fix by itself.
//!
//! Evidence: the step, its share, its own time (or cost).

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::plan::Measure;
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "hot-spot";

pub struct HotSpot;

impl Rule for HotSpot {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.totals.measure.is_some()
    }

    fn variants(&self) -> &'static [&'static str] {
        &["cost"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let by_time = cx.report.totals.measure == Some(Measure::Time);
        let best = cx
            .nodes()
            .filter_map(|(id, _)| {
                let d = cx.derived(id);
                let share = if by_time { d.self_pct } else { d.cost_pct }?;
                Some((id, share))
            })
            .filter(|(_, share)| *share > 0.0)
            .fold(None, |best: Option<(_, f64)>, (id, share)| match best {
                Some((_, b)) if b >= share => best,
                _ => Some((id, share)),
            });
        let Some((id, share)) = best else { return Vec::new() };
        let d = cx.derived(id);
        let mut f = Finding::new(ID, Severity::Info)
            .node(id)
            .impact(share)
            .text(E::Step, cx.step(id))
            .with(E::Share, share * 100.0, Unit::Percent)
            .at(cx.node_range(id));
        if by_time {
            f = f.with(E::Time, d.self_ms.unwrap_or(0.0), Unit::Millis);
        } else {
            f = f.with(E::Cost, d.self_cost.unwrap_or(0.0), Unit::Cost).variant("cost");
        }
        vec![f]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/hot_spot.rs"]
mod tests;
