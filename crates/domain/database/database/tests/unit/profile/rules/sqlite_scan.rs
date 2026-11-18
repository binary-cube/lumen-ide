use super::*;
use crate::profile::EngineCounters;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, NodeId, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, table};

fn sqlite() -> ProfileBuilder {
    ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
}

#[test]
fn a_scan_with_a_where_on_it() {
    let text = "SELECT * FROM orders WHERE customer_id = 7";
    let p = sqlite().node(OpKind::SeqScan, "SCAN orders", |n| on(n, "orders", None)).build();
    let check =
        Check::sqlite(text).table(table("orders", &["id:INTEGER", "customer_id:INTEGER"], &[]));
    let found = check.rule(ID, p);
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert!(f.variant.is_none());
    assert_eq!(f.text_of(E::Columns), Some("customer_id"));
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\");")
    );
    assert_eq!(&text[f.sql_range.clone().unwrap()], "customer_id = 7");
}

#[test]
fn a_scan_inside_a_loop_by_its_join_column() {
    let text = "SELECT * FROM customers c JOIN orders o ON o.customer_id = c.id";
    let p = sqlite()
        .node(OpKind::SeqScan, "SCAN c", |n| on(n, "c", None))
        .node(OpKind::SeqScan, "SCAN o", |n| on(n, "o", None))
        .with(|p| {
            p.statement.counters =
                Some(EngineCounters { fullscan_steps: Some(500_000), ..EngineCounters::default() });
            p.statement.rows_returned = Some(100);
        })
        .build();
    let check = Check::sqlite(text)
        .table(table("orders", &["id:INTEGER", "customer_id:INTEGER"], &[]))
        .table(table("customers", &["id:INTEGER"], &[]));
    let found = check.rule(ID, p);
    assert_eq!(found.len(), 1, "the outer scan reads each row once: {found:?}");
    let f = &found[0];
    assert_eq!(f.nodes, [NodeId(1)]);
    assert_eq!(f.variant, Some("loop"));
    assert_eq!(f.text_of(E::Table), Some("orders"), "its alias taken back to its table");
    assert_eq!(f.quantity(E::Fullscan).unwrap().value, 500_000.0);
    assert!(f.impact >= 0.5, "the counters make it worse");
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\");")
    );
}

#[test]
fn a_whole_table_read_for_all_its_rows_is_right() {
    let p = sqlite().node(OpKind::SeqScan, "SCAN orders", |n| on(n, "orders", None)).build();
    assert!(Check::sqlite("SELECT * FROM orders").rule(ID, p.clone()).is_empty());
    // An index search isn't a scan; PostgreSQL's Seq Scan is another rule's.
    let p2 = sqlite()
        .node(OpKind::IndexScan, "SEARCH orders USING INDEX i (customer_id=?)", |n| {
            on(n, "orders", None);
            cond(n, CondKind::IndexCond, "(customer_id=?)");
        })
        .build();
    assert!(Check::sqlite("SELECT * FROM orders WHERE customer_id = 1").rule(ID, p2).is_empty());
    let mut pg = p;
    pg.engine.driver = "postgres".into();
    pg.engine.name = "PostgreSQL".into();
    assert!(Check::postgres("SELECT * FROM orders WHERE id = 1").rule(ID, pg).is_empty());
}

#[test]
fn a_small_table_is_read_faster_whole() {
    let text = "SELECT * FROM orders WHERE customer_id = 7";
    let p = sqlite()
        .node(OpKind::SeqScan, "SCAN orders", |n| {
            on(n, "orders", None);
            crate::profile::testing::ran(n, 0.0, 0.1, 999.0, 1.0);
        })
        .build();
    assert!(Check::sqlite(text).rule(ID, p.clone()).is_empty(), "999 rows read");
    let mut big = p;
    big.nodes[0].actuals.rows = Some(1_000.0);
    assert_eq!(Check::sqlite(text).rule(ID, big).len(), 1);
    // As its statistics say, the plan only estimated.
    let p = sqlite().node(OpKind::SeqScan, "SCAN orders", |n| on(n, "orders", None)).build();
    let mut facts = table("orders", &["customer_id:INTEGER"], &[]);
    facts.rows = Some(20);
    assert!(Check::sqlite(text).table(facts).rule(ID, p).is_empty());
}

#[test]
fn an_existing_index_isnt_suggested() {
    let text = "SELECT * FROM orders WHERE customer_id = 7";
    let p = sqlite().node(OpKind::SeqScan, "SCAN orders", |n| on(n, "orders", None)).build();
    let check = Check::sqlite(text).table(table(
        "orders",
        &["id:INTEGER", "customer_id:INTEGER"],
        &[("orders_customer", &["customer_id"])],
    ));
    let found = check.rule(ID, p);
    assert_eq!(found.len(), 1, "still read whole: the index isn't used");
    assert!(found[0].suggestion.is_none());
}
