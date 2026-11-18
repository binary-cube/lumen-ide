//! `implicit-cast` — a condition converts an indexed column (`(code)::text = '7'`) to compare
//! it with a value of another type: its index, on the column as stored, can't be used.
//!
//! - **Evidence:** a scan's filter (not its index condition: an index used through the cast
//!   is fine) comparing a cast column with a value, the column having an index starting with
//!   it. PostgreSQL writes `(varchar_column)::text` for every text column: a text column cast
//!   to a text type isn't a conversion (its type known from the table's structure; without
//!   it nothing is said).
//! - **Severity:** `Warning`.
//! - **Suggestion:** a value of the column's own type (`MatchLiteralType`).

use super::{EvidenceKey as E, Finding, RewriteHint, Rule, RuleContext, Severity, Suggestion};
use crate::TypeClass;
use crate::profile::derive::ProfileReport;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "implicit-cast";

pub struct ImplicitCast;

impl Rule for ImplicitCast {
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
            let Some(facts) = cx.facts.table(&table) else { continue };
            let Some(structure) = facts.structure.as_ref() else { continue };
            let mut predicates = cx.predicates(id, &[CondKind::Filter]);
            if cx.is_sqlite() && node.op == OpKind::SeqScan {
                predicates.extend(cx.where_predicates(id));
            }
            for p in predicates {
                let (Some(ty), None) = (&p.cast, &p.wrapper) else { continue };
                let Some(column) = structure.columns.iter().find(|c| p.column.is(&c.name)) else {
                    continue;
                };
                if column.class == TypeClass::Text && super::texty(ty) {
                    continue;
                }
                let Some(index) = facts.index_starting_with(std::slice::from_ref(&column.name))
                else {
                    continue;
                };
                out.push(
                    Finding::new(ID, Severity::Warning)
                        .node(id)
                        .impact(cx.inclusive_share(id))
                        .text(E::Step, cx.step(id))
                        .text(E::Table, table.to_string())
                        .text(E::Column, column.name.clone())
                        .text(E::Type, ty.clone())
                        .text(E::Value, column.declared_type.clone())
                        .text(E::Index, index.name.clone())
                        .at(cx.column_range(id, &column.name))
                        .suggest(Some(Suggestion::Rewrite { hint: RewriteHint::MatchLiteralType })),
                );
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/implicit_cast.rs"]
mod tests;
