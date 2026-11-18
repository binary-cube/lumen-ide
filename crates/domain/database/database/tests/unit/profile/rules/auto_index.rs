use super::*;
use crate::profile::EngineCounters;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, table};

const TEXT: &str = "SELECT * FROM customers c JOIN orders o ON o.customer_id = c.id";

#[test]
fn an_automatic_index_made_for_good() {
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Actual { timing: false })
        .node(OpKind::SeqScan, "SCAN c", |n| on(n, "c", None))
        .node(OpKind::AutoIndex, "SEARCH o USING AUTOMATIC COVERING INDEX (customer_id=?)", |n| {
            on(n, "o", None);
            cond(n, CondKind::IndexCond, "(customer_id=?)");
        })
        .with(|p| {
            p.statement.counters =
                Some(EngineCounters { autoindex: Some(50_000), ..EngineCounters::default() })
        })
        .build();
    let check =
        Check::sqlite(TEXT).table(table("orders", &["id:INTEGER", "customer_id:INTEGER"], &[]));
    let found = check.rule(ID, p);
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.quantity(E::Read).unwrap().value, 50_000.0);
    assert_eq!(f.text_of(E::Table), Some("orders"));
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\");")
    );
    assert_eq!(&TEXT[f.sql_range.clone().unwrap()], "orders o");
}

#[test]
fn the_counter_alone_says_it() {
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Actual { timing: false })
        .node(OpKind::SeqScan, "SCAN t", |n| on(n, "t", None))
        .with(|p| {
            p.statement.counters =
                Some(EngineCounters { autoindex: Some(10), ..EngineCounters::default() })
        })
        .build();
    let found = Check::sqlite("SELECT 1").rule(ID, p);
    assert_eq!(found.len(), 1);
    assert!(found[0].nodes.is_empty() && found[0].suggestion.is_none());
}

#[test]
fn none_built_none_said() {
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Actual { timing: false })
        .node(OpKind::IndexScan, "SEARCH t USING INDEX i (a=?)", |n| on(n, "t", None))
        .with(|p| {
            p.statement.counters =
                Some(EngineCounters { autoindex: Some(0), ..EngineCounters::default() })
        })
        .build();
    assert!(Check::sqlite("SELECT 1").rule(ID, p).is_empty());
}
