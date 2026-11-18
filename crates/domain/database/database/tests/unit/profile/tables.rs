use super::*;
use crate::profile::node::CondKind;
use crate::profile::rules::{EvidenceKey, Finding, Severity};
use crate::profile::testing::{ProfileBuilder, cond, on, planned, ran};
use crate::profile::{ProfileDepth, derive};
use crate::sql::Flavor;

const PG: Flavor = Flavor::Postgres { version: Some(170_000) };

#[test]
fn each_table_once_with_how_it_was_read_and_what_it_cost() {
    let text =
        "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id WHERE o.status = 'x'";
    let mut report = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
            .child(OpKind::NestedLoop, "Nested Loop", |n| ran(n, 0.0, 100.0, 50.0, 1.0))
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                on(n, "orders", Some("o"));
                ran(n, 0.0, 90.0, 50.0, 1.0);
                n.actuals.rows_removed_by_filter = Some(9950.0);
            })
            .node(OpKind::IndexScan, "Index Scan", |n| {
                on(n, "customers", Some("c"));
                n.index = Some("customers_pkey".into());
                ran(n, 0.0, 0.1, 1.0, 50.0);
            })
            .with(|p| p.statement.execution_ms = Some(100.0))
            .build(),
    );
    report.findings = vec![
        Finding::new("hot-spot", Severity::Info).node(NodeId(1)),
        Finding::new("seq-scan-selective", Severity::Critical).node(NodeId(1)),
        Finding::new("dead-tuples", Severity::Info).text(EvidenceKey::Table, "public.customers"),
    ];
    let map = SqlMap::new(text, PG);
    let u = usage(&report, &map);
    assert_eq!(u.len(), 2);
    let orders = &u[0];
    assert_eq!(orders.table.name, "orders", "the heaviest first");
    assert_eq!(orders.aliases, ["o"]);
    assert_eq!(orders.accesses(), [Access::Scan]);
    assert_eq!(orders.rows_read, Some(10_000.0), "kept and removed");
    assert_eq!(orders.rows_returned, Some(50.0));
    assert!((orders.share.unwrap() - 0.9).abs() < 1e-9);
    assert_eq!(orders.findings, [1], "the hot spot is no problem");
    let customers = &u[1];
    assert_eq!(customers.accesses(), [Access::Index]);
    assert_eq!(customers.indexes_used(), ["customers_pkey"]);
    assert!(customers.uses_index("CUSTOMERS_PKEY"));
    assert_eq!(customers.rows_returned, Some(50.0), "in all its loops");
    assert_eq!(customers.findings, [2], "by its name");
}

#[test]
fn the_same_table_twice_is_one_table_with_both_aliases() {
    let text = "SELECT * FROM orders a JOIN orders b ON b.parent_id = a.id";
    let report = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
            .child(OpKind::HashJoin, "Hash Join", |n| ran(n, 0.0, 10.0, 5.0, 1.0))
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                on(n, "orders", Some("a"));
                ran(n, 0.0, 4.0, 100.0, 1.0);
            })
            .child(OpKind::Hash, "Hash", |n| ran(n, 0.0, 5.0, 100.0, 1.0))
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                on(n, "orders", Some("b"));
                ran(n, 0.0, 4.5, 100.0, 1.0);
            })
            .build(),
    );
    let u = usage(&report, &SqlMap::new(text, PG));
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].aliases, ["a", "b"]);
    assert_eq!(u[0].scans.len(), 2);
    assert_eq!(u[0].rows_read, Some(200.0));
    assert_eq!(u[0].nodes(), [NodeId(1), NodeId(3)]);
}

#[test]
fn sqlite_names_a_table_by_its_alias_and_a_bitmap_by_its_index() {
    let text = "SELECT * FROM orders o WHERE o.total > 10";
    let report = derive(
        ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
            .node(OpKind::SeqScan, "SCAN o", |n| on(n, "o", None))
            .build(),
    );
    let u = usage(&report, &SqlMap::new(text, Flavor::Sqlite));
    assert_eq!(u[0].table.name, "orders");
    assert_eq!(u[0].aliases, ["o"]);
    assert!(u[0].estimated);
    assert_eq!(u[0].rows_read, None, "nothing estimated either");

    let report = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Estimated)
            .child(OpKind::BitmapHeapScan, "Bitmap Heap Scan", |n| {
                on(n, "orders", None);
                planned(n, 0.0, 500.0, 120.0);
                cond(n, CondKind::Recheck, "(status = 'x'::text)");
            })
            .node(OpKind::BitmapIndexScan, "Bitmap Index Scan", |n| {
                n.index = Some("idx_orders_status".into());
                planned(n, 0.0, 10.0, 120.0);
            })
            .build(),
    );
    let u = usage(&report, &SqlMap::new("SELECT * FROM orders", PG));
    assert_eq!(u.len(), 1, "the index scan isn't a table");
    assert_eq!(u[0].accesses(), [Access::Bitmap]);
    assert_eq!(u[0].indexes_used(), ["idx_orders_status"]);
    assert_eq!(u[0].rows_read, Some(120.0), "estimated");
    assert!(u[0].share.is_some(), "of the cost");
}

#[test]
fn a_write_counts_its_table() {
    let report = derive(
        ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
            .child(OpKind::ModifyTable(crate::profile::node::WriteKind::Update), "Update", |n| {
                on(n, "orders", None);
                ran(n, 0.0, 5.0, 0.0, 1.0);
            })
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                on(n, "orders", None);
                ran(n, 0.0, 2.0, 10.0, 1.0);
            })
            .build(),
    );
    let u = usage(&report, &SqlMap::new("UPDATE orders SET total = 0", PG));
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].accesses(), [Access::Scan, Access::Write]);
    assert_eq!(u[0].rows_read, Some(10.0), "the write reads nothing more");
}
