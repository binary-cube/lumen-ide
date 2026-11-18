//! `fk-join-no-index` — a join on a foreign key whose columns no index starts with: the
//! referencing table is read whole to find a row's children (and every delete of a parent
//! row scans it too).
//!
//! - **Evidence:** a join comparing a foreign key's columns with the ones it references —
//!   the engine's key, or a virtual one the user accepted (plan 37) — the referencing table's
//!   indexes known with none starting with the key's columns, and that table scanned whole
//!   under the join. Without the table's structure nothing is said.
//! - **Severity:** `Warning` (variant `virtual` for an accepted virtual key).
//! - **Suggestion:** an index on the key's columns.

use super::facts::{KeyFact, same_table};
use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::TableRef;
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{CondKind, NodeId, OpKind};

pub(super) const ID: &str = "fk-join-no-index";

pub struct FkJoinNoIndex;

fn is_join(op: OpKind) -> bool {
    matches!(op, OpKind::NestedLoop | OpKind::HashJoin | OpKind::MergeJoin)
}

/// The most nodes looked at under one join (a join over thousands of partitions is read
/// as far as this).
const MAX_UNDER: usize = 4096;

/// The scans under `id` (itself included), depth first.
fn scans_under(cx: &RuleContext<'_>, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    let mut seen = 0;
    while let Some(at) = stack.pop() {
        seen += 1;
        if seen > MAX_UNDER.min(cx.profile().nodes.len()) {
            break;
        }
        let n = cx.node(at);
        if n.op.is_scan() {
            out.push(at);
        }
        stack.extend(n.children.iter().rev().copied());
    }
    out
}

/// The table a column of a join's condition is of: by its qualifier, else the one scan under
/// the join whose table has such a column.
fn column_table(
    cx: &RuleContext<'_>,
    scans: &[NodeId],
    column: &crate::profile::cond::ColumnRef,
) -> Option<TableRef> {
    if let Some(q) = &column.qualifier {
        if let Some(found) = cx.map.resolve(q) {
            return Some(found.table.clone());
        }
        return scans.iter().find_map(|s| {
            cx.qualifiers(*s)
                .iter()
                .any(|x| x.eq_ignore_ascii_case(q))
                .then(|| cx.table_of(*s))
                .flatten()
        });
    }
    let mut owners = scans.iter().filter_map(|s| {
        let t = cx.table_of(*s)?;
        (cx.facts.table(&t)?.has_column(&column.name) == Some(true)).then_some(t)
    });
    let first = owners.next()?;
    owners.all(|t| same_table(&t, &first)).then_some(first)
}

impl Rule for FkJoinNoIndex {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| is_join(n.op))
    }

    fn variants(&self) -> &'static [&'static str] {
        &["virtual"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        if cx.facts.tables.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<Finding> = Vec::new();
        let mut told: Vec<(TableRef, Vec<String>)> = Vec::new();
        for (id, node) in cx.nodes() {
            if !is_join(node.op) {
                continue;
            }
            let scans = scans_under(cx, id);
            // The pairs of columns this join compares, each with its table.
            let mut pairs: Vec<(TableRef, String, TableRef, String)> = Vec::new();
            let own =
                cx.conditions(id, &[CondKind::HashCond, CondKind::MergeCond, CondKind::JoinFilter]);
            let inner = node
                .children
                .iter()
                .flat_map(|c| cx.conditions(*c, &[CondKind::IndexCond, CondKind::Filter]));
            for pair in own.chain(inner).flat_map(|c| c.joins.iter()) {
                let (Some(a), Some(b)) =
                    (column_table(cx, &scans, &pair.left), column_table(cx, &scans, &pair.right))
                else {
                    continue;
                };
                pairs.push((a, pair.left.name.clone(), b, pair.right.name.clone()));
            }
            if pairs.is_empty() {
                continue;
            }
            let compares = |child: &TableRef, column: &str, target: &TableRef, reference: &str| {
                pairs.iter().any(|(a, ac, b, bc)| {
                    (same_table(a, child)
                        && ac.eq_ignore_ascii_case(column)
                        && same_table(b, target)
                        && bc.eq_ignore_ascii_case(reference))
                        || (same_table(b, child)
                            && bc.eq_ignore_ascii_case(column)
                            && same_table(a, target)
                            && ac.eq_ignore_ascii_case(reference))
                })
            };
            let mut tables: Vec<TableRef> = Vec::new();
            for (a, _, b, _) in &pairs {
                for t in [a, b] {
                    if !tables.iter().any(|x| same_table(x, t)) {
                        tables.push(t.clone());
                    }
                }
            }
            for child in &tables {
                let Some(facts) = cx.facts.table(child).filter(|f| f.knows_indexes()) else {
                    continue;
                };
                for key in cx.facts.foreign_keys(child) {
                    let KeyFact { columns, target, references, accepted } = &key;
                    if columns.is_empty() || columns.len() != references.len() {
                        continue;
                    }
                    let joined =
                        columns.iter().zip(references).all(|(c, r)| compares(child, c, target, r));
                    if !joined || facts.index_starting_with(columns).is_some() {
                        continue;
                    }
                    // The referencing side read whole under this join.
                    let Some(scan) = scans.iter().copied().find(|s| {
                        cx.node(*s).op == OpKind::SeqScan
                            && cx.table_of(*s).is_some_and(|t| same_table(&t, child))
                    }) else {
                        continue;
                    };
                    if told.iter().any(|(t, c)| same_table(t, child) && c == columns) {
                        continue;
                    }
                    told.push((child.clone(), columns.clone()));
                    let share = cx.inclusive_share(scan);
                    let mut f = Finding::new(ID, Severity::Warning)
                        .node(id)
                        .node(scan)
                        .impact(share)
                        .text(E::Step, cx.step(scan))
                        .text(E::Table, child.to_string())
                        .text(E::Columns, columns.join(", "))
                        .text(E::Target, format!("{target}({})", references.join(", ")))
                        .with(E::Share, share * 100.0, Unit::Percent)
                        .at(cx.column_range(scan, &columns[0]))
                        .suggest(cx.create_index(child, IndexSpec::on(columns.clone())));
                    if *accepted {
                        f = f.variant("virtual");
                    }
                    out.push(f);
                }
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/fk_join_no_index.rs"]
mod tests;
