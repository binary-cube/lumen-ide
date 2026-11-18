use super::*;
use crate::profile::node::{CondKind, Io, Parallel};
use crate::profile::rules::{Finding, Severity};
use crate::profile::testing::{ProfileBuilder, cond, on, planned, ran};
use crate::profile::{ProfileDepth, ProfileReport, derive};

fn io(hit: u64, read: u64) -> Io {
    Io { shared_hit: Some(hit), shared_read: Some(read), block_size: Some(8192), ..Io::default() }
}

/// A join of orders and customers: orders read whole (`slow`) or through an index.
fn join(slow: bool) -> ProfileReport {
    let b = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true }).child(
        OpKind::HashJoin,
        "Hash Join",
        |n| {
            ran(n, 1.0, if slow { 840.0 } else { 11.0 }, 6000.0, 1.0);
            planned(n, 10.0, if slow { 126_000.0 } else { 900.0 }, 6000.0);
            cond(n, CondKind::HashCond, "(o.customer_id = c.id)");
        },
    );
    let b = if slow {
        b.node(OpKind::SeqScan, "Seq Scan", |n| {
            on(n, "orders", Some("o"));
            ran(n, 0.1, 800.0, 6000.0, 1.0);
            planned(n, 0.0, 125_000.0, 6000.0);
            n.actuals.rows_removed_by_filter = Some(400_000.0);
            n.io = Some(io(12_000, 40_000));
        })
    } else {
        b.node(OpKind::IndexScan, "Index Scan", |n| {
            on(n, "orders", Some("o"));
            n.index = Some("idx_orders_status".into());
            ran(n, 0.1, 6.0, 6000.0, 1.0);
            planned(n, 0.0, 400.0, 6000.0);
            n.io = Some(io(300, 20));
        })
    };
    let profile = b
        .child(OpKind::Hash, "Hash", |n| ran(n, 3.0, 3.0, 4000.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            on(n, "customers", Some("c"));
            ran(n, 0.0, 2.5, 4000.0, 1.0);
            n.io = Some(io(740, 0));
        })
        .with(|p| {
            p.statement.execution_ms = Some(if slow { 845.0 } else { 12.0 });
            p.statement.planning_ms = Some(1.2);
            p.sum_io();
        })
        .build();
    derive(profile)
}

fn changes(d: &ProfileDiff) -> Vec<Change> {
    d.rows.iter().map(|r| r.change).collect()
}

#[test]
fn the_same_plan_is_the_same_all_through() {
    let (a, b) = (join(true), join(true));
    let d = diff(&a, &b);
    assert_eq!(changes(&d), [Change::Same; 4]);
    assert!(!d.shape_changed);
    assert_eq!(d.measure, Some(Measure::Time));
    // Each row both sides' step, parents first, depths kept.
    assert!(d.rows.iter().all(|r| r.before.is_some() && r.after.is_some()));
    assert_eq!(d.rows.iter().map(|r| r.depth).collect::<Vec<_>>(), [0, 1, 1, 2]);
    assert_eq!(d.rows[3].parent, Some(2));
    assert_eq!(d.statement.time.trend(true), Trend::Even);
}

#[test]
fn a_seq_scan_that_became_an_index_scan_is_changed_not_removed_and_added() {
    let d = diff(&join(true), &join(false));
    assert_eq!(changes(&d), [Change::Same, Change::Changed, Change::Same, Change::Same]);
    assert!(d.shape_changed);
    let scan = &d.rows[1];
    assert_eq!(scan.time.before, Some(800.0));
    assert_eq!(scan.time.after, Some(6.0));
    assert_eq!(scan.time.trend(true), Trend::Better);
    assert_eq!(scan.buffers.change(), Some(320.0 - 52_000.0));
    assert_eq!(scan.read.trend(true), Trend::Better);
    // The statement: 845 ms → 12 ms, −98.6 %.
    let t = d.statement.time;
    assert_eq!((t.before, t.after), (Some(845.0), Some(12.0)));
    assert!((t.ratio().unwrap() + 0.9858).abs() < 1e-3);
    assert_eq!(t.trend(true), Trend::Better);
    // Backwards, worse.
    let back = diff(&join(false), &join(true));
    assert_eq!(back.statement.time.trend(true), Trend::Worse);
    assert_eq!(back.rows[1].change, Change::Changed);
}

#[test]
fn steps_added_and_removed_keep_their_subtrees() {
    // After: a Gather on top, the join under it; a Sort added; the Hash gone (customers read
    // by an index straight under the join).
    let after = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Gather, "Gather", |n| {
            ran(n, 1.0, 20.0, 6000.0, 1.0);
            n.parallel =
                Some(Parallel { planned: Some(2), launched: Some(2), ..Parallel::default() });
        })
        .child(OpKind::HashJoin, "Hash Join", |n| ran(n, 1.0, 18.0, 6000.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            on(n, "orders", Some("o"));
            ran(n, 0.0, 10.0, 6000.0, 1.0);
        })
        .child(OpKind::Sort, "Sort", |n| ran(n, 1.0, 2.0, 4000.0, 1.0))
        .node(OpKind::IndexScan, "Index Scan", |n| {
            on(n, "customers", Some("c"));
            ran(n, 0.0, 1.0, 4000.0, 1.0);
        })
        .build();
    let d = diff(&join(true), &derive(after));
    let rows: Vec<(Change, u32)> = d.rows.iter().map(|r| (r.change, r.depth)).collect();
    // The Gather added, the join met under it; orders the same; the Hash removed, the Sort
    // added; customers met (changed) under the Sort.
    assert_eq!(rows[0], (Change::Added, 0));
    assert_eq!(rows[1], (Change::Same, 1));
    assert_eq!(rows[2], (Change::Same, 2));
    assert!(rows.contains(&(Change::Removed, 2)), "{rows:?}");
    assert!(rows.contains(&(Change::Changed, 3)), "{rows:?}");
    let (same, changed, added, removed) = d.counts();
    assert_eq!((same, changed), (2, 1));
    assert!(added >= 2 && removed == 1, "{rows:?}");
    // Nothing of either side lost: every step is in exactly one row.
    let befores: Vec<_> = d.rows.iter().filter_map(|r| r.before).collect();
    let afters: Vec<_> = d.rows.iter().filter_map(|r| r.after).collect();
    assert_eq!(befores.len(), 4);
    assert_eq!(afters.len(), 5);
}

#[test]
fn a_whole_subtree_removed_or_added() {
    let small = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
            .node(OpKind::Result, "Result", |n| ran(n, 0.0, 0.01, 1.0, 1.0))
            .build(),
    );
    let d = diff(&join(true), &small);
    assert_eq!(d.rows.iter().filter(|r| r.change == Change::Removed).count(), 4);
    assert_eq!(d.rows.iter().filter(|r| r.change == Change::Added).count(), 1);
    // Removed first, then added, each subtree whole and nested.
    assert_eq!(d.rows[0].change, Change::Removed);
    assert_eq!(d.rows[3].parent, Some(2));
    assert_eq!(d.rows[4].change, Change::Added);
}

#[test]
fn the_order_of_children_doesnt_break_the_match() {
    // The join's sides swapped.
    let swapped = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::HashJoin, "Hash Join", |n| ran(n, 1.0, 840.0, 6000.0, 1.0))
        .child(OpKind::Hash, "Hash", |n| ran(n, 3.0, 3.0, 4000.0, 1.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            on(n, "customers", Some("c"));
            ran(n, 0.0, 2.5, 4000.0, 1.0);
        })
        .up()
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            on(n, "orders", Some("o"));
            ran(n, 0.1, 800.0, 6000.0, 1.0);
        })
        .build();
    let d = diff(&join(true), &derive(swapped));
    assert_eq!(changes(&d), [Change::Same; 4], "{:?}", changes(&d));
}

#[test]
fn deltas_say_better_and_worse_past_the_noise() {
    let d = Delta::new(Some(100.0), Some(103.0));
    assert_eq!(d.trend(true), Trend::Even, "3 % is noise");
    assert_eq!(Delta::new(Some(100.0), Some(130.0)).trend(true), Trend::Worse);
    assert_eq!(Delta::new(Some(100.0), Some(70.0)).trend(true), Trend::Better);
    assert_eq!(Delta::new(Some(100.0), Some(130.0)).trend(false), Trend::Better);
    assert_eq!(Delta::new(Some(0.0), Some(5.0)).trend(true), Trend::Worse, "from nothing");
    assert_eq!(Delta::new(Some(0.0), Some(0.0)).trend(true), Trend::Even);
    assert_eq!(Delta::new(None, Some(5.0)).trend(true), Trend::Even, "unknown");
    assert_eq!(Delta::new(Some(0.0), Some(5.0)).ratio(), None);
    assert_eq!(Delta::new(Some(4.0), Some(5.0)).change(), Some(1.0));
}

#[test]
fn findings_resolved_new_and_kept() {
    let mut a = join(true);
    let mut b = join(false);
    let scan = |r: &ProfileReport| r.order[1];
    a.findings = vec![
        Finding::new("hot-spot", Severity::Info).node(scan(&a)),
        Finding::new("seq-scan-selective", Severity::Critical).node(scan(&a)),
        Finding::new("cold-cache", Severity::Info),
        Finding::new("dead-tuples", Severity::Info)
            .text(crate::profile::rules::EvidenceKey::Table, "public.orders"),
    ];
    b.findings = vec![
        Finding::new("hot-spot", Severity::Info).node(scan(&b)),
        Finding::new("cold-cache", Severity::Info),
        Finding::new("dead-tuples", Severity::Info)
            .text(crate::profile::rules::EvidenceKey::Table, "orders"),
        Finding::new("misestimate", Severity::Warning).node(b.order[3]),
    ];
    let d = diff(&a, &b);
    assert_eq!(d.findings.resolved, [1], "the selective scan");
    assert_eq!(d.findings.new, [3], "the misestimate");
    assert_eq!(d.findings.kept, [(2, 1), (3, 2)], "the hot spot is no problem");
}

#[test]
fn estimated_plans_compare_by_cost() {
    let est = |cost: f64| {
        derive(
            ProfileBuilder::new("postgres", ProfileDepth::Estimated)
                .node(OpKind::SeqScan, "Seq Scan", |n| {
                    on(n, "orders", None);
                    planned(n, 0.0, cost, 100.0);
                })
                .build(),
        )
    };
    let d = diff(&est(1000.0), &est(10.0));
    assert_eq!(d.measure, Some(Measure::Cost));
    assert_eq!(d.statement.cost.trend(true), Trend::Better);
    assert_eq!(d.rows[0].rows.before, Some(100.0), "the rows expected");
    assert_eq!(d.statement.time.before, None);
}

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn thousands_of_partitions_compare_quickly() {
    let plan =
        |n: usize, index: bool| {
            let mut b = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
                .child(OpKind::Append, "Append", |n| ran(n, 0.0, 100.0, 5000.0, 1.0));
            for i in 0..n {
                let (op, name) = if index && i % 2 == 0 {
                    (OpKind::IndexScan, "Index Scan")
                } else {
                    (OpKind::SeqScan, "Seq Scan")
                };
                b = b.node(op, name, |node| {
                    on(node, &format!("events_{i}"), None);
                    ran(node, 0.0, 0.02, 1.0, 1.0);
                });
            }
            derive(b.build())
        };
    let (a, b) = (plan(5000, false), plan(5000, true));
    let started = std::time::Instant::now();
    let d = diff(&a, &b);
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
    assert_eq!(d.rows.len(), 5001, "each partition met, past the cells' limit too");
    assert_eq!(d.counts().1, 2500, "those read through an index changed");
    assert_eq!(diff(&a, &a).counts().0, 5001, "the same, greedily");
}
