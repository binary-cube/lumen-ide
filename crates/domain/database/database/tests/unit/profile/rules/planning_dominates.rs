use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::testing::{Check, ProfileBuilder};

fn timed(planning: f64, execution: f64) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::Append, "Append", |_| {})
        .with(|p| {
            p.statement.planning_ms = Some(planning);
            p.statement.execution_ms = Some(execution);
        })
        .build()
}

#[test]
fn planning_longer_than_running() {
    let found = Check::postgres("SELECT 1").rule(ID, timed(45.0, 3.0));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Info);
    assert_eq!(found[0].quantity(E::Planning).unwrap().value, 45.0);
    assert_eq!(Check::postgres("SELECT 1").rule(ID, timed(10.0, 10.0)).len(), 1, "at the limits");
}

#[test]
fn a_quick_plan_is_fine() {
    assert!(Check::postgres("SELECT 1").rule(ID, timed(0.2, 0.1)).is_empty(), "under 10 ms");
    assert!(Check::postgres("SELECT 1").rule(ID, timed(12.0, 300.0)).is_empty());
}
