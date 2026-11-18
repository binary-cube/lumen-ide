//! `heap-fetches` — an `Index Only Scan` that still went to the table for many rows: the
//! table's visibility map is behind (pages changed since the last `VACUUM`).
//!
//! - **Evidence:** an `IndexOnlyScan` whose `Heap Fetches` (PostgreSQL counts them for all
//!   loops) are at least [`Thresholds::heap_fetches_share`](super::Thresholds) (10 %) of its
//!   rows in all loops.
//! - **Severity:** `Info`.
//! - **Suggestion:** `VACUUM (ANALYZE)` of its table.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::OpKind;

pub(super) const ID: &str = "heap-fetches";

pub struct HeapFetches;

impl Rule for HeapFetches {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.actuals.heap_fetches.is_some_and(|h| h > 0.0))
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::IndexOnlyScan {
                continue;
            }
            let (Some(fetches), Some(rows)) = (node.actuals.heap_fetches, node.total_rows()) else {
                continue;
            };
            if fetches <= 0.0 || fetches < rows.max(1.0) * cx.thresholds.heap_fetches_share {
                continue;
            }
            let mut f = Finding::new(ID, Severity::Info)
                .node(id)
                .impact(cx.inclusive_share(id))
                .text(E::Step, cx.step(id))
                .with(E::HeapFetches, fetches, Unit::Rows)
                .with(E::Actual, rows, Unit::Rows)
                .with(E::Share, (fetches / rows.max(1.0)).min(1.0) * 100.0, Unit::Percent)
                .at(cx.node_range(id));
            if let Some(table) = cx.table_of(id) {
                f = f.text(E::Table, table.to_string()).suggest(cx.vacuum_sql(&table));
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/heap_fetches.rs"]
mod tests;
