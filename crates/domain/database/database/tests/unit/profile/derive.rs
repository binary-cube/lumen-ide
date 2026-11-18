use super::*;
use crate::plan::Measure;
use crate::profile::node::{ObjectRef, Parallel};
use crate::profile::testing::{ProfileBuilder, planned, ran};
use crate::profile::{NodeId, ProfileDepth};

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

#[test]
fn self_time_takes_the_childrens_out_and_loops_count() {
    // Nested Loop 10 ms: its outer scan 2 ms, its inner index scan 0.05 ms × 100 loops.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::NestedLoop, "Nested Loop", |n| ran(n, 0.0, 10.0, 100.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 2.0, 100.0, 1.0);
            n.estimates.rows = Some(1.0);
        })
        .node(OpKind::IndexScan, "Index Scan", |n| {
            ran(n, 0.0, 0.05, 1.0, 100.0);
            n.estimates.rows = Some(1.0);
        })
        .with(|p| p.statement.execution_ms = Some(10.0))
        .build();
    let r = derive(p);
    assert!(close(r.derived(NodeId(2)).inclusive_ms, 5.0), "× its loops");
    assert!(close(r.derived(NodeId(0)).self_ms, 3.0), "10 − 2 − 5");
    assert!(close(r.derived(NodeId(0)).self_pct, 0.3));
    assert!(close(r.derived(NodeId(2)).total_rows, 100.0));
    assert!(close(r.derived(NodeId(1)).misestimate, 100.0), "100 real rows of 1 expected");
    assert!(close(r.derived(NodeId(1)).share_of_parent, 0.2));
    assert_eq!(r.totals.measure, Some(Measure::Time));
    assert_eq!(r.hot_path, [NodeId(0), NodeId(2)], "the inner scan weighs more in all its loops");
    assert_eq!(r.totals.worst_misestimate, Some(100.0));
    assert!(!r.derived(NodeId(0)).approx);
}

#[test]
fn workers_times_are_averages_under_a_gather() {
    // Gather 30 ms, 2 workers + the leader: the parallel scan's 25 ms × 3 loops is 25 ms of
    // wall time, not 75.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Gather, "Gather", |n| {
            ran(n, 1.0, 30.0, 3000.0, 1.0);
            n.parallel =
                Some(Parallel { planned: Some(2), launched: Some(2), ..Parallel::default() });
        })
        .child(OpKind::Aggregate(crate::profile::AggStrategy::Plain), "Partial Aggregate", |n| {
            ran(n, 0.0, 27.0, 1.0, 3.0);
        })
        .node(OpKind::SeqScan, "Parallel Seq Scan", |n| ran(n, 0.0, 25.0, 1000.0, 3.0))
        .build();
    let r = derive(p);
    assert!(close(r.derived(NodeId(2)).inclusive_ms, 25.0));
    assert!(close(r.derived(NodeId(1)).self_ms, 2.0));
    assert!(close(r.derived(NodeId(0)).self_ms, 3.0));
    assert!(r.derived(NodeId(2)).approx && r.derived(NodeId(1)).approx);
    assert!(!r.derived(NodeId(0)).approx, "the gather's own figures are its own");
}

#[test]
fn an_initplan_is_taken_out_once_and_a_cte_is_paid_by_its_first_reader() {
    // Result (5 ms) with an InitPlan (1 ms) — taken out once.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Result, "Result", |n| ran(n, 0.0, 5.0, 1.0, 1.0))
        .node(OpKind::Aggregate(crate::profile::AggStrategy::Plain), "Aggregate", |n| {
            n.role = Some(ChildRole::InitPlan);
            n.subplan_name = Some("InitPlan 1 (returns $0)".into());
            ran(n, 0.0, 1.0, 1.0, 1.0);
        })
        .build();
    let r = derive(p);
    assert!(close(r.derived(NodeId(0)).self_ms, 4.0));

    // A CTE materialized (its plan, 6 ms, an InitPlan of the top node) read by two scans:
    // the first (7 ms) pays for it, the second (0.5 ms) doesn't, the top node not again.
    let cte = |n: &mut crate::profile::ProfileNode, t: f64| {
        n.object = Some(ObjectRef::new(ObjectKind::Cte, "totals"));
        ran(n, 0.0, t, 10.0, 1.0);
    };
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::HashJoin, "Hash Join", |n| ran(n, 0.0, 9.0, 10.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            n.role = Some(ChildRole::InitPlan);
            n.subplan_name = Some("CTE totals".into());
            ran(n, 0.0, 6.0, 10.0, 1.0);
        })
        .node(OpKind::CteScan, "CTE Scan", |n| cte(n, 7.0))
        .child(OpKind::Hash, "Hash", |n| ran(n, 0.0, 0.6, 10.0, 1.0))
        .node(OpKind::CteScan, "CTE Scan", |n| cte(n, 0.5))
        .build();
    let r = derive(p);
    assert!(close(r.derived(NodeId(0)).self_ms, 9.0 - 7.0 - 0.6), "the CTE's plan not taken twice");
    assert!(close(r.derived(NodeId(2)).self_ms, 1.0), "the first reader: 7 − the CTE's 6");
    assert!(close(r.derived(NodeId(4)).self_ms, 0.5));
}

#[test]
fn never_executed_and_rounding_past_the_parent() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Append, "Append", |n| ran(n, 0.0, 1.0, 5.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 0.0, 0.0, 0.0);
            n.actuals.never_executed = true;
            n.estimates.rows = Some(100.0);
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| ran(n, 0.0, 1.002, 5.0, 1.0))
        .build();
    let r = derive(p);
    assert!(close(r.derived(NodeId(1)).inclusive_ms, 0.0));
    assert_eq!(r.derived(NodeId(1)).misestimate, None, "never ran: nothing to compare");
    // The children took longer than the parent (the engine's rounding): none below zero.
    assert!(close(r.derived(NodeId(0)).self_ms, 0.0));
    assert!(r.derived(NodeId(0)).approx);
}

#[test]
fn an_estimated_plan_is_weighed_by_cost_and_a_bare_one_by_nothing() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .child(OpKind::Sort, "Sort", |n| planned(n, 100.0, 120.0, 10.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| planned(n, 0.0, 90.0, 1000.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| planned(n, 0.0, 10.0, 10.0))
        .build();
    let r = derive(p);
    assert_eq!(r.totals.measure, Some(Measure::Cost));
    assert!(close(r.derived(NodeId(0)).self_cost, 20.0));
    assert!(close(r.derived(NodeId(1)).cost_pct, 0.75));
    assert_eq!(r.derived(NodeId(0)).self_ms, None, "no time on an estimated plan");
    assert_eq!(r.hot_path, [NodeId(0), NodeId(1)]);
    assert_eq!(r.totals.cost, Some(120.0));

    let bare = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN users", |_| {})
        .build();
    let r = derive(bare);
    assert_eq!(r.totals.measure, None);
    assert!(r.hot_path.is_empty());
    assert_eq!(r.order, [NodeId(0)]);
}

#[test]
fn the_columns_shown_are_those_a_node_has() {
    use crate::profile::metric::{Metric, present};
    let estimated = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Estimated)
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                planned(n, 0.0, 10.0, 5.0);
                n.estimates.width = Some(8.0);
            })
            .build(),
    );
    assert_eq!(present(&estimated), [Metric::EstRows, Metric::Cost, Metric::Width]);
    let analyzed = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                ran(n, 0.1, 1.0, 5.0, 1.0);
                n.io = Some(crate::profile::Io { shared_hit: Some(1), ..Default::default() });
                n.actuals.rows_removed_by_filter = Some(3.0);
            })
            .build(),
    );
    assert_eq!(
        present(&analyzed),
        [
            Metric::SelfTime,
            Metric::TotalTime,
            Metric::StartupTime,
            Metric::ActualRows,
            Metric::Loops,
            Metric::SharedHit,
            Metric::RemovedByFilter
        ]
    );
    let node = &analyzed.profile.nodes[0];
    let q = Metric::SharedHit.of(node, analyzed.derived(NodeId(0))).unwrap();
    assert_eq!((q.value, q.unit), (1.0, crate::profile::Unit::Blocks));
}

/// The budget: `derive` on 10 000 nodes (a deep and wide tree) well under 20 ms in a release
/// build (generous in debug).
#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn derive_keeps_within_its_budget() {
    let mut b = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true }).child(
        OpKind::Append,
        "Append",
        |n| ran(n, 0.0, 100.0, 10_000.0, 1.0),
    );
    for i in 0..9_999 {
        b = b.node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 0.01, 1.0, 1.0);
            n.object = Some(ObjectRef::new(ObjectKind::Table, format!("part_{i}")));
        });
    }
    let p = b.build();
    let started = std::time::Instant::now();
    let r = derive(p);
    let took = started.elapsed();
    assert_eq!(r.order.len(), 10_000);
    let budget = if cfg!(debug_assertions) { 400 } else { 20 };
    assert!(took.as_millis() < budget, "{took:?}");
}
