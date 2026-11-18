//! `dead-tuples` — a table holds many dead rows (updated or deleted, not yet vacuumed): every
//! scan reads past them.
//!
//! - **Evidence:** the server's statistics of a table the profile reads (39h6's
//!   `table_stats`): dead rows at least [`Thresholds::dead_share`](super::Thresholds) (20 %) of
//!   the live ones, on a table of at least `small_table_rows`. Without them nothing is said.
//! - **Severity:** `Info`; `Warning` from `dead_warning_share` (50 %).
//! - **Suggestion:** `VACUUM (ANALYZE)` of the table.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "dead-tuples";

pub struct DeadTuples;

impl Rule for DeadTuples {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, _report: &ProfileReport) -> bool {
        true
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        for facts in &cx.facts.tables {
            let Some(stats) = &facts.stats else { continue };
            let (Some(dead), Some(live)) = (stats.dead_rows, stats.live_rows) else { continue };
            let (dead, live) = (dead as f64, live as f64);
            if live + dead < t.small_table_rows || dead < live.max(1.0) * t.dead_share {
                continue;
            }
            let share = dead / live.max(1.0);
            let severity =
                if share >= t.dead_warning_share { Severity::Warning } else { Severity::Info };
            let mut f = Finding::new(ID, severity)
                .text(E::Table, facts.table.to_string())
                .with(E::Dead, dead, Unit::Rows)
                .with(E::Live, live, Unit::Rows)
                .with(E::DeadShare, share * 100.0, Unit::Percent)
                .suggest(cx.vacuum_sql(&facts.table));
            let mut impact: f64 = 0.0;
            for (id, _) in cx.nodes() {
                if cx.node(id).op.is_scan()
                    && cx.table_of(id).is_some_and(|x| super::same_table(&x, &facts.table))
                {
                    impact = impact.max(cx.inclusive_share(id));
                    if f.nodes.is_empty() {
                        f = f.at(cx.node_range(id));
                    }
                    f = f.node(id);
                }
            }
            out.push(f.impact(impact));
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/dead_tuples.rs"]
mod tests;
