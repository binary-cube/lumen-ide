//! `misestimate` — the planner expected far other rows than came: its other choices (the
//! join's method, the order, the memory) rest on that number.
//!
//! - **Evidence:** a node run, its real rows per loop over the estimated ones at least
//!   [`Thresholds::misestimate_factor`](super::Thresholds) either way, with at least
//!   `misestimate_rows` (100) on one side — a guess of 3 rows against 30 changes nothing.
//!   Only where it starts: a node whose child already misses as much is that child's
//!   consequence. Under a `LIMIT`, fewer rows than expected are the limit's doing.
//! - **Severity:** `Warning`; its impact is its parent join's share when it feeds one
//!   (variant `join`).
//! - **Suggestion:** `ANALYZE` of its table; with two columns or more of it in its filter,
//!   extended statistics (PostgreSQL 10+). The table's statistics, when read, are evidence.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity, suggest};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{CondKind, NodeId, OpKind};

pub(super) const ID: &str = "misestimate";

pub struct Misestimate;

/// How far off a node's estimate is, as a factor ≥ 1, and whether more came than expected.
fn off(cx: &RuleContext<'_>, id: NodeId) -> Option<(f64, bool)> {
    let m = cx.derived(id).misestimate?;
    if m >= 1.0 { Some((m, true)) } else { Some((1.0 / m.max(f64::MIN_POSITIVE), false)) }
}

fn is_join(op: OpKind) -> bool {
    matches!(op, OpKind::NestedLoop | OpKind::HashJoin | OpKind::MergeJoin)
}

impl Rule for Misestimate {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.depth.runs() && report.derived.iter().any(|d| d.misestimate.is_some())
    }

    fn variants(&self) -> &'static [&'static str] {
        &["join"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            let Some((factor, more)) = off(cx, id) else { continue };
            let (Some(real), Some(est)) = (node.actuals.rows, node.estimates.rows) else {
                continue;
            };
            if factor < t.misestimate_factor || real.max(est) < t.misestimate_rows {
                continue;
            }
            // A child missing as much the same way: it started there.
            let inherited = node
                .children
                .iter()
                .any(|c| off(cx, *c).is_some_and(|(f, m)| m == more && f >= t.misestimate_factor));
            if inherited {
                continue;
            }
            if !more && cx.ancestors(id).any(|a| cx.node(a).op == OpKind::Limit) {
                continue;
            }
            let parent_join = node.parent.filter(|p| is_join(cx.node(*p).op));
            let impact =
                parent_join.map_or_else(|| cx.inclusive_share(id), |p| cx.inclusive_share(p));
            let mut f = Finding::new(ID, Severity::Warning)
                .node(id)
                .impact(impact)
                .text(E::Step, cx.step(id))
                .with(E::Estimated, est, Unit::Rows)
                .with(E::Actual, real, Unit::Rows)
                .with(E::Factor, cx.derived(id).misestimate.unwrap_or(1.0), Unit::Ratio)
                .at(cx.node_range(id));
            if let Some(p) = parent_join {
                f = f.node(p).variant("join").text(E::Target, cx.step(p));
            }
            if let Some(table) = cx.table_of(id) {
                f = f.text(E::Table, table.to_string());
                if let Some(stats) = cx.facts.table(&table).and_then(|t| t.stats.as_ref()) {
                    if let Some(last) =
                        stats.last_analyze.as_ref().or(stats.last_autoanalyze.as_ref())
                    {
                        f = f.text(E::Value, last.clone());
                    }
                    if let Some(m) = stats.modified_since_analyze {
                        f = f.with(E::Modified, m as f64, Unit::Rows);
                    }
                }
                let columns: Vec<String> = {
                    let mut c: Vec<String> = Vec::new();
                    for p in cx
                        .predicates(id, &[CondKind::Filter, CondKind::IndexCond, CondKind::Recheck])
                    {
                        if cx.plain(id, p)
                            && !c.iter().any(|x| x.eq_ignore_ascii_case(&p.column.name))
                        {
                            c.push(p.column.name.clone());
                        }
                    }
                    c
                };
                let statistics = (columns.len() >= 2)
                    .then(|| suggest::statistics(&*cx.dialect, cx.version(), &table, &columns))
                    .flatten();
                f = f.suggest(statistics.or_else(|| cx.analyze_sql(&table)));
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/misestimate.rs"]
mod tests;
