use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

fn scan(filter: &str, driver: &str) -> crate::profile::Profile {
    ProfileBuilder::new(driver, ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 20.0, 2.0, 1.0);
            on(n, "users", None);
            cond(n, CondKind::Filter, filter);
        })
        .build()
}

fn users() -> crate::profile::rules::TableFacts {
    table("users", &["id:integer", "name"], &[("users_name", &["name"])])
}

#[test]
fn a_pattern_starting_with_a_wildcard_on_postgres() {
    let text = "SELECT * FROM users WHERE name LIKE '%escu'";
    let found = Check::postgres(text)
        .table(users())
        .rule(ID, scan("((name)::text ~~ '%escu'::text)", "postgres"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].variant, Some("trigram"));
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some(
            "CREATE INDEX \"idx_users_name_trgm\" ON \"users\" USING gin (\"name\" gin_trgm_ops);"
        )
    );
    assert_eq!(&text[found[0].sql_range.clone().unwrap()], "name LIKE '%escu'");
}

#[test]
fn a_prefix_pattern_or_no_index_is_fine() {
    let check = Check::postgres("SELECT 1").table(users());
    assert!(check.rule(ID, scan("((name)::text ~~ 'Ion%'::text)", "postgres")).is_empty());
    let check = Check::postgres("SELECT 1").table(table("users", &["name"], &[]));
    assert!(check.rule(ID, scan("((name)::text ~~ '%escu'::text)", "postgres")).is_empty());
}

#[test]
fn sqlite_has_no_trigram_index() {
    let p = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN users", |n| on(n, "users", None))
        .build();
    let found =
        Check::sqlite("SELECT * FROM users WHERE name LIKE '%escu'").table(users()).rule(ID, p);
    assert_eq!(found.len(), 1);
    assert!(found[0].variant.is_none() && found[0].suggestion.is_none());
}
