//! `sqlite-temp-btree` — SQLite sorts (or groups, or removes duplicates) in a temporary
//! B-tree (`USE TEMP B-TREE FOR ORDER BY`): every row is put in it before the first comes
//! out. An index in that order gives them sorted.
//!
//! - **Evidence:** a `TempBTree` over at least
//!   [`Thresholds::temp_btree_rows`](super::Thresholds) rows — the statement's rows, the real
//!   or estimated rows of the scans beside it, or their table's rows. Sorting a few rows is
//!   nothing.
//! - **Severity:** `Info`; `Warning` from `temp_btree_warning_rows` (100 000).
//! - **Suggestion:** for `ORDER BY` / `GROUP BY` of plain columns of one table, an index on
//!   the `WHERE`'s equalities of that table, then those columns in that order (`DESC` kept).

use super::sqlite_scan::is_sqlite;
use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{CondKind, NodeId, OpKind};
use crate::profile::sql_map::ClauseKind;

pub(super) const ID: &str = "sqlite-temp-btree";

pub struct SqliteTempBTree;

/// The scans beside `id` (its siblings).
fn scans_beside(cx: &RuleContext<'_>, id: NodeId) -> Vec<NodeId> {
    let siblings = match cx.node(id).parent {
        Some(p) => cx.node(p).children.clone(),
        None => cx.profile().roots.clone(),
    };
    siblings.into_iter().filter(|s| *s != id && cx.node(*s).op.is_scan()).collect()
}

impl Rule for SqliteTempBTree {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        is_sqlite(report) && report.profile.nodes.iter().any(|n| n.op == OpKind::TempBTree)
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::TempBTree {
                continue;
            }
            let purpose = node
                .condition(&CondKind::SortKey)
                .or_else(|| node.condition(&CondKind::GroupKey))
                .unwrap_or("")
                .to_owned();
            let scans = scans_beside(cx, id);
            let mut rows = cx.profile().statement.rows_returned.map(|r| r as f64);
            for s in &scans {
                let n = cx.node(*s);
                let r = n
                    .total_rows()
                    .or(n.estimates.rows)
                    .or_else(|| cx.table_facts(*s).and_then(|f| f.row_count()).map(|r| r as f64));
                rows = match (rows, r) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
            }
            let Some(rows) = rows.filter(|r| *r >= t.temp_btree_rows) else { continue };
            let severity =
                if rows >= t.temp_btree_warning_rows { Severity::Warning } else { Severity::Info };
            let mut f = Finding::new(ID, severity)
                .node(id)
                .impact(cx.inclusive_share(id).max(cx.self_share(id)))
                .text(E::Step, cx.step(id))
                .text(E::Purpose, purpose.clone())
                .with(E::Read, rows, Unit::Rows);
            let kind = if purpose.contains("GROUP BY") {
                Some(ClauseKind::GroupBy)
            } else if purpose.contains("ORDER BY") {
                Some(ClauseKind::OrderBy)
            } else {
                None
            };
            f = f.at(kind
                .and_then(|k| cx.map.top(k))
                .map(|c| cx.to_document(c.keyword.start..c.range.end.max(c.keyword.end))));
            // One table sorted: its index in that order.
            let scan = match scans.as_slice() {
                [one] => Some(*one),
                _ => None,
            };
            if let (Some(kind), Some(scan)) = (kind, scan)
                && let Some(table) = cx.table_of(scan)
            {
                let qualifiers = cx.qualifiers(scan);
                let refs: Vec<&str> = qualifiers.iter().map(String::as_str).collect();
                if let Some(sorted) = cx.map.sort_columns(kind, &refs) {
                    let equal = cx.index_columns(
                        scan,
                        &cx.where_predicates(scan)
                            .into_iter()
                            .filter(|p| p.compare.is_equality())
                            .collect::<Vec<_>>(),
                    );
                    let mut columns = equal.clone();
                    let mut descending = vec![false; equal.len()];
                    for (c, desc) in sorted {
                        if !columns.iter().any(|x| x.eq_ignore_ascii_case(&c)) {
                            columns.push(c);
                            descending.push(desc);
                        }
                    }
                    f = f.text(E::Table, table.to_string()).text(E::Columns, columns.join(", "));
                    let spec = IndexSpec { descending, ..IndexSpec::on(columns) };
                    f = f.suggest(cx.create_index(&table, spec));
                }
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/sqlite_temp_btree.rs"]
mod tests;
