//! `or-defeats-index` — an `OR` between different columns makes a table be read whole,
//! though each column has its own index.
//!
//! - **Evidence:** a `SeqScan` whose filter (SQLite: the statement's `WHERE`) is an `OR` at
//!   its top whose branches compare different columns of its table, each with an index
//!   starting with it. The same column in every branch (`a = 1 OR a = 2`) is an `IN`, which
//!   its index serves; SQLite's `MULTI-INDEX OR` already uses both indexes.
//! - **Severity:** `Info`.
//! - **Suggestion:** a `UNION ALL` of one query per branch (PostgreSQL may also combine the
//!   indexes in a `BitmapOr` once its statistics are right).

use super::{EvidenceKey as E, Finding, RewriteHint, Rule, RuleContext, Severity, Suggestion};
use crate::profile::cond::ColumnRef;
use crate::profile::derive::ProfileReport;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "or-defeats-index";

pub struct OrDefeatsIndex;

impl Rule for OrDefeatsIndex {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.op == OpKind::SeqScan)
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::SeqScan {
                continue;
            }
            let Some(table) = cx.table_of(id) else { continue };
            let Some(facts) = cx.facts.table(&table).filter(|f| f.knows_indexes()) else {
                continue;
            };
            let qualifiers = cx.qualifiers(id);
            let mine = |c: &ColumnRef| match &c.qualifier {
                None => facts.has_column(&c.name) != Some(false),
                Some(q) => qualifiers.iter().any(|x| x.eq_ignore_ascii_case(q)),
            };
            let mut branches: Vec<&Vec<ColumnRef>> =
                cx.conditions(id, &[CondKind::Filter]).flat_map(|c| c.or_branches.iter()).collect();
            if branches.is_empty() && cx.is_sqlite() {
                branches = cx.where_condition.iter().flat_map(|c| c.or_branches.iter()).collect();
            }
            if branches.len() < 2 {
                continue;
            }
            // Each branch's one column of this table.
            let mut columns: Vec<String> = Vec::new();
            let mut each = true;
            for b in &branches {
                let own: Vec<&ColumnRef> = b.iter().filter(|c| mine(c)).collect();
                match own.first() {
                    Some(c) if own.iter().all(|x| x.is(&c.name)) => {
                        if !columns.iter().any(|x| c.is(x)) {
                            columns.push(c.name.clone());
                        }
                    }
                    _ => each = false,
                }
            }
            if !each || columns.len() < 2 {
                continue;
            }
            if !columns.iter().all(|c| facts.index_starting_with(std::slice::from_ref(c)).is_some())
            {
                continue;
            }
            out.push(
                Finding::new(ID, Severity::Info)
                    .node(id)
                    .impact(cx.inclusive_share(id))
                    .text(E::Step, cx.step(id))
                    .text(E::Table, table.to_string())
                    .text(E::Columns, columns.join(", "))
                    .at(cx.node_range(id))
                    .suggest(Some(Suggestion::Rewrite { hint: RewriteHint::UnionAll })),
            );
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/or_defeats_index.rs"]
mod tests;
