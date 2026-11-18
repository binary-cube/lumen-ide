//! `function-on-column` — a condition wraps an indexed column in a function
//! (`lower(email) = '…'`): the index on the column can't be used for it.
//!
//! - **Evidence:** a scan's filter (SQLite: the statement's `WHERE` on a table scanned
//!   whole) comparing `f(column)` with a value, the column's table having an index starting
//!   with it and none on an expression of it. Without its indexes known nothing is said.
//! - **Severity:** `Warning`.
//! - **Suggestion:** an index on the expression (PostgreSQL; SQLite 3.9+), or the condition
//!   written on the column itself (a function of several arguments).

use super::{
    EvidenceKey as E, Finding, IndexSpec, RewriteHint, Rule, RuleContext, Severity, Suggestion,
};
use crate::profile::derive::ProfileReport;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "function-on-column";

pub struct FunctionOnColumn;

impl Rule for FunctionOnColumn {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.op.is_scan())
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if !node.op.is_scan() {
                continue;
            }
            let Some(table) = cx.table_of(id) else { continue };
            let Some(facts) = cx.facts.table(&table).filter(|f| f.knows_indexes()) else {
                continue;
            };
            let mut predicates = cx.predicates(id, &[CondKind::Filter]);
            if cx.is_sqlite() && node.op == OpKind::SeqScan {
                predicates.extend(cx.where_predicates(id));
            }
            for p in predicates {
                let Some(w) = &p.wrapper else { continue };
                let Some(index) = facts.index_starting_with(std::slice::from_ref(&p.column.name))
                else {
                    continue;
                };
                if facts.expression_index_on(&p.column.name).is_some() {
                    continue;
                }
                let mut f = Finding::new(ID, Severity::Warning)
                    .node(id)
                    .impact(cx.inclusive_share(id))
                    .text(E::Step, cx.step(id))
                    .text(E::Table, table.to_string())
                    .text(E::Column, p.column.name.clone())
                    .text(E::Function, w.function.clone())
                    .text(E::Index, index.name.clone())
                    .at(cx.column_range(id, &p.column.name));
                let suggestion = if w.single {
                    cx.dialect.quote_identifier(&p.column.name).ok().and_then(|q| {
                        let spec = IndexSpec {
                            expression: Some(format!("{}({q})", w.function)),
                            ..IndexSpec::on(vec![p.column.name.clone()])
                        };
                        cx.create_index(&table, spec)
                    })
                } else {
                    Some(Suggestion::Rewrite { hint: RewriteHint::UnwrapColumn })
                };
                f = f.suggest(suggestion);
                out.push(f);
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/function_on_column.rs"]
mod tests;
