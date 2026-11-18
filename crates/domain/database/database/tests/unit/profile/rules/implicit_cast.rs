use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::{RewriteHint, Suggestion};
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

fn scan(kind: CondKind, text: &str) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 20.0, 1.0, 1.0);
            on(n, "orders", None);
            cond(n, kind, text);
        })
        .build()
}

fn orders() -> crate::profile::rules::TableFacts {
    table(
        "orders",
        &["id:integer", "code:integer", "status:varchar(20)"],
        &[("orders_code", &["code"]), ("orders_status", &["status"])],
    )
}

#[test]
fn an_indexed_column_converted_to_compare_it() {
    let text = "SELECT * FROM orders WHERE code = '7'::text";
    let found = Check::postgres(text)
        .table(orders())
        .rule(ID, scan(CondKind::Filter, "((code)::text = '7'::text)"));
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.text_of(E::Column), Some("code"));
    assert_eq!(f.text_of(E::Type), Some("text"));
    assert_eq!(f.text_of(E::Value), Some("integer"));
    assert_eq!(f.suggestion, Some(Suggestion::Rewrite { hint: RewriteHint::MatchLiteralType }));
}

#[test]
fn postgres_varchar_casts_are_no_conversion() {
    let found = Check::postgres("SELECT 1")
        .table(orders())
        .rule(ID, scan(CondKind::Filter, "((status)::text = 'paid'::text)"));
    assert!(found.is_empty());
}

#[test]
fn a_cast_in_the_index_condition_or_without_an_index_is_fine() {
    let check = Check::postgres("SELECT 1").table(orders());
    assert!(check.rule(ID, scan(CondKind::IndexCond, "((code)::text = '7'::text)")).is_empty());
    let check = Check::postgres("SELECT 1").table(table("orders", &["code:integer"], &[]));
    assert!(check.rule(ID, scan(CondKind::Filter, "((code)::text = '7'::text)")).is_empty());
    // Without the structure, the column's type is unknown.
    assert!(
        Check::postgres("SELECT 1")
            .rule(ID, scan(CondKind::Filter, "((code)::text = '7'::text)"))
            .is_empty()
    );
}
