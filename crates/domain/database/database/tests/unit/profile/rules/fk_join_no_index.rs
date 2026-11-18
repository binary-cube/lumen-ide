use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, NodeId, OpKind};
use crate::profile::rules::{Suggestion, TableFacts};
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};
use crate::relations::VirtualKey;
use crate::{ForeignKey, TableRef};

const TEXT: &str = "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id";

/// `orders o` scanned whole, hashed against `customers c`.
fn hash_join() -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::HashJoin, "Hash Join", |n| {
            ran(n, 0.0, 50.0, 1000.0, 1.0);
            cond(n, CondKind::HashCond, "(o.customer_id = c.id)");
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 30.0, 100_000.0, 1.0);
            on(n, "orders", Some("o"));
        })
        .child(OpKind::Hash, "Hash", |n| ran(n, 0.0, 5.0, 1000.0, 1.0))
        .node(OpKind::IndexScan, "Index Scan", |n| {
            ran(n, 0.0, 4.0, 1000.0, 1.0);
            on(n, "customers", Some("c"));
        })
        .with(|p| p.statement.execution_ms = Some(50.0))
        .build()
}

fn orders(indexes: &[(&str, &[&str])], key: bool) -> TableFacts {
    let mut t = table("orders", &["id:integer", "customer_id:integer"], indexes);
    if key {
        t.structure.as_mut().unwrap().foreign_keys.push(ForeignKey {
            name: Some("orders_customer_fk".into()),
            columns: vec!["customer_id".into()],
            table: TableRef::new("customers"),
            references: vec!["id".into()],
            on_update: Default::default(),
            on_delete: Default::default(),
        });
    }
    t
}

fn customers() -> TableFacts {
    table("customers", &["id:integer", "name"], &[("customers_pkey", &["id"])])
}

#[test]
fn a_foreign_key_joined_without_an_index() {
    let check =
        Check::postgres(TEXT).table(orders(&[("orders_pkey", &["id"])], true)).table(customers());
    let found = check.rule(ID, hash_join());
    assert_eq!(found.len(), 1, "{found:?}");
    let f = &found[0];
    assert_eq!(f.nodes, [NodeId(0), NodeId(1)]);
    assert_eq!(f.text_of(E::Columns), Some("customer_id"));
    assert_eq!(f.text_of(E::Target), Some("customers(id)"));
    assert!(f.variant.is_none());
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\");")
    );
    assert_eq!(&TEXT[f.sql_range.clone().unwrap()], "c.id = o.customer_id");
}

#[test]
fn an_accepted_virtual_key_counts_too() {
    let mut check = Check::postgres(TEXT).table(orders(&[], false)).table(customers());
    assert!(check.rule(ID, hash_join()).is_empty(), "no key: nothing to say");
    check.facts.notes.accept(VirtualKey {
        table: TableRef::new("orders"),
        columns: vec!["customer_id".into()],
        target: TableRef::new("customers"),
        references: vec!["id".into()],
    });
    let found = check.rule(ID, hash_join());
    assert_eq!(found[0].variant, Some("virtual"));
}

#[test]
fn an_index_already_there_or_no_structure_says_nothing() {
    let check = Check::postgres(TEXT)
        .table(orders(&[("orders_customer", &["customer_id", "id"])], true))
        .table(customers());
    assert!(check.rule(ID, hash_join()).is_empty(), "an index starts with the key");
    // Nothing known: nothing guessed.
    assert!(Check::postgres(TEXT).rule(ID, hash_join()).is_empty());
    // The referencing side read through an index, not whole.
    let mut p = hash_join();
    p.nodes[1].op = OpKind::IndexScan;
    let check = Check::postgres(TEXT).table(orders(&[], true)).table(customers());
    assert!(check.rule(ID, p).is_empty());
}
