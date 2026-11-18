use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, NodeId, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

const ACT: ProfileDepth = ProfileDepth::Actual { timing: true };

/// A hash join over a scan of `orders` expecting `est` rows and getting `real`.
fn joined(est: f64, real: f64, filter: &str) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ACT)
        .child(OpKind::HashJoin, "Hash Join", |n| {
            ran(n, 0.0, 20.0, real, 1.0);
            n.estimates.rows = Some(est);
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 8.0, real, 1.0);
            n.estimates.rows = Some(est);
            on(n, "orders", None);
            if !filter.is_empty() {
                cond(n, CondKind::Filter, filter);
            }
        })
        .child(OpKind::Hash, "Hash", |n| ran(n, 0.0, 2.0, 10.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 1.0, 10.0, 1.0);
            n.estimates.rows = Some(10.0);
            on(n, "customers", None);
        })
        .with(|p| p.statement.execution_ms = Some(20.0))
        .build()
}

#[test]
fn where_it_starts_not_its_consequences() {
    let found = Check::postgres("SELECT 1").rule(ID, joined(10.0, 5_000.0, "(status = 'x'::text)"));
    assert_eq!(found.len(), 1, "the join only follows its scan: {found:?}");
    let f = &found[0];
    assert_eq!(f.nodes, [NodeId(1), NodeId(0)]);
    assert_eq!(f.variant, Some("join"), "it feeds a join");
    assert_eq!(f.quantity(E::Factor).unwrap().value, 500.0);
    assert!((f.impact - 1.0).abs() < 1e-9, "the join's share");
    assert_eq!(f.suggestion.as_ref().and_then(Suggestion::sql), Some("ANALYZE \"orders\";"));
}

#[test]
fn two_columns_of_a_table_want_extended_statistics() {
    let check = Check::postgres("SELECT 1");
    let found = check.rule(ID, joined(10.0, 5_000.0, "((city = 'x'::text) AND (zip = '1'::text))"));
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some(
            "CREATE STATISTICS \"stx_orders_city_zip\" (ndistinct, dependencies, mcv) ON \"city\", \"zip\" FROM \"orders\";\nANALYZE \"orders\";"
        )
    );
    // Before 12: no `mcv`; before 10: no extended statistics at all.
    let mut old = Check::postgres("SELECT 1");
    old.flavor = crate::sql::Flavor::Postgres { version: Some(110_000) };
    let sql = old.rule(ID, joined(10.0, 5_000.0, "((city = 'x'::text) AND (zip = '1'::text))"))[0]
        .suggestion
        .clone()
        .unwrap();
    assert!(sql.sql().unwrap().contains("(ndistinct, dependencies) ON"));
    old.flavor = crate::sql::Flavor::Postgres { version: Some(96_000) };
    let sql = old.rule(ID, joined(10.0, 5_000.0, "((city = 'x'::text) AND (zip = '1'::text))"))[0]
        .suggestion
        .clone()
        .unwrap();
    assert_eq!(sql.sql(), Some("ANALYZE \"orders\";"));
}

#[test]
fn a_wrong_guess_on_three_rows_is_nothing() {
    // ×100 off, but 3 rows against 0.03: nothing chosen differently.
    assert!(Check::postgres("SELECT 1").rule(ID, joined(1.0, 3.0, "")).is_empty());
    // At the limits: ×10 and 100 rows.
    assert_eq!(Check::postgres("SELECT 1").rule(ID, joined(10.0, 100.0, "")).len(), 1);
    assert!(Check::postgres("SELECT 1").rule(ID, joined(11.0, 100.0, "")).is_empty(), "×9.1");
    assert!(Check::postgres("SELECT 1").rule(ID, joined(9.0, 90.0, "")).is_empty(), "under 100");
}

#[test]
fn fewer_rows_under_a_limit_are_the_limits() {
    let p = ProfileBuilder::new("postgres", ACT)
        .child(OpKind::Limit, "Limit", |n| {
            ran(n, 0.0, 1.0, 10.0, 1.0);
            n.estimates.rows = Some(10.0);
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 1.0, 10.0, 1.0);
            n.estimates.rows = Some(100_000.0);
            on(n, "orders", None);
        })
        .build();
    assert!(Check::postgres("SELECT 1").rule(ID, p).is_empty());
    // An estimated plan has nothing to compare.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "Seq Scan", |n| n.estimates.rows = Some(10.0))
        .build();
    assert!(Check::postgres("SELECT 1").rule(ID, p).is_empty());
}

#[test]
fn old_statistics_are_evidence() {
    let mut facts = table("orders", &["status"], &[]);
    facts.stats = Some(crate::profile::TableStats {
        last_autoanalyze: Some("2026-01-02 10:00:00+00".into()),
        modified_since_analyze: Some(40_000),
        ..crate::profile::TableStats::new(crate::TableRef::new("orders"))
    });
    let found = Check::postgres("SELECT 1").table(facts).rule(ID, joined(10.0, 5_000.0, ""));
    assert_eq!(found[0].text_of(E::Value), Some("2026-01-02 10:00:00+00"));
    assert_eq!(found[0].quantity(E::Modified).unwrap().value, 40_000.0);
}
