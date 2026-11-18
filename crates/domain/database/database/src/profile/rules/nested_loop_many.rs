//! `nested-loop-many` — a nested loop running its inner side many times: each loop pays for
//! it again, a full scan of the inner table each time.
//!
//! - **Evidence:** a `NestedLoop` whose inner side (its `Inner` child, else its second) ran
//!   at least [`Thresholds::nested_loops`](super::Thresholds) times (on a plan only estimated:
//!   the outer side's rows), and is a full scan or takes `nested_share` (20 %) of the time. A
//!   loop of one iteration is a lookup, not a problem.
//! - **Severity:** `Warning`; `Critical` for a full scan taking over `critical_share`.
//! - **Suggestion:** an index on the inner side's join column (its table's indexes known);
//!   when the outer side brought far more rows than the planner expected (variant
//!   `outer_misestimate`), its estimate is the cause: `ANALYZE` of its table.

use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{ChildRole, CondKind, NodeId, OpKind};

pub(super) const ID: &str = "nested-loop-many";

pub struct NestedLoopMany;

/// A join's outer and inner sides (its subplans aside).
pub(super) fn sides(cx: &RuleContext<'_>, id: NodeId) -> Option<(NodeId, NodeId)> {
    let node = cx.node(id);
    let plain: Vec<NodeId> = node
        .children
        .iter()
        .copied()
        .filter(|c| !matches!(cx.node(*c).role, Some(ChildRole::InitPlan | ChildRole::SubPlan)))
        .collect();
    let inner = plain
        .iter()
        .copied()
        .find(|c| cx.node(*c).role == Some(ChildRole::Inner))
        .or_else(|| plain.get(1).copied())?;
    let outer = plain.iter().copied().find(|c| *c != inner)?;
    Some((outer, inner))
}

/// The inner side's column a join compares, and what it's compared with.
pub(super) fn join_column(
    cx: &RuleContext<'_>,
    join: NodeId,
    inner: NodeId,
) -> Option<(String, String)> {
    let object = cx.object(inner)?.clone();
    let qualifiers = cx.qualifiers(inner);
    let mine = |c: &crate::profile::cond::ColumnRef| match &c.qualifier {
        None => true,
        Some(q) => qualifiers.iter().any(|x| x.eq_ignore_ascii_case(q)),
    };
    let kinds = [CondKind::JoinFilter, CondKind::HashCond, CondKind::MergeCond];
    let inner_kinds = [CondKind::Filter, CondKind::IndexCond, CondKind::Recheck];
    let pairs = cx
        .conditions(join, &kinds)
        .chain(cx.conditions(inner, &inner_kinds))
        .flat_map(|c| c.joins.iter());
    for pair in pairs {
        let (a, b) = (&pair.left, &pair.right);
        let (m, o) = match (
            mine(a) && b.qualifier.is_some() && !mine(b),
            mine(b) && a.qualifier.is_some() && !mine(a),
        ) {
            (true, _) => (a, b),
            (_, true) => (b, a),
            _ => match pair.side_of(&object) {
                Some(found) => found,
                None => continue,
            },
        };
        let other = match &o.qualifier {
            Some(q) => format!("{q}.{}", o.name),
            None => o.name.clone(),
        };
        return Some((m.name.clone(), other));
    }
    // An index condition against an outer value (`id = $1`, SQLite's `id=?`).
    for p in cx.predicates(inner, &[CondKind::IndexCond]) {
        if cx.plain(inner, p) && !p.literal {
            return Some((p.column.name.clone(), String::new()));
        }
    }
    None
}

impl Rule for NestedLoopMany {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.op == OpKind::NestedLoop)
    }

    fn variants(&self) -> &'static [&'static str] {
        &["outer_misestimate"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::NestedLoop {
                continue;
            }
            let Some((outer, inner)) = sides(cx, id) else { continue };
            let inner_node = cx.node(inner);
            let loops = if inner_node.ran() {
                inner_node.actuals.loops
            } else {
                cx.node(outer).estimates.rows
            };
            let Some(loops) = loops.filter(|l| *l >= t.nested_loops) else { continue };
            let full = inner_node.op == OpKind::SeqScan;
            let share = cx.inclusive_share(inner);
            if !full && share < t.nested_share {
                continue;
            }
            let severity = if full && share > t.critical_share {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let mut f = Finding::new(ID, severity)
                .node(id)
                .node(inner)
                .impact(share.max(cx.inclusive_share(id) * 0.5))
                .text(E::Step, cx.step(inner))
                .with(E::Loops, loops, Unit::Count)
                .with(E::Share, share * 100.0, Unit::Percent)
                .at(cx.node_range(inner));
            let outer_off =
                cx.derived(outer).misestimate.is_some_and(|m| m >= t.misestimate_factor);
            if let Some(table) = cx.table_of(inner) {
                f = f.text(E::Table, table.to_string());
                if let Some((column, other)) = join_column(cx, id, inner) {
                    f = f.text(E::Column, column.clone());
                    if !other.is_empty() {
                        f = f.text(E::Target, other);
                    }
                    if let Some(r) = cx.column_range(inner, &column) {
                        f = f.at(Some(r));
                    }
                    if full {
                        f = f.suggest(cx.create_index(&table, IndexSpec::on(vec![column])));
                    }
                }
            }
            if outer_off {
                f = f.variant("outer_misestimate").with(
                    E::Factor,
                    cx.derived(outer).misestimate.unwrap_or(1.0),
                    Unit::Ratio,
                );
                if f.suggestion.is_none()
                    && let Some(table) = cx.table_of(outer)
                {
                    f = f.suggest(cx.analyze_sql(&table));
                }
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/nested_loop_many.rs"]
mod tests;
