use std::time::Instant;

use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{CondKind, OpKind, Parallel};
use crate::profile::testing::{Check, ProfileBuilder, cond, on, ran, table};

#[test]
fn every_rule_has_a_stable_id_once() {
    let ids: Vec<&str> = rules().iter().map(|r| r.id()).collect();
    assert_eq!(ids.len(), 29);
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    assert!(ids.iter().all(|id| id.chars().all(|c| c.is_ascii_lowercase() || c == '-')));
    assert_eq!(ids[0], "hot-spot");
    // Three texts each, and two per variant.
    let keys = text_keys();
    let variants: usize = rules().iter().map(|r| r.variants().len()).sum();
    assert_eq!(keys.len(), 29 * 3 + variants * 2);
    assert!(keys.contains(&"profile.finding.seq-scan-selective.why_index_function".to_owned()));
}

#[test]
fn a_findings_texts_are_its_rules_or_its_variants() {
    let f = Finding::new("misestimate", Severity::Warning);
    assert_eq!(f.text_key("title"), "profile.finding.misestimate.title");
    assert_eq!(f.text_key("why"), "profile.finding.misestimate.why");
    let f = f.variant("join");
    assert_eq!(f.text_key("title"), "profile.finding.misestimate.title", "one title");
    assert_eq!(f.text_key("fix"), "profile.finding.misestimate.fix_join");
    assert!(Finding::new("x", Severity::Info).impact(f64::NAN).impact == 0.0);
    assert_eq!(Finding::new("x", Severity::Info).impact(3.0).impact, 1.0);
}

/// A plan with several problems: a selective scan (critical), a spill, a misestimate.
fn troubled() -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Sort, "Sort", |n| {
            ran(n, 0.0, 100.0, 50.0, 1.0);
            n.estimates.rows = Some(50.0);
            n.spill = Some(crate::profile::Spill {
                kind: crate::profile::SpillKind::Sort,
                disk_bytes: Some(10 << 20),
                batches: None,
            });
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 80.0, 50.0, 1.0);
            n.estimates.rows = Some(5_000.0);
            n.actuals.rows_removed_by_filter = Some(199_950.0);
            on(n, "orders", None);
            cond(n, CondKind::Filter, "(customer_id = 7)");
        })
        .with(|p| p.statement.execution_ms = Some(100.0))
        .build()
}

#[test]
fn the_hot_spot_first_then_the_worst() {
    let check = Check::postgres("SELECT * FROM orders WHERE customer_id = 7 ORDER BY id")
        .table(table("orders", &["id:integer", "customer_id:integer"], &[]));
    let found = check.analyze(troubled());
    let rules: Vec<&str> = found.iter().map(|f| f.rule).collect();
    assert_eq!(rules[0], "hot-spot");
    assert_eq!(rules[1], "seq-scan-selective", "critical: {rules:?}");
    assert!(rules.contains(&"spill-to-disk") && rules.contains(&"misestimate"));
    for pair in found[1..].windows(2) {
        assert!(
            pair[0].severity > pair[1].severity
                || (pair[0].severity == pair[1].severity && pair[0].impact >= pair[1].impact),
            "{rules:?}"
        );
    }
    let (problems, critical) = problems(&found);
    assert_eq!(problems, found.len() - 1, "the hot spot isn't a problem");
    assert_eq!(critical, 1);
}

#[test]
fn a_mysql_profile_in_the_normalized_form_is_read_the_same() {
    // MySQL's `access_type: ALL` with `attached_condition`, as its driver will normalize it.
    let p = ProfileBuilder::new("mysql", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Table scan", |n| {
            ran(n, 0.0, 40.0, 12.0, 1.0);
            n.actuals.rows_removed_by_filter = Some(80_000.0);
            on(n, "orders", None);
            cond(n, CondKind::Filter, "(`orders`.`status` = 'x')");
        })
        .with(|p| p.statement.execution_ms = Some(40.0))
        .build();
    let check = Check::new("SELECT * FROM orders WHERE status = 'x'", crate::sql::Flavor::MySql)
        .table(table("orders", &["status"], &[]));
    let found = check.analyze(p);
    let scan = found.iter().find(|f| f.rule == "seq-scan-selective").unwrap();
    assert_eq!(scan.text_of(EvidenceKey::Columns), Some("status"));
    assert_eq!(
        scan.suggestion.as_ref().and_then(Suggestion::sql),
        // In MySQL's own quoting (its dialect, plan 39p).
        Some("CREATE INDEX `idx_orders_status` ON `orders` (`status`);")
    );
}

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn ten_thousand_nodes_within_the_budget() {
    // An Append of 5 000 partitions, each a filtered scan; a parallel gather above.
    let mut b = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Gather, "Gather", |n| {
            ran(n, 0.0, 500.0, 1000.0, 1.0);
            n.parallel =
                Some(Parallel { planned: Some(4), launched: Some(2), ..Parallel::default() });
        })
        .child(OpKind::Append, "Append", |n| ran(n, 0.0, 480.0, 1000.0, 3.0));
    for i in 0..5_000 {
        let name = format!("orders_p{i}");
        b = b
            .child(OpKind::NestedLoop, "Nested Loop", |n| ran(n, 0.0, 0.09, 1.0, 1.0))
            .node(OpKind::SeqScan, "Seq Scan", |n| {
                ran(n, 0.0, 0.05, 1.0, 1.0);
                n.actuals.rows_removed_by_filter = Some(20_000.0);
                n.estimates.rows = Some(1.0);
                on(n, &name, None);
                cond(n, CondKind::Filter, "((status)::text = 'paid'::text)");
            })
            .up();
    }
    let p = b.with(|p| p.statement.execution_ms = Some(500.0)).build();
    assert!(p.nodes.len() >= 10_000);
    let check = Check::postgres("SELECT * FROM orders WHERE status = 'paid'");
    let report = crate::profile::derive(p);
    let statement = check.statement();
    let started = Instant::now();
    let cx = check.context(&report, &statement);
    let found = analyze(&cx);
    let took = started.elapsed();
    assert!(found.iter().filter(|f| f.rule == "seq-scan-selective").count() == 5_000);
    let budget = if cfg!(debug_assertions) { 3_000 } else { 20 };
    assert!(took.as_millis() < budget, "{took:?}");
}
