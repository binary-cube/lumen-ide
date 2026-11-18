use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, planned};

fn with(disabled: bool, cost: f64, settings: &[(&str, &str)]) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .child(OpKind::NestedLoop, "Nested Loop", |n| planned(n, 0.0, cost + 10.0, 50.0))
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            planned(n, 0.0, cost, 50.0);
            n.flags.disabled = disabled;
        })
        .node(OpKind::IndexScan, "Index Scan", |n| planned(n, 0.0, 5.0, 1.0))
        .with(|p| {
            for (name, value) in settings {
                p.statement
                    .settings
                    .push(crate::profile::Setting { name: (*name).into(), value: (*value).into() });
            }
        })
        .build()
}

#[test]
fn a_disabled_step_and_its_setting() {
    let found =
        Check::postgres("SELECT 1").rule(ID, with(true, 100.0, &[("enable_seqscan", "'off'")]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].nodes, [crate::profile::NodeId(1)]);
    assert_eq!(found[0].text_of(E::Setting), Some("enable_seqscan"));
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("SET enable_seqscan = on;")
    );
}

#[test]
fn before_18_by_its_cost_the_deepest_once() {
    let found = Check::postgres("SELECT 1").rule(ID, with(false, 1e10 + 834.0, &[]));
    assert_eq!(found.len(), 1, "the loop above carries the cost too: {found:?}");
    assert_eq!(found[0].nodes, [crate::profile::NodeId(1)]);
    assert_eq!(found[0].text_of(E::Setting), Some("enable_seqscan"));
}

#[test]
fn a_setting_off_alone_and_nothing_off() {
    let found =
        Check::postgres("SELECT 1").rule(ID, with(false, 100.0, &[("enable_hashjoin", "off")]));
    assert_eq!(found[0].variant, Some("setting"));
    assert!(
        Check::postgres("SELECT 1")
            .rule(ID, with(false, 100.0, &[("work_mem", "'64MB'")]))
            .is_empty()
    );
    assert!(Check::postgres("SELECT 1").rule(ID, with(false, 9e9, &[])).is_empty());
}
