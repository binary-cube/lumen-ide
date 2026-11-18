use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::{RewriteHint, Suggestion};
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

const TEXT: &str = "SELECT * FROM users WHERE lower(email) = 'a@b.ro'";

fn scan(filter: &str) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 30.0, 1.0, 1.0);
            n.actuals.rows_removed_by_filter = Some(99_999.0);
            on(n, "users", None);
            cond(n, CondKind::Filter, filter);
        })
        .build()
}

fn users(indexes: &[(&str, &[&str])]) -> crate::profile::rules::TableFacts {
    table("users", &["id:integer", "email:varchar(200)"], indexes)
}

#[test]
fn a_function_around_an_indexed_column() {
    let check = Check::postgres(TEXT).table(users(&[("users_email", &["email"])]));
    let found = check.rule(ID, scan("(lower((email)::text) = 'a@b.ro'::text)"));
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.text_of(E::Function), Some("lower"));
    assert_eq!(f.text_of(E::Index), Some("users_email"));
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_users_lower_email\" ON \"users\" ((lower(\"email\")));")
    );
    assert_eq!(&TEXT[f.sql_range.clone().unwrap()], "lower(email) = 'a@b.ro'");
}

#[test]
fn several_arguments_are_rewritten_not_indexed() {
    let check = Check::postgres(TEXT).table(users(&[("users_email", &["email"])]));
    let found = check.rule(ID, scan("(substr((email)::text, 1, 3) = 'abc'::text)"));
    assert_eq!(found[0].suggestion, Some(Suggestion::Rewrite { hint: RewriteHint::UnwrapColumn }));
}

#[test]
fn an_expression_index_or_no_index_says_nothing() {
    // Already indexed on the expression.
    let check = Check::postgres(TEXT)
        .table(users(&[("users_email", &["email"]), ("users_lower", &["lower((email)::text)"])]));
    assert!(check.rule(ID, scan("(lower((email)::text) = 'a@b.ro'::text)")).is_empty());
    // No index on the column: nothing lost by the function (seq-scan-selective's to say).
    let check = Check::postgres(TEXT).table(users(&[]));
    assert!(check.rule(ID, scan("(lower((email)::text) = 'a@b.ro'::text)")).is_empty());
    // A plain comparison.
    let check = Check::postgres(TEXT).table(users(&[("users_email", &["email"])]));
    assert!(check.rule(ID, scan("((email)::text = 'a@b.ro'::text)")).is_empty());
    // Nothing known.
    assert!(Check::postgres(TEXT).rule(ID, scan("(lower((email)::text) = 'x'::text)")).is_empty());
}

#[test]
fn sqlite_reads_it_in_the_where() {
    let text = "SELECT * FROM users WHERE lower(email) = ?";
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN users", |n| on(n, "users", None))
        .build();
    let check = Check::sqlite(text).table(users(&[("users_email", &["email"])]));
    let found = check.rule(ID, p);
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_users_lower_email\" ON \"users\" ((lower(\"email\")));")
    );
}
