use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

const TEXT: &str = "SELECT * FROM orders o WHERE o.status = 'paid'";

/// A scan of `orders o` keeping `kept` of `read` rows, the whole statement's time.
fn scan(read: f64, kept: f64, filter: &str) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 10.0, kept, 1.0);
            n.actuals.rows_removed_by_filter = Some(read - kept);
            n.estimates.rows = Some(kept);
            on(n, "orders", Some("o"));
            cond(n, CondKind::Filter, filter);
        })
        .with(|p| p.statement.execution_ms = Some(10.0))
        .build()
}

fn orders(indexes: &[(&str, &[&str])]) -> crate::profile::rules::TableFacts {
    table("orders", &["id:integer", "status:varchar(20)", "email", "total:numeric"], indexes)
}

#[test]
fn a_scan_reading_many_to_keep_few_wants_an_index() {
    let check = Check::postgres(TEXT).table(orders(&[("orders_pkey", &["id"])]));
    let found = check.rule(ID, scan(100_000.0, 50.0, "((status)::text = 'paid'::text)"));
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.severity, Severity::Critical, "all of the time");
    assert_eq!(f.quantity(E::Read).unwrap().value, 100_000.0);
    assert_eq!(f.quantity(E::Kept).unwrap().value, 50.0);
    // A varchar column is cast to text in the plan's words: still the column, as stored.
    assert_eq!(f.text_of(E::Columns), Some("status"));
    let Some(Suggestion::CreateIndex { sql, concurrently, .. }) = &f.suggestion else {
        panic!("{:?}", f.suggestion)
    };
    assert_eq!(sql, "CREATE INDEX \"idx_orders_status\" ON \"orders\" (\"status\");");
    assert!(!concurrently);
    assert_eq!(&TEXT[f.sql_range.clone().unwrap()], "o.status = 'paid'");
    // A column not in the text: its table's place.
    let found = check.rule(ID, scan(100_000.0, 50.0, "(total > '100'::numeric)"));
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_total\" ON \"orders\" (\"total\");")
    );
    assert_eq!(&TEXT[found[0].sql_range.clone().unwrap()], "orders o");
}

#[test]
fn equalities_first_then_a_range_and_its_place() {
    let text = "SELECT * FROM orders WHERE email = 'a@b.ro' AND total > 10";
    let check = Check::postgres(text).table(orders(&[]));
    let found = check
        .rule(ID, scan(50_000.0, 10.0, "((total > '10'::numeric) AND (email = 'a@b.ro'::text))"));
    let f = &found[0];
    assert_eq!(f.text_of(E::Columns), Some("email, total"));
    assert_eq!(
        f.suggestion.as_ref().and_then(Suggestion::sql),
        Some("CREATE INDEX \"idx_orders_email_total\" ON \"orders\" (\"email\", \"total\");")
    );
    assert_eq!(&text[f.sql_range.clone().unwrap()], "email = 'a@b.ro'");
}

#[test]
fn on_production_the_index_is_made_concurrently() {
    let check = Check::postgres(TEXT).table(orders(&[])).production();
    let found = check.rule(ID, scan(100_000.0, 50.0, "(email = 'x'::text)"));
    let Some(Suggestion::CreateIndex { sql, concurrently, .. }) = &found[0].suggestion else {
        panic!()
    };
    assert!(*concurrently);
    assert_eq!(sql, "CREATE INDEX CONCURRENTLY \"idx_orders_email\" ON \"orders\" (\"email\");");
}

#[test]
fn at_the_limits() {
    let check = Check::postgres(TEXT).table(orders(&[]));
    // 10 000 read, 10 % kept: just enough.
    assert_eq!(check.rule(ID, scan(10_000.0, 1_000.0, "(email = 'x'::text)")).len(), 1);
    assert!(check.rule(ID, scan(9_999.0, 10.0, "(email = 'x'::text)")).is_empty(), "too few read");
    assert!(
        check.rule(ID, scan(10_000.0, 1_001.0, "(email = 'x'::text)")).is_empty(),
        "keeps over 10 %"
    );
}

#[test]
fn a_small_table_or_one_read_almost_whole_is_fine() {
    let check = Check::postgres(TEXT).table(orders(&[]));
    assert!(check.rule(ID, scan(800.0, 2.0, "(email = 'x'::text)")).is_empty(), "small");
    assert!(
        check.rule(ID, scan(100_000.0, 95_000.0, "(email = 'x'::text)")).is_empty(),
        "almost all"
    );
    // No filter: a full read is what's asked.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 10.0, 100_000.0, 1.0);
            on(n, "orders", None);
        })
        .build();
    assert!(check.rule(ID, p).is_empty());
}

#[test]
fn an_existing_index_says_why_it_isnt_used() {
    let check = Check::postgres(TEXT).table(orders(&[("orders_email", &["email"])]));
    // Plain and well estimated: the planner judged the index not worth it.
    let found = check.rule(ID, scan(100_000.0, 50.0, "(email = 'x'::text)"));
    assert_eq!(found[0].variant, Some("index_selectivity"));
    assert_eq!(found[0].text_of(E::Index), Some("orders_email"));
    assert!(found[0].suggestion.is_none(), "never an index that exists");
    // A function around it.
    let found = check.rule(ID, scan(100_000.0, 50.0, "(lower(email) = 'x'::text)"));
    assert_eq!(found[0].variant, Some("index_function"));
    assert_eq!(found[0].text_of(E::Function), Some("lower"));
    // A cast.
    let check = Check::postgres(TEXT).table(orders(&[("orders_total", &["total"])]));
    let found = check.rule(ID, scan(100_000.0, 50.0, "((total)::text = '5'::text)"));
    assert_eq!(found[0].variant, Some("index_cast"));
    // Badly estimated: the statistics.
    let mut p = scan(100_000.0, 50.0, "(total = 5)");
    p.nodes[0].estimates.rows = Some(60_000.0);
    let found = check.rule(ID, p);
    assert_eq!(found[0].variant, Some("index_statistics"));
    assert_eq!(found[0].suggestion.as_ref().and_then(Suggestion::sql), Some("ANALYZE \"orders\";"));
}

#[test]
fn without_facts_no_index_is_guessed() {
    let found = Check::postgres(TEXT).rule(ID, scan(100_000.0, 50.0, "(email = 'x'::text)"));
    assert_eq!(found.len(), 1);
    assert!(found[0].suggestion.is_none());
    assert_eq!(found[0].text_of(E::Columns), Some("email"));
}

#[test]
fn an_estimated_plan_by_the_tables_rows() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            crate::profile::testing::planned(n, 0.0, 2000.0, 20.0);
            on(n, "orders", None);
            cond(n, CondKind::Filter, "(email = 'x'::text)");
        })
        .build();
    let mut facts = orders(&[]);
    facts.rows = Some(80_000);
    let found = Check::postgres(TEXT).table(facts).rule(ID, p.clone());
    assert_eq!(found[0].quantity(E::Read).unwrap().value, 80_000.0);
    assert!(found[0].suggestion.is_some());
    // Its rows unknown: nothing to weigh.
    assert!(Check::postgres(TEXT).rule(ID, p).is_empty());
}
