use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, NodeId, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

const ACT: ProfileDepth = ProfileDepth::Actual { timing: true };
const TEXT: &str = "SELECT * FROM customers c JOIN orders o ON o.customer_id = c.id";

/// A nested loop: customers outside, `inner` inside, run `loops` times.
fn nested(loops: f64, inner: OpKind, inner_ms: f64) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ACT)
        .child(OpKind::NestedLoop, "Nested Loop", |n| {
            ran(n, 0.0, 100.0, loops, 1.0);
            cond(n, CondKind::JoinFilter, "(o.customer_id = c.id)");
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 2.0, loops, 1.0);
            n.estimates.rows = Some(loops);
            on(n, "customers", Some("c"));
        })
        .node(inner, if inner == OpKind::SeqScan { "Seq Scan" } else { "Index Scan" }, |n| {
            ran(n, 0.0, inner_ms / loops, 1.0, loops);
            on(n, "orders", Some("o"));
        })
        .with(|p| p.statement.execution_ms = Some(100.0))
        .build()
}

fn orders(indexes: &[(&str, &[&str])]) -> crate::profile::rules::TableFacts {
    table("orders", &["id:integer", "customer_id:integer"], indexes)
}

#[test]
fn a_full_scan_inside_thousands_of_loops() {
    let check = Check::postgres(TEXT).table(orders(&[("orders_pkey", &["id"])]));
    let found = check.rule(ID, nested(5_000.0, OpKind::SeqScan, 90.0));
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.severity, Severity::Critical);
    assert_eq!(f.nodes, [NodeId(0), NodeId(2)]);
    assert_eq!(f.quantity(E::Loops).unwrap().value, 5_000.0);
    assert_eq!(f.text_of(E::Column), Some("customer_id"));
    assert_eq!(f.text_of(E::Target), Some("c.id"));
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\");")
    );
    assert_eq!(&TEXT[f.sql_range.clone().unwrap()], "o.customer_id = c.id");
}

#[test]
fn a_loop_of_one_iteration_is_a_lookup() {
    let check = Check::postgres(TEXT).table(orders(&[]));
    assert!(check.rule(ID, nested(1.0, OpKind::SeqScan, 90.0)).is_empty());
    // At the limit: 1 000 loops; one less isn't.
    assert_eq!(check.rule(ID, nested(1_000.0, OpKind::SeqScan, 10.0)).len(), 1);
    assert!(check.rule(ID, nested(999.0, OpKind::SeqScan, 90.0)).is_empty());
}

#[test]
fn an_index_scan_inside_counts_only_when_it_weighs() {
    let check = Check::postgres(TEXT).table(orders(&[("orders_customer", &["customer_id"])]));
    // Cheap lookups: fine.
    assert!(check.rule(ID, nested(5_000.0, OpKind::IndexScan, 5.0)).is_empty());
    // A fifth of the time: said, as a warning, nothing to index.
    let found = check.rule(ID, nested(5_000.0, OpKind::IndexScan, 25.0));
    assert_eq!(found[0].severity, Severity::Warning);
    assert!(found[0].suggestion.is_none());
}

#[test]
fn an_existing_index_isnt_suggested_again() {
    let check = Check::postgres(TEXT).table(orders(&[("orders_customer", &["customer_id", "id"])]));
    let found = check.rule(ID, nested(5_000.0, OpKind::SeqScan, 90.0));
    assert!(found[0].suggestion.is_none(), "an index already starts with it");
}

#[test]
fn the_outer_sides_estimate_is_the_cause() {
    let mut p = nested(5_000.0, OpKind::SeqScan, 90.0);
    p.nodes[1].estimates.rows = Some(5.0);
    let found = Check::postgres(TEXT).rule(ID, p);
    assert_eq!(found[0].variant, Some("outer_misestimate"));
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("ANALYZE \"customers\";")
    );
}
