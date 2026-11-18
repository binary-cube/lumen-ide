use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::{RewriteHint, Suggestion};
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

fn scan(filter: &str) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 20.0, 2.0, 1.0);
            on(n, "users", None);
            cond(n, CondKind::Filter, filter);
        })
        .build()
}

fn users(indexes: &[(&str, &[&str])]) -> crate::profile::rules::TableFacts {
    table("users", &["id:integer", "email", "phone"], indexes)
}

#[test]
fn an_or_of_two_indexed_columns() {
    let check = Check::postgres("SELECT 1")
        .table(users(&[("users_email", &["email"]), ("users_phone", &["phone"])]));
    let found = check.rule(ID, scan("((email = 'a'::text) OR (phone = '1'::text))"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Info);
    assert_eq!(found[0].text_of(E::Columns), Some("email, phone"));
    assert_eq!(found[0].suggestion, Some(Suggestion::Rewrite { hint: RewriteHint::UnionAll }));
}

#[test]
fn the_same_column_twice_is_an_in() {
    let check = Check::postgres("SELECT 1").table(users(&[("users_email", &["email"])]));
    assert!(check.rule(ID, scan("((email = 'a'::text) OR (email = 'b'::text))")).is_empty());
}

#[test]
fn a_branch_without_an_index_reads_the_table_anyway() {
    let check = Check::postgres("SELECT 1").table(users(&[("users_email", &["email"])]));
    assert!(check.rule(ID, scan("((email = 'a'::text) OR (phone = '1'::text))")).is_empty());
    assert!(
        Check::postgres("SELECT 1")
            .rule(ID, scan("((email = 'a'::text) OR (phone = '1'::text))"))
            .is_empty()
    );
}

#[test]
fn sqlite_reads_its_where() {
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN users", |n| on(n, "users", None))
        .build();
    let check = Check::sqlite("SELECT * FROM users WHERE email = ? OR phone = ?")
        .table(users(&[("users_email", &["email"]), ("users_phone", &["phone"])]));
    assert_eq!(check.rule(ID, p).len(), 1);
    // `MULTI-INDEX OR` already uses both: no scan to warn of.
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .child(OpKind::MultiIndexOr, "MULTI-INDEX OR", |_| {})
        .node(OpKind::IndexScan, "SEARCH users USING INDEX users_email (email=?)", |n| {
            on(n, "users", None)
        })
        .build();
    assert!(check.rule(ID, p).is_empty());
}
