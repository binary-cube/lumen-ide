//! `big-offset` — a large `OFFSET`: the engine reads and throws away every row before the
//! page, so each next page is slower.
//!
//! - **Evidence:** the statement's top level's `OFFSET` (or MySQL's and SQLite's
//!   `LIMIT offset, count`) of a number at least [`Thresholds::big_offset`](super::Thresholds)
//!   (10 000).
//! - **Severity:** `Info`.
//! - **Suggestion:** keyset pagination (`WHERE key > :last ORDER BY key`).

use super::{EvidenceKey as E, Finding, RewriteHint, Rule, RuleContext, Severity, Suggestion};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::OpKind;

pub(super) const ID: &str = "big-offset";

pub struct BigOffset;

impl Rule for BigOffset {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, _report: &ProfileReport) -> bool {
        true
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let Some((offset, at)) = cx.map.offset() else { return Vec::new() };
        if offset < cx.thresholds.big_offset {
            return Vec::new();
        }
        let limit = cx.nodes().find(|(_, n)| n.op == OpKind::Limit).map(|(id, _)| id);
        let mut f = Finding::new(ID, Severity::Info)
            .with(E::Offset, offset as f64, Unit::Rows)
            .at(Some(cx.to_document(at)))
            .suggest(Some(Suggestion::Rewrite { hint: RewriteHint::KeysetPagination }));
        if let Some(id) = limit {
            f = f.node(id).impact(cx.inclusive_share(id));
        }
        vec![f]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/big_offset.rs"]
mod tests;
