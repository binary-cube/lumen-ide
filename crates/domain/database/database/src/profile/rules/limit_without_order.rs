//! `limit-without-order` — a `LIMIT` (or `FETCH FIRST`) whose `SELECT` has no `ORDER BY`:
//! which rows come isn't defined, and may change from one run (or plan) to the next.
//!
//! - **Evidence:** the statement's text alone: a `LIMIT` at a level (the statement's, or a
//!   subquery's) with no `ORDER BY` at the same one. `LIMIT 1` with an `ORDER BY` is fine.
//! - **Severity:** `Info`.
//! - **Suggestion:** none to write: an `ORDER BY` only the user knows (the text says so).

use super::{Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;

pub(super) const ID: &str = "limit-without-order";

pub struct LimitWithoutOrder;

impl Rule for LimitWithoutOrder {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, _report: &ProfileReport) -> bool {
        true
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        if !cx.statement.kind.is_query() {
            return Vec::new();
        }
        let Some(at) = cx.map.limit_without_order() else { return Vec::new() };
        let limit = cx.nodes().find(|(_, n)| n.op == crate::profile::node::OpKind::Limit);
        let mut f = Finding::new(ID, Severity::Info).at(Some(cx.to_document(at)));
        if let Some((id, _)) = limit {
            f = f.node(id);
        }
        vec![f]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/limit_without_order.rs"]
mod tests;
