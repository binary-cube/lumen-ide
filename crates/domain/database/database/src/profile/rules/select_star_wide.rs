//! `select-star-wide` — `SELECT *` of wide rows: every column is read, sent and kept, and no
//! index can answer the query alone (an `Index Only Scan` needs only the columns it holds).
//!
//! - **Evidence:** `*` (or `t.*`) in the top level's select list, and a row of at least
//!   [`Thresholds::wide_row_bytes`](super::Thresholds) (500, the planner's width) or more than
//!   `wide_columns` (20) columns in the tables read (their structure known). A narrow table's
//!   `*` costs nothing.
//! - **Severity:** `Info`.
//! - **Suggestion:** listing the columns used (`ListColumns`).

use super::{EvidenceKey as E, Finding, RewriteHint, Rule, RuleContext, Severity, Suggestion};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "select-star-wide";

pub struct SelectStarWide;

impl Rule for SelectStarWide {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, _report: &ProfileReport) -> bool {
        true
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let Some(star) = cx.map.select_star() else { return Vec::new() };
        let width = cx
            .profile()
            .roots
            .iter()
            .filter_map(|r| cx.profile().get(*r)?.estimates.width)
            .fold(None, |a: Option<f64>, w| Some(a.map_or(w, |a| a.max(w))));
        let columns: usize = cx
            .map
            .tables()
            .iter()
            .filter(|o| o.depth == 0)
            .filter_map(|o| cx.facts.table(&o.table)?.structure.as_ref())
            .map(|s| s.columns.len())
            .sum();
        let wide = width.is_some_and(|w| w >= t.wide_row_bytes) || columns > t.wide_columns;
        if !wide {
            return Vec::new();
        }
        let mut f = Finding::new(ID, Severity::Info)
            .impact(0.0)
            .at(Some(cx.to_document(star)))
            .suggest(Some(Suggestion::Rewrite { hint: RewriteHint::ListColumns }));
        if let Some(w) = width {
            f = f.with(E::Width, w, Unit::Bytes);
        }
        if columns > 0 {
            f = f.with(E::ColumnCount, columns as f64, Unit::Count);
        }
        if let Some(root) = cx.profile().roots.first() {
            f = f.node(*root);
        }
        vec![f]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/select_star_wide.rs"]
mod tests;
