use std::time::Duration;

use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::{RewriteHint, Suggestion};
use crate::profile::testing::{Check, ProfileBuilder};
use crate::sql::{Flavor, fingerprint};

fn any() -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::IndexScan, "Index Scan", |_| {})
        .build()
}

/// The check with `runs` runs of `text`'s query (other values), one a second.
fn ran(text: &str, runs: u64, every: Duration) -> Check {
    let mut check = Check::postgres("SELECT * FROM orders WHERE customer_id = 7");
    for i in 0..runs {
        let other = text.replace("{n}", &i.to_string());
        let print = fingerprint(&other, Flavor::Postgres { version: Some(170_000) });
        check.recent.record(print.hash, every * i as u32);
    }
    check
}

#[test]
fn the_same_query_run_twenty_times_in_a_minute() {
    let check = ran("select * from orders where customer_id = {n}", 20, Duration::from_secs(1));
    let found = check.rule(ID, any());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].quantity(E::Runs).unwrap().value, 20.0);
    assert_eq!(found[0].suggestion, Some(Suggestion::Rewrite { hint: RewriteHint::Batch }));
    assert_eq!(found[0].sql_range, Some(0..check.text.len()));
}

#[test]
fn fewer_runs_or_spread_out_are_fine() {
    assert!(
        ran("select * from orders where customer_id = {n}", 19, Duration::from_secs(1))
            .rule(ID, any())
            .is_empty()
    );
    // Twenty runs over twenty minutes: not a loop.
    assert!(
        ran("select * from orders where customer_id = {n}", 20, Duration::from_secs(60))
            .rule(ID, any())
            .is_empty()
    );
    // Another query run often.
    assert!(
        ran("select * from users where id = {n}", 50, Duration::from_millis(10))
            .rule(ID, any())
            .is_empty()
    );
}

#[test]
fn the_servers_history_says_it_too() {
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::IndexScan, "Index Scan", |_| {})
        .with(|p| {
            p.statement.history = Some(crate::profile::StatementStats {
                calls: Some(120_000),
                mean_ms: Some(0.08),
                ..Default::default()
            })
        })
        .build();
    let found = Check::postgres("SELECT 1").rule(ID, p.clone());
    assert_eq!(found[0].variant, Some("server"));
    let mut slow = p;
    slow.statement.history.as_mut().unwrap().mean_ms = Some(40.0);
    assert!(Check::postgres("SELECT 1").rule(ID, slow).is_empty(), "slow calls aren't round trips");
}
