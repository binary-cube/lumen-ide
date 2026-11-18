use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::testing::{Check, ProfileBuilder, planned, ran};

#[test]
fn the_node_with_the_most_time_of_its_own() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::NestedLoop, "Nested Loop", |n| ran(n, 0.0, 10.0, 100.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| ran(n, 0.0, 2.0, 100.0, 1.0))
        .node(OpKind::IndexScan, "Index Scan", |n| ran(n, 0.0, 0.05, 1.0, 100.0))
        .with(|p| p.statement.execution_ms = Some(10.0))
        .build();
    let found = Check::postgres("SELECT 1").rule(ID, p);
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.nodes, [crate::profile::NodeId(2)], "5 ms of its own in 100 loops");
    assert!((f.quantity(E::Share).unwrap().value - 50.0).abs() < 1e-6);
    assert!(f.variant.is_none() && !f.is_problem());
}

#[test]
fn an_estimated_plan_by_its_cost() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .child(OpKind::HashJoin, "Hash Join", |n| planned(n, 1.0, 100.0, 10.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| planned(n, 0.0, 70.0, 10.0))
        .node(OpKind::Hash, "Hash", |n| planned(n, 0.0, 10.0, 10.0))
        .build();
    let found = Check::postgres("SELECT 1").rule(ID, p);
    assert_eq!(found[0].variant, Some("cost"));
    assert_eq!(found[0].nodes, [crate::profile::NodeId(1)]);
    assert!(found[0].quantity(E::Cost).is_some());
}

#[test]
fn nothing_weighed_nothing_said() {
    // SQLite's plan: no cost, no time.
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN t", |_| {})
        .build();
    assert!(Check::sqlite("SELECT * FROM t").rule(ID, p).is_empty());
    // Every node at zero.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::Result, "Result", |n| planned(n, 0.0, 0.0, 1.0))
        .build();
    assert!(Check::postgres("SELECT 1").rule(ID, p).is_empty());
}
