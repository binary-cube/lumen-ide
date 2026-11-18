use super::*;
use crate::TableRef;
use crate::profile::ProfileDepth;
use crate::profile::TableStats;
use crate::profile::node::OpKind;
use crate::profile::rules::{Suggestion, TableFacts};
use crate::profile::testing::{Check, ProfileBuilder, on, ran};

fn with_stats(live: u64, dead: u64) -> TableFacts {
    TableFacts {
        stats: Some(TableStats {
            live_rows: Some(live),
            dead_rows: Some(dead),
            ..TableStats::new(TableRef::new("orders"))
        }),
        ..TableFacts::new(TableRef::new("orders"))
    }
}

fn scan() -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 10.0, 100.0, 1.0);
            on(n, "orders", None);
        })
        .build()
}

#[test]
fn many_dead_rows_want_a_vacuum() {
    let found = Check::postgres("SELECT 1").table(with_stats(10_000, 3_000)).rule(ID, scan());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Info);
    assert_eq!(found[0].nodes, [crate::profile::NodeId(0)]);
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("VACUUM (ANALYZE) \"orders\";")
    );
    let found = Check::postgres("SELECT 1").table(with_stats(10_000, 6_000)).rule(ID, scan());
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn few_dead_rows_or_a_small_table_or_nothing_known() {
    assert!(
        Check::postgres("SELECT 1").table(with_stats(10_000, 1_999)).rule(ID, scan()).is_empty()
    );
    assert_eq!(
        Check::postgres("SELECT 1").table(with_stats(10_000, 2_000)).rule(ID, scan()).len(),
        1
    );
    assert!(
        Check::postgres("SELECT 1").table(with_stats(100, 300)).rule(ID, scan()).is_empty(),
        "small"
    );
    assert!(Check::postgres("SELECT 1").rule(ID, scan()).is_empty());
}
