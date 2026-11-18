//! `sqlite-scan` — SQLite reads a whole table (`SCAN t`, not through an index) where an
//! index would find the rows: inside a loop (each outer row reads it again) or with a
//! `WHERE` on it.
//!
//! - **Evidence:** a SQLite `SeqScan` of a table that's the inner one of a loop (a scan
//!   before it at its level, or its real loops > 1), or that the statement's `WHERE`
//!   compares. A table read whole for all its rows (no `WHERE`, not in a loop) is right, and
//!   so is one of fewer than `small_table_rows` (1 000) rows read in all (as measured, or as
//!   its statistics say; a plan only estimated, without them, is said).
//!   Made stronger by the statement's counters: full-scan steps at least
//!   [`Thresholds::sqlite_fullscan_factor`](super::Thresholds) times the rows returned.
//! - **Severity:** `Warning`.
//! - **Suggestion:** an index on the `WHERE`'s columns of the table, else on the column it's
//!   joined by (its indexes known, none starting so).

use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{ChildRole, NodeId, ObjectKind, OpKind};

pub(super) const ID: &str = "sqlite-scan";

pub struct SqliteScan;

pub(super) fn is_sqlite(report: &ProfileReport) -> bool {
    let e = &report.profile.engine;
    e.driver == "sqlite" || e.name.eq_ignore_ascii_case("sqlite")
}

/// A scan before `id` among its siblings: `id` is the inner side of SQLite's loop.
fn in_loop(cx: &RuleContext<'_>, id: NodeId) -> bool {
    let node = cx.node(id);
    if node.actuals.loops.is_some_and(|l| l > 1.0) {
        return true;
    }
    let siblings = match node.parent {
        Some(p) => &cx.node(p).children,
        None => &cx.profile().roots,
    };
    let before = siblings.iter().take_while(|s| **s != id);
    before
        .map(|s| cx.node(*s))
        .any(|s| s.op.is_scan() && !matches!(s.role, Some(ChildRole::SubPlan | ChildRole::Cte)))
}

impl Rule for SqliteScan {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        is_sqlite(report) && report.profile.nodes.iter().any(|n| n.op == OpKind::SeqScan)
    }

    fn variants(&self) -> &'static [&'static str] {
        &["loop"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let counters = cx.profile().statement.counters;
        let returned = cx.profile().statement.rows_returned;
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::SeqScan
                || node.object.as_ref().is_none_or(|o| o.kind != ObjectKind::Table)
            {
                continue;
            }
            let looped = in_loop(cx, id);
            let filtered = cx.where_predicates(id);
            if !looped && filtered.is_empty() {
                continue;
            }
            let Some(table) = cx.table_of(id) else { continue };
            // A small table is read faster whole (its rows as measured, else as known).
            let rows = node
                .total_rows()
                .or_else(|| cx.facts.table(&table).and_then(|f| f.row_count()).map(|r| r as f64));
            if rows.is_some_and(|r| r < t.small_table_rows) {
                continue;
            }
            let share = cx.inclusive_share(id);
            let mut f = Finding::new(ID, Severity::Warning)
                .node(id)
                .text(E::Step, cx.step(id))
                .text(E::Table, table.to_string())
                .at(cx.node_range(id));
            if looped {
                f = f.variant("loop");
            }
            let mut impact = share;
            if let Some(steps) = counters.and_then(|c| c.fullscan_steps) {
                let rows = returned.unwrap_or(0);
                f = f.with(E::Fullscan, steps as f64, Unit::Count).with(
                    E::Returned,
                    rows as f64,
                    Unit::Rows,
                );
                if steps as f64 >= t.sqlite_fullscan_factor * (rows.max(1) as f64) {
                    impact = impact.max(0.5);
                }
            }
            if let Some(rows) = node.total_rows() {
                f = f.with(E::Read, rows, Unit::Rows);
            }
            let mut columns = cx.index_columns(id, &filtered);
            if columns.is_empty() || looped {
                // The column it's joined by.
                let qualifiers = cx.qualifiers(id);
                let mine = |c: &crate::profile::cond::ColumnRef| {
                    c.qualifier
                        .as_ref()
                        .is_some_and(|q| qualifiers.iter().any(|x| x.eq_ignore_ascii_case(q)))
                };
                let joined = cx.statement_joins.iter().find_map(|p| {
                    if mine(&p.left) && !mine(&p.right) {
                        Some(p.left.name.clone())
                    } else if mine(&p.right) && !mine(&p.left) {
                        Some(p.right.name.clone())
                    } else {
                        None
                    }
                });
                if looped && let Some(j) = joined {
                    f = f.text(E::Column, j.clone());
                    columns = vec![j];
                }
            }
            if let Some(first) = columns.first() {
                f = f.text(E::Columns, columns.join(", "));
                if let Some(r) = cx.column_range(id, first) {
                    f = f.at(Some(r));
                }
            }
            f = f.impact(impact);
            if !columns.is_empty() {
                f = f.suggest(cx.create_index(&table, IndexSpec::on(columns)));
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/sqlite_scan.rs"]
mod tests;
