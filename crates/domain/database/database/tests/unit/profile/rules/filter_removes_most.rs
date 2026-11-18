use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

const TEXT: &str = "SELECT * FROM orders WHERE customer_id = 7 AND status = 'paid'";

fn index_scan(kept: f64, removed: f64, loops: f64, filter: &str) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::IndexScan, "Index Scan", |n| {
            ran(n, 0.0, 5.0, kept, loops);
            n.actuals.rows_removed_by_filter = Some(removed);
            n.index = Some("orders_customer".into());
            on(n, "orders", None);
            cond(n, CondKind::IndexCond, "(customer_id = 7)");
            cond(n, CondKind::Filter, filter);
        })
        .with(|p| p.statement.execution_ms = Some(5.0))
        .build()
}

fn orders() -> crate::profile::rules::TableFacts {
    table(
        "orders",
        &["id:integer", "customer_id:integer", "status:varchar(10)", "total:numeric"],
        &[("orders_customer", &["customer_id"])],
    )
}

#[test]
fn an_index_scan_whose_filter_throws_most_away() {
    let check = Check::postgres(TEXT).table(orders());
    let found = check.rule(ID, index_scan(10.0, 9_990.0, 1.0, "((status)::text = 'paid'::text)"));
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.quantity(E::Removed).unwrap().value, 9_990.0);
    assert!((f.quantity(E::RemovedShare).unwrap().value - 99.9).abs() < 1e-9);
    // Literals only: a partial index on the index condition's columns.
    let Some(Suggestion::CreateIndex { sql, where_clause, .. }) = &f.suggestion else { panic!() };
    assert_eq!(where_clause.as_deref(), Some("((status)::text = 'paid'::text)"));
    assert_eq!(
        sql,
        "CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\") WHERE ((status)::text = 'paid'::text);"
    );
    assert_eq!(&TEXT[f.sql_range.clone().unwrap()], "status = 'paid'");
}

#[test]
fn a_range_in_the_filter_makes_it_composite() {
    let check = Check::postgres(TEXT).table(orders());
    let found = check.rule(ID, index_scan(10.0, 9_990.0, 1.0, "(total > '100'::numeric)"));
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some(
            "CREATE INDEX \"idx_orders_customer_id_total\" ON \"orders\" (\"customer_id\", \"total\");"
        )
    );
}

#[test]
fn per_loop_figures_count_in_all_loops() {
    // 2 kept and 18 removed per loop × 100 loops: 1 800 removed of 2 000.
    let found =
        Check::postgres(TEXT).table(orders()).rule(ID, index_scan(2.0, 18.0, 100.0, "(total > 1)"));
    assert_eq!(found[0].quantity(E::Removed).unwrap().value, 1_800.0);
}

#[test]
fn at_the_limits_and_below() {
    let check = Check::postgres(TEXT).table(orders());
    // 90 % of 1 000 removed... 900 isn't 1 000.
    assert!(check.rule(ID, index_scan(100.0, 900.0, 1.0, "(total > 1)")).is_empty());
    assert_eq!(check.rule(ID, index_scan(111.0, 1_000.0, 1.0, "(total > 1)")).len(), 1, "90.0 %");
    assert!(check.rule(ID, index_scan(200.0, 1_000.0, 1.0, "(total > 1)")).is_empty(), "83 %");
}

#[test]
fn a_big_selective_seq_scan_is_the_other_rules() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 5.0, 10.0, 1.0);
            n.actuals.rows_removed_by_filter = Some(99_990.0);
            on(n, "orders", None);
            cond(n, CondKind::Filter, "(total > 1)");
        })
        .build();
    let check = Check::postgres(TEXT).table(orders());
    assert!(check.rule(ID, p.clone()).is_empty());
    let all = check.analyze(p);
    assert!(all.iter().any(|f| f.rule == "seq-scan-selective"));
    assert!(!all.iter().any(|f| f.rule == ID), "said once");
}
