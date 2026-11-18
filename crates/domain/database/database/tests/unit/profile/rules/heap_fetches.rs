use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, on, ran};

fn index_only(rows: f64, loops: f64, fetches: f64) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::IndexOnlyScan, "Index Only Scan", |n| {
            ran(n, 0.0, 0.05, rows, loops);
            n.actuals.heap_fetches = Some(fetches);
            on(n, "orders", Some("o"));
        })
        .build()
}

#[test]
fn an_index_only_scan_going_to_the_table() {
    // The nested-loop-subplan fixture's: 1 200 fetches of 10 rows × 1 000 loops.
    let found = Check::postgres("SELECT 1").rule(ID, index_only(10.0, 1_000.0, 1_200.0));
    assert_eq!(found.len(), 1);
    assert!((found[0].quantity(E::Share).unwrap().value - 12.0).abs() < 1e-9);
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("VACUUM (ANALYZE) \"orders\";")
    );
}

#[test]
fn few_fetches_are_fine() {
    assert!(Check::postgres("SELECT 1").rule(ID, index_only(10.0, 1_000.0, 999.0)).is_empty());
    assert!(Check::postgres("SELECT 1").rule(ID, index_only(10_000.0, 1.0, 0.0)).is_empty());
    // SQLite has no VACUUM per table.
    let mut p = index_only(10.0, 100.0, 500.0);
    p.engine.driver = "sqlite".into();
    let found = Check::sqlite("SELECT 1").rule(ID, p);
    assert!(found[0].suggestion.is_none());
}
