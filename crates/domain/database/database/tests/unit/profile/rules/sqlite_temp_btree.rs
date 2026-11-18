use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, table};

fn sorted(purpose: &str, rows: Option<u64>) -> crate::profile::Profile {
    ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN orders", |n| on(n, "orders", None))
        .node(OpKind::TempBTree, &format!("USE TEMP B-TREE FOR {purpose}"), |n| {
            cond(n, CondKind::SortKey, purpose);
        })
        .with(|p| p.statement.rows_returned = rows)
        .build()
}

fn orders(rows: u64) -> crate::profile::rules::TableFacts {
    let mut t = table("orders", &["id:INTEGER", "status", "created:INTEGER"], &[]);
    t.rows = Some(rows);
    t
}

#[test]
fn an_order_by_over_many_rows_wants_its_index() {
    let text = "SELECT * FROM orders WHERE status = 'paid' ORDER BY created DESC";
    let check = Check::sqlite(text).table(orders(50_000));
    let found = check.rule(ID, sorted("ORDER BY", None));
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.text_of(E::Purpose), Some("ORDER BY"));
    assert_eq!(f.quantity(E::Read).unwrap().value, 50_000.0);
    assert_eq!(f.text_of(E::Columns), Some("status, created"), "the WHERE's equality first");
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some(
            "CREATE INDEX \"idx_orders_status_created\" ON \"orders\" (\"status\", \"created\" DESC);"
        )
    );
    assert_eq!(&text[f.sql_range.clone().unwrap()], "ORDER BY created DESC");
    // A hundred thousand rows: a warning.
    let check = Check::sqlite(text).table(orders(200_000));
    assert_eq!(check.rule(ID, sorted("ORDER BY", None))[0].severity, Severity::Warning);
}

#[test]
fn a_few_rows_sorted_is_nothing() {
    let text = "SELECT * FROM orders ORDER BY created";
    assert!(Check::sqlite(text).table(orders(999)).rule(ID, sorted("ORDER BY", None)).is_empty());
    assert_eq!(
        Check::sqlite(text).table(orders(1_000)).rule(ID, sorted("ORDER BY", None)).len(),
        1
    );
    // Rows unknown: nothing weighed.
    assert!(Check::sqlite(text).rule(ID, sorted("ORDER BY", None)).is_empty());
    // The statement's rows count too.
    assert_eq!(Check::sqlite(text).rule(ID, sorted("ORDER BY", Some(5_000))).len(), 1);
}

#[test]
fn a_distinct_or_an_expression_has_no_index_to_write() {
    let text = "SELECT DISTINCT status FROM orders";
    let found = Check::sqlite(text).table(orders(50_000)).rule(ID, sorted("DISTINCT", None));
    assert!(found[0].suggestion.is_none());
    let text = "SELECT * FROM orders ORDER BY lower(status)";
    let found = Check::sqlite(text).table(orders(50_000)).rule(ID, sorted("ORDER BY", None));
    assert!(found[0].suggestion.is_none());
}
