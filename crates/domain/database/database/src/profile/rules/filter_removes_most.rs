//! `filter-removes-most` — a scan's filter throws away most of what it read: on an index scan
//! the index doesn't cover the filter, and each row thrown away was fetched for nothing.
//!
//! - **Evidence:** any scan that ran, at least
//!   [`Thresholds::filter_removed_share`](super::Thresholds) (90 %) of its rows removed by its
//!   filter and at least `filter_removed_rows` (1 000) of them. A sequential scan big enough
//!   for `seq-scan-selective` is that rule's (not said twice).
//! - **Severity:** `Warning`.
//! - **Suggestion:** an index covering the filter too: the index condition's columns, then
//!   the filter's (composite); when the filter compares only literals (`status = 'paid'`) and
//!   the scan has an index condition, a partial index on that condition's columns with the
//!   filter as its `WHERE`.

use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "filter-removes-most";

pub struct FilterRemovesMost;

impl Rule for FilterRemovesMost {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report
            .profile
            .nodes
            .iter()
            .any(|n| n.actuals.rows_removed_by_filter.is_some_and(|r| r > 0.0))
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if !node.op.is_scan() || !node.ran() {
                continue;
            }
            let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
            let Some(removed) = node.actuals.rows_removed_by_filter.map(|r| r * loops) else {
                continue;
            };
            let kept = node.actuals.rows.unwrap_or(0.0) * loops;
            let read = kept + removed;
            if removed < t.filter_removed_rows || removed < read * t.filter_removed_share {
                continue;
            }
            if node.op == OpKind::SeqScan
                && read >= t.seq_scan_rows
                && kept <= read * t.seq_scan_kept
                && node.condition(&CondKind::Filter).is_some()
            {
                continue;
            }
            let share = cx.inclusive_share(id);
            let mut f = Finding::new(ID, Severity::Warning)
                .node(id)
                .impact(share)
                .text(E::Step, cx.step(id))
                .with(E::Read, read, Unit::Rows)
                .with(E::Removed, removed, Unit::Rows)
                .with(E::RemovedShare, removed / read.max(1.0) * 100.0, Unit::Percent)
                .at(cx.node_range(id));
            let filter = cx.predicates(id, &[CondKind::Filter]);
            let index = cx.predicates(id, &[CondKind::IndexCond]);
            if let Some(first) = filter.first() {
                let place = cx.column_range(id, &first.column.name).or_else(|| f.sql_range.clone());
                f = f.at(place);
            }
            if let Some(table) = cx.table_of(id) {
                f = f.text(E::Table, table.to_string());
                let index_columns = cx.index_columns(id, &index);
                let filter_columns = cx.index_columns(id, &filter);
                let literal_only = !filter.is_empty()
                    && filter
                        .iter()
                        .all(|p| p.literal && cx.plain(id, p) && p.compare.is_equality());
                let raw_filter = node.condition(&CondKind::Filter).map(str::to_owned);
                let spec = if literal_only && !index_columns.is_empty() && raw_filter.is_some() {
                    IndexSpec { where_clause: raw_filter, ..IndexSpec::on(index_columns.clone()) }
                } else {
                    let mut columns = index_columns.clone();
                    for c in filter_columns {
                        if !columns.iter().any(|x| x.eq_ignore_ascii_case(&c)) {
                            columns.push(c);
                        }
                    }
                    IndexSpec::on(columns)
                };
                if (!spec.columns.is_empty() && spec.columns != index_columns)
                    || spec.where_clause.is_some()
                {
                    f = f.text(E::Columns, spec.columns.join(", "));
                    f = f.suggest(cx.create_index(&table, spec));
                }
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/filter_removes_most.rs"]
mod tests;
