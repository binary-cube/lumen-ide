//! `stale-stats` — the planner's statistics of a table are missing or old: its estimates (and
//! so its choices) rest on guesses.
//!
//! - **Evidence:** the table's statistics as read (39h6's `table_stats`; SQLite's
//!   `sqlite_stat1`): never analyzed (no `ANALYZE` nor autoanalyze, PostgreSQL), or rows
//!   changed since the last one at least [`Thresholds::modified_share`](super::Thresholds)
//!   (20 %) of the live ones; on SQLite, a statement joining tables without `sqlite_stat1`
//!   (one table alone is planned well without it). Without them nothing is said.
//! - **Severity:** `Warning` for never analyzed; `Info` otherwise (variants `changed`,
//!   `sqlite`).
//! - **Suggestion:** `ANALYZE` of the table.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "stale-stats";

pub struct StaleStats;

impl Rule for StaleStats {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, _report: &ProfileReport) -> bool {
        true
    }

    fn variants(&self) -> &'static [&'static str] {
        &["changed", "sqlite"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let mut out = Vec::new();
        let joins = cx.map.tables().len() > 1
            || cx.profile().nodes.iter().filter(|n| n.op.is_scan()).count() > 1;
        for facts in &cx.facts.tables {
            let Some(stats) = &facts.stats else { continue };
            let table = &facts.table;
            let live = stats.live_rows.map(|l| l as f64);
            let found = if cx.is_sqlite() {
                (stats.stats_present == Some(false) && joins)
                    .then(|| Finding::new(ID, Severity::Info).variant("sqlite"))
            } else if stats.last_analyze.is_none()
                && stats.last_autoanalyze.is_none()
                && stats.stats_present != Some(true)
                && (stats.live_rows.is_some() || stats.modified_since_analyze.is_some())
            {
                Some(Finding::new(ID, Severity::Warning))
            } else if let (Some(modified), Some(live)) = (stats.modified_since_analyze, live)
                && modified as f64 >= live.max(1.0) * t.modified_share
                && live + modified as f64 >= t.small_table_rows
            {
                Some(
                    Finding::new(ID, Severity::Info)
                        .variant("changed")
                        .with(E::Modified, modified as f64, Unit::Rows)
                        .with(
                            E::ModifiedShare,
                            modified as f64 / live.max(1.0) * 100.0,
                            Unit::Percent,
                        ),
                )
            } else {
                None
            };
            let Some(mut f) = found else { continue };
            f = f.text(E::Table, table.to_string()).suggest(cx.analyze_sql(table));
            if let Some(l) = live {
                f = f.with(E::Live, l, Unit::Rows);
            }
            if let Some(last) = stats.last_analyze.as_ref().or(stats.last_autoanalyze.as_ref()) {
                f = f.text(E::Value, last.clone());
            }
            let mut impact: f64 = 0.0;
            for (id, n) in cx.nodes() {
                if n.op.is_scan() && cx.table_of(id).is_some_and(|x| super::same_table(&x, table)) {
                    impact = impact.max(cx.inclusive_share(id));
                    if f.nodes.is_empty() {
                        f = f.at(cx.node_range(id));
                    }
                    f = f.node(id);
                }
            }
            out.push(f.impact(impact));
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/stale_stats.rs"]
mod tests;
