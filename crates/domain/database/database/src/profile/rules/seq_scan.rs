//! `seq-scan-selective` — a sequential scan reading many rows to keep few: an index on what
//! its filter compares would read only those.
//!
//! - **Evidence:** a `SeqScan` with a filter, having read at least
//!   [`Thresholds::seq_scan_rows`](super::Thresholds) rows (kept and removed, in all loops;
//!   on a plan only estimated, the table's rows from the facts) and kept at most
//!   `seq_scan_kept` (10 %) of them. A scan keeping most of its table is right as it is, and a
//!   small table is read faster whole.
//! - **Severity:** `Warning`; `Critical` when the scan takes over `critical_share` (30 %) of
//!   the time.
//! - **Suggestion:** an index on the filter's equalities, then its range (only when the
//!   table's indexes are known and none starts so). When one already exists, why it isn't
//!   used: a function around the column, a cast, the statistics, or rows too many to be
//!   worth it.

use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "seq-scan-selective";

pub struct SeqScanSelective;

/// What a scan read and kept (in all its loops): `None` when it can't be known.
pub(super) fn read_and_kept(
    cx: &RuleContext<'_>,
    id: crate::profile::NodeId,
) -> Option<(f64, f64)> {
    let node = cx.node(id);
    let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
    if node.ran() {
        let kept = node.actuals.rows? * loops;
        let removed = node.actuals.rows_removed_by_filter.unwrap_or(0.0) * loops;
        return Some((kept + removed, kept));
    }
    // Estimated: the table's rows against the planner's.
    let rows = cx.table_facts(id)?.row_count()? as f64;
    Some((rows, node.estimates.rows?.min(rows)))
}

impl Rule for SeqScanSelective {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.op == OpKind::SeqScan)
    }

    fn variants(&self) -> &'static [&'static str] {
        &["index_function", "index_cast", "index_statistics", "index_selectivity"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::SeqScan || node.condition(&CondKind::Filter).is_none() {
                continue;
            }
            if node.actuals.never_executed {
                continue;
            }
            let Some((read, kept)) = read_and_kept(cx, id) else { continue };
            if read < t.seq_scan_rows || kept > read * t.seq_scan_kept {
                continue;
            }
            let share = cx.inclusive_share(id);
            let severity =
                if share > t.critical_share { Severity::Critical } else { Severity::Warning };
            let predicates = cx.predicates(id, &[CondKind::Filter]);
            let columns = cx.index_columns(id, &predicates);
            let mut f = Finding::new(ID, severity)
                .node(id)
                .impact(share)
                .text(E::Step, cx.step(id))
                .with(E::Read, read, Unit::Rows)
                .with(E::Kept, kept, Unit::Rows)
                .with(E::KeptShare, kept / read.max(1.0) * 100.0, Unit::Percent)
                .with(E::Share, share * 100.0, Unit::Percent);
            if !columns.is_empty() {
                f = f.text(E::Columns, columns.join(", "));
            }
            f = f.at(columns
                .first()
                .and_then(|c| cx.column_range(id, c))
                .or_else(|| cx.node_range(id)));
            let Some(table) = cx.table_of(id) else {
                out.push(f);
                continue;
            };
            f = f.text(E::Table, table.to_string());
            let facts = cx.facts.table(&table).filter(|f| f.knows_indexes());
            // The columns compared, however they're written (an index on any of them).
            let existing = facts.and_then(|facts| {
                predicates.iter().find_map(|p| {
                    facts.index_starting_with(std::slice::from_ref(&p.column.name)).map(|i| (*p, i))
                })
            });
            match existing {
                Some((p, index)) => {
                    f = f.text(E::Index, index.name.clone()).text(E::Column, p.column.name.clone());
                    if let Some(w) = &p.wrapper {
                        f = f.variant("index_function").text(E::Function, w.function.clone());
                    } else if let Some(ty) = p.cast.as_ref().filter(|_| !cx.plain(id, p)) {
                        f = f.variant("index_cast").text(E::Type, ty.clone());
                    } else if cx.derived(id).misestimate.is_some_and(|m| {
                        m.max(1.0 / m.max(f64::MIN_POSITIVE)) >= t.misestimate_factor
                    }) {
                        f = f.variant("index_statistics").suggest(cx.analyze_sql(&table));
                    } else {
                        f = f.variant("index_selectivity");
                    }
                }
                None if !columns.is_empty() => {
                    f = f.suggest(cx.create_index(&table, IndexSpec::on(columns)));
                }
                None => {}
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/seq_scan.rs"]
mod tests;
