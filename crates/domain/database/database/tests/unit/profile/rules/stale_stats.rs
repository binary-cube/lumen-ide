use super::*;
use crate::TableRef;
use crate::profile::ProfileDepth;
use crate::profile::TableStats;
use crate::profile::node::OpKind;
use crate::profile::rules::{Suggestion, TableFacts};
use crate::profile::testing::{Check, ProfileBuilder, on};

fn facts(edit: impl FnOnce(&mut TableStats)) -> TableFacts {
    let mut stats = TableStats::new(TableRef::new("orders"));
    edit(&mut stats);
    TableFacts { stats: Some(stats), ..TableFacts::new(TableRef::new("orders")) }
}

fn scans(driver: &str, tables: &[&str]) -> crate::profile::Profile {
    let mut b = ProfileBuilder::new(driver, ProfileDepth::Estimated);
    for t in tables {
        b = b.node(OpKind::SeqScan, &format!("SCAN {t}"), |n| on(n, t, None));
    }
    b.build()
}

#[test]
fn never_analyzed() {
    let check =
        Check::postgres("SELECT * FROM orders").table(facts(|s| s.live_rows = Some(50_000)));
    let found = check.rule(ID, scans("postgres", &["orders"]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].suggestion.as_ref().and_then(Suggestion::sql), Some("ANALYZE \"orders\";"));
}

#[test]
fn changed_much_since_the_last_analyze() {
    let check = Check::postgres("SELECT * FROM orders").table(facts(|s| {
        s.live_rows = Some(10_000);
        s.modified_since_analyze = Some(2_000);
        s.last_autoanalyze = Some("2026-10-01".into());
    }));
    let found = check.rule(ID, scans("postgres", &["orders"]));
    assert_eq!(found[0].variant, Some("changed"));
    assert_eq!(found[0].severity, Severity::Info);
    let check = Check::postgres("SELECT * FROM orders").table(facts(|s| {
        s.live_rows = Some(10_000);
        s.modified_since_analyze = Some(1_999);
        s.last_analyze = Some("2026-10-01".into());
    }));
    assert!(check.rule(ID, scans("postgres", &["orders"])).is_empty());
}

#[test]
fn sqlite_without_stat1_only_matters_for_joins() {
    let text = "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id";
    let check = Check::sqlite(text).table(facts(|s| s.stats_present = Some(false)));
    let found = check.rule(ID, scans("sqlite", &["orders", "customers"]));
    assert_eq!(found[0].variant, Some("sqlite"));
    let check =
        Check::sqlite("SELECT * FROM orders").table(facts(|s| s.stats_present = Some(false)));
    assert!(check.rule(ID, scans("sqlite", &["orders"])).is_empty(), "one table alone");
    let check = Check::sqlite(text).table(facts(|s| s.stats_present = Some(true)));
    assert!(check.rule(ID, scans("sqlite", &["orders", "customers"])).is_empty());
    // Nothing read: nothing said.
    assert!(Check::sqlite(text).rule(ID, scans("sqlite", &["orders", "customers"])).is_empty());
}
