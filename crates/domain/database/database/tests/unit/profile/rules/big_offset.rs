use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::{RewriteHint, Suggestion};
use crate::profile::testing::{Check, ProfileBuilder, planned};

fn limited() -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .child(OpKind::Limit, "Limit", |n| planned(n, 100.0, 101.0, 10.0))
        .node(OpKind::IndexScan, "Index Scan", |n| planned(n, 0.0, 1000.0, 100_000.0))
        .build()
}

#[test]
fn a_big_offset_wants_keyset_pagination() {
    let text = "SELECT * FROM t ORDER BY id LIMIT 20 OFFSET 50000";
    let found = Check::postgres(text).rule(ID, limited());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].quantity(E::Offset).unwrap().value, 50_000.0);
    assert_eq!(
        found[0].suggestion,
        Some(Suggestion::Rewrite { hint: RewriteHint::KeysetPagination })
    );
    assert_eq!(&text[found[0].sql_range.clone().unwrap()], "OFFSET 50000");
    // SQLite's `LIMIT offset, count`.
    let found = Check::sqlite("SELECT * FROM t ORDER BY id LIMIT 10000, 20").rule(ID, limited());
    assert_eq!(found.len(), 1, "at the limit");
}

#[test]
fn a_small_or_unknown_offset_is_fine() {
    assert!(
        Check::postgres("SELECT * FROM t ORDER BY id LIMIT 20 OFFSET 9999")
            .rule(ID, limited())
            .is_empty()
    );
    assert!(
        Check::postgres("SELECT * FROM t ORDER BY id LIMIT 20 OFFSET $1")
            .rule(ID, limited())
            .is_empty()
    );
    assert!(Check::postgres("SELECT * FROM t LIMIT 20").rule(ID, limited()).is_empty());
}
