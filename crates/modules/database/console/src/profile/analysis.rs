//! A profile's analysis in the console (plan 39h3): what the rules need gathered on the
//! console's connection, in the background, right after the profile ([`gather`]: each of the
//! profile's tables' structure — at most [`rules::MAX_TABLES`] — and, the auxiliary queries
//! allowed and the driver not limited, the server's statistics of them), the rules run
//! ([`analyze`]), and their findings said in the locales' words ([`texts`]): the domain gives
//! only ids and figures.

use std::sync::Arc;

use database::Session;
use database::console::PlannedStatement;
use database::profile::rules::{
    self, Evidence, Facts, Finding, RecentRuns, RuleContext, Severity, TableFacts, Thresholds,
};
use database::profile::sql_map::SqlMap;
use database::profile::{Profile, ProfileReport, Quantity, Unit};
use database::relations::RelationNotes;
use database::sql::Flavor;
use database::{Cancel, TableRef};

/// What the analysis knows of the source beyond the session: the relations the user decided
/// (its accepted virtual keys) and whether it's marked as production (an index then
/// suggested `CONCURRENTLY`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Analysis {
    pub notes: RelationNotes,
    pub production: bool,
}

/// The facts of `profile`'s tables, read on `session` (`statement` the text profiled):
/// their structures, and their statistics when `auxiliary` queries are allowed and the
/// driver has them. What fails is left unknown (a rule needing it says nothing).
pub fn gather(
    session: &mut dyn Session,
    profile: &Profile,
    statement: &str,
    flavor: Flavor,
    auxiliary: bool,
    notes: RelationNotes,
    cancel: &Cancel,
) -> Facts {
    let map = SqlMap::new(statement, flavor);
    let tables: Vec<TableRef> = rules::tables_of(profile, &map);
    let caps = session.console();
    let limited = caps.is_some_and(|c| c.limited);
    let stats_allowed = auxiliary && !limited && caps.is_some_and(|c| c.profile.table_stats);
    let stats = if stats_allowed && !tables.is_empty() && !cancel.is_cancelled() {
        session.table_stats(&tables, cancel).unwrap_or_else(|e| {
            tracing::debug!("a profile's tables' statistics weren't read: {e}");
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let mut out = Facts { tables: Vec::with_capacity(tables.len()), notes };
    for table in tables {
        if cancel.is_cancelled() {
            break;
        }
        let structure = match session.structure(&table) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::debug!("a profiled table's structure wasn't read: {e}");
                None
            }
        };
        let stats = stats.iter().find(|s| rules::same_table(&s.table, &table)).cloned();
        let table = structure.as_ref().map_or(table, |s| s.table.clone());
        out.tables.push(TableFacts { structure, stats, ..TableFacts::new(table) });
    }
    out
}

/// What the rules find in `report`, sorted (the hot spot first).
pub fn analyze(
    report: &ProfileReport,
    facts: &Facts,
    statement: &PlannedStatement,
    flavor: Flavor,
    recent: &RecentRuns,
    production: bool,
) -> Vec<Finding> {
    let thresholds = Thresholds::DEFAULT;
    let cx = RuleContext::new(report, facts, statement, flavor, recent, &thresholds, production);
    rules::analyze(&cx)
}

/// `report` with `findings` (what the views share).
pub fn with_findings(mut report: ProfileReport, findings: Vec<Finding>) -> Arc<ProfileReport> {
    report.findings = findings;
    Arc::new(report)
}

/// A finding in words: its title, why it matters (with its figures), what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingText {
    pub title: String,
    pub why: String,
    pub fix: String,
}

/// `finding`'s texts in the current language, its evidence their arguments.
pub fn texts(finding: &Finding) -> FindingText {
    let args: Vec<(&str, i18n::Value)> = finding
        .evidence
        .iter()
        .map(|(key, e)| {
            let value = match e {
                Evidence::Text(t) => t.clone(),
                Evidence::Quantity(q) => quantity(*q),
            };
            (key.id(), i18n::Value::from(value))
        })
        .collect();
    FindingText {
        title: i18n::tf(&finding.text_key("title"), &args),
        why: i18n::tf(&finding.text_key("why"), &args),
        fix: i18n::tf(&finding.text_key("fix"), &args),
    }
}

/// A severity's word.
pub fn severity(severity: Severity) -> String {
    i18n::t(&format!("profile.severity.{}", severity.id()))
}

/// The Output's part of the summary: `3 problems (1 critical)`, or none found.
pub fn problems_text(findings: &[Finding]) -> String {
    let (problems, critical) = rules::problems(findings);
    if critical > 0 {
        i18n::tr!(
            "console.profile_problems_critical",
            count = problems as i64,
            critical = critical as i64
        )
    } else {
        i18n::tr!("console.profile_problems", count = problems as i64)
    }
}

/// A figure as the texts say it: rows and counts grouped by thousands, times, bytes,
/// shares, factors (`×120 ↑`, `×0.01 ↓`).
pub fn quantity(q: Quantity) -> String {
    let v = q.value;
    match q.unit {
        Unit::Rows | Unit::Count | Unit::Blocks | Unit::Pages => grouped(v.round() as i64),
        Unit::Millis => millis(v),
        Unit::Bytes => i18n::bytes(v.max(0.0) as u64),
        Unit::Percent => {
            let text =
                if v < 10.0 && v.fract() != 0.0 { format!("{v:.1}%") } else { format!("{v:.0}%") };
            decimal(text)
        }
        Unit::Ratio => {
            if v >= 1.0 {
                format!("×{} ↑", grouped(v.round() as i64))
            } else if v > 0.0 {
                let text = if v >= 0.01 { format!("×{v:.2}") } else { format!("×{v:.4}") };
                format!("{} ↓", decimal(text))
            } else {
                "×0".to_owned()
            }
        }
        Unit::Cost => grouped(v.round() as i64),
        Unit::None => i18n::Locale::from_tag(&i18n::locale()).format_number(v),
    }
}

/// A time: `0.042 ms`, `845.2 ms`, `2.48 s`.
fn millis(ms: f64) -> String {
    let text = if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms >= 1.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.3} ms")
    };
    decimal(text)
}

/// The decimal point in the current language.
fn decimal(text: String) -> String {
    if i18n::locale() == "ro" { text.replace('.', ",") } else { text }
}

/// `1204332` → `1,204,332` (`1 204 332` in Romanian).
pub fn grouped(n: i64) -> String {
    let separator = if i18n::locale() == "ro" { '\u{202f}' } else { ',' };
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/profile/analysis.rs"]
mod tests;
