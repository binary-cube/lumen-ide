//! `auto-index` — SQLite built an index for this statement alone (`AUTOMATIC … INDEX`) and
//! drops it after: each run pays for building it again.
//!
//! - **Evidence:** an `AutoIndex` step, or the statement's `autoindex` counter above zero
//!   (the rows put in automatic indexes).
//! - **Severity:** `Warning` (variant `counter` when only the counter says it).
//! - **Suggestion:** the same index, made for good, on the columns SQLite names (its table's
//!   indexes known).

use super::sqlite_scan::is_sqlite;
use super::{EvidenceKey as E, Finding, IndexSpec, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{CondKind, OpKind};

pub(super) const ID: &str = "auto-index";

pub struct AutoIndex;

fn counted(report: &ProfileReport) -> Option<u64> {
    report.profile.statement.counters.and_then(|c| c.autoindex).filter(|n| *n > 0)
}

impl Rule for AutoIndex {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        is_sqlite(report)
            && (report.profile.nodes.iter().any(|n| n.op == OpKind::AutoIndex)
                || counted(report).is_some())
    }

    fn variants(&self) -> &'static [&'static str] {
        &["counter"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let rows = counted(cx.report);
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::AutoIndex {
                continue;
            }
            let columns = cx.index_columns(id, &cx.predicates(id, &[CondKind::IndexCond]));
            let mut f = Finding::new(ID, Severity::Warning)
                .node(id)
                .impact(cx.inclusive_share(id).max(0.2))
                .text(E::Step, cx.step(id))
                .at(cx.node_range(id));
            if let Some(n) = rows {
                f = f.with(E::Read, n as f64, Unit::Rows);
            }
            if let Some(table) = cx.table_of(id) {
                f = f.text(E::Table, table.to_string());
                if !columns.is_empty() {
                    f = f
                        .text(E::Columns, columns.join(", "))
                        .suggest(cx.create_index(&table, IndexSpec::on(columns)));
                }
            }
            out.push(f);
        }
        if out.is_empty()
            && let Some(n) = rows
        {
            out.push(Finding::new(ID, Severity::Warning).variant("counter").impact(0.2).with(
                E::Read,
                n as f64,
                Unit::Rows,
            ));
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/auto_index.rs"]
mod tests;
