use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::testing::{Check, ProfileBuilder, planned};

fn limited() -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .child(OpKind::Limit, "Limit", |n| planned(n, 0.0, 1.0, 10.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| planned(n, 0.0, 10.0, 100.0))
        .build()
}

#[test]
fn a_limit_without_an_order() {
    let text = "SELECT * FROM t LIMIT 10";
    let found = Check::postgres(text).rule(ID, limited());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].nodes, [crate::profile::NodeId(0)]);
    assert_eq!(&text[found[0].sql_range.clone().unwrap()], "LIMIT");
    let text = "SELECT * FROM t FETCH FIRST 5 ROWS ONLY";
    assert_eq!(Check::postgres(text).rule(ID, limited()).len(), 1);
}

#[test]
fn limit_one_with_an_order_is_fine() {
    assert!(Check::postgres("SELECT * FROM t ORDER BY id LIMIT 1").rule(ID, limited()).is_empty());
    assert!(Check::postgres("SELECT * FROM t").rule(ID, limited()).is_empty());
    // A write's LIMIT (SQLite's DELETE … LIMIT) isn't a query's page.
    assert!(Check::sqlite("DELETE FROM t LIMIT 5").rule(ID, limited()).is_empty());
}
