//! `n-plus-one` — the same query run over and over with other values (one per row of
//! something else): many round trips where one query would do.
//!
//! - **Evidence:** the statement's fingerprint ([`crate::sql::fingerprint`]) run at least
//!   [`Thresholds::repeated_runs`](super::Thresholds) (20) times in the console's session
//!   within `repeated_window` (60 s); or the server's history of it (`pg_stat_statements`):
//!   at least `repeated_calls` (1 000) calls of at most `repeated_mean_ms` (1 ms) each.
//! - **Severity:** `Info` (variant `server` when it's the server's history).
//! - **Suggestion:** one query for all the values (`IN (…)`, a join: `Batch`).

use super::{EvidenceKey as E, Finding, RewriteHint, Rule, RuleContext, Severity, Suggestion};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::sql::fingerprint;

pub(super) const ID: &str = "n-plus-one";

pub struct NPlusOne;

impl Rule for NPlusOne {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, _report: &ProfileReport) -> bool {
        true
    }

    fn variants(&self) -> &'static [&'static str] {
        &["server"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let rewrite = Some(Suggestion::Rewrite { hint: RewriteHint::Batch });
        let whole = cx.to_document(0..cx.statement.sent().len());
        let print = fingerprint(cx.statement.sent(), cx.flavor);
        let runs = cx.recent.count_within(print.hash, t.repeated_window);
        if runs >= t.repeated_runs {
            return vec![
                Finding::new(ID, Severity::Info)
                    .with(E::Runs, runs as f64, Unit::Count)
                    .with(E::Window, t.repeated_window.as_secs_f64() * 1000.0, Unit::Millis)
                    .at(Some(whole))
                    .suggest(rewrite),
            ];
        }
        if let Some(h) = cx.profile().statement.history
            && let (Some(calls), Some(mean)) = (h.calls, h.mean_ms)
            && calls >= t.repeated_calls
            && mean <= t.repeated_mean_ms
        {
            return vec![
                Finding::new(ID, Severity::Info)
                    .variant("server")
                    .with(E::Calls, calls as f64, Unit::Count)
                    .with(E::Mean, mean, Unit::Millis)
                    .at(Some(whole))
                    .suggest(rewrite),
            ];
        }
        Vec::new()
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/n_plus_one.rs"]
mod tests;
