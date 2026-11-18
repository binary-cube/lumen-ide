//! `no-statement-stats` — the server's history of the query was asked for and the server
//! keeps none: what the same query costs over time, its calls (and so the server's side of
//! `n-plus-one`) stay unknown.
//!
//! - **Evidence:** the driver's note ([`ProfileNote::NoStatementStats`]): PostgreSQL without
//!   the `pg_stat_statements` extension installed (or not loaded, or not readable). Nothing is
//!   ever installed by us: the text says what installing it means.
//! - **Severity:** `Info`.
//! - **Suggestion:** none to write (`shared_preload_libraries`, a restart and `CREATE
//!   EXTENSION` are the server's administrator's to decide).

use super::{Finding, Rule, RuleContext, Severity};
use crate::profile::ProfileNote;
use crate::profile::derive::ProfileReport;

pub(super) const ID: &str = "no-statement-stats";

pub struct NoStatementStats;

impl Rule for NoStatementStats {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.has_note(ProfileNote::NoStatementStats)
    }

    fn check(&self, _cx: &RuleContext<'_>) -> Vec<Finding> {
        vec![Finding::new(ID, Severity::Info)]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/no_statement_stats.rs"]
mod tests;
