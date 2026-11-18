//! `leading-wildcard` — `LIKE '%…'` on an indexed column: a B-tree index finds a pattern only
//! by its start, so the column is read in every row.
//!
//! - **Evidence:** a scan's filter (SQLite: the statement's `WHERE`) with a `LIKE` (`~~`,
//!   `ILIKE`) whose pattern starts with `%`, on a column with an index starting with it.
//!   `LIKE 'abc%'` can use the index.
//! - **Severity:** `Info`.
//! - **Suggestion:** on PostgreSQL a trigram index (`USING gin (… gin_trgm_ops)`, variant
//!   `trigram`: it needs the `pg_trgm` extension, which the editor never installs); elsewhere
//!   a full-text search, said in words.

use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "leading-wildcard";

pub struct LeadingWildcard;

impl Rule for LeadingWildcard {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.op.is_scan())
    }

    fn variants(&self) -> &'static [&'static str] {
        &["trigram"]
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
            for p in predicates.into_iter().filter(|p| p.leading_wildcard && p.wrapper.is_none()) {
                let Some(index) = facts.index_starting_with(std::slice::from_ref(&p.column.name))
                else {
                    continue;
                };
                let mut f = Finding::new(ID, Severity::Info)
                    .node(id)
                    .impact(cx.inclusive_share(id))
                    .text(E::Step, cx.step(id))
                    .text(E::Table, table.to_string())
                    .text(E::Column, p.column.name.clone())
                    .text(E::Index, index.name.clone())
                    .at(cx.column_range(id, &p.column.name));
                if cx.is_postgres() {
                    let spec = IndexSpec {
                        method: Some(("gin".to_owned(), "gin_trgm_ops".to_owned())),
                        ..IndexSpec::on(vec![p.column.name.clone()])
                    };
                    f = f.variant("trigram").suggest(cx.create_index(&table, spec));
                }
                out.push(f);
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/leading_wildcard.rs"]
mod tests;
