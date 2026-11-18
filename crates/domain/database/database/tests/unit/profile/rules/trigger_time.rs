use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{OpKind, WriteKind};
use crate::profile::testing::{Check, ProfileBuilder};

fn write(triggers: &[(&str, f64)], execution: f64) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::ModifyTable(WriteKind::Update), "Update", |_| {})
        .with(|p| {
            p.statement.execution_ms = Some(execution);
            for (name, ms) in triggers {
                p.statement.triggers.push(crate::profile::TriggerTime {
                    name: (*name).into(),
                    relation: Some("accounts".into()),
                    calls: Some(10),
                    ms: Some(*ms),
                });
            }
        })
        .build()
}

#[test]
fn triggers_taking_much_of_a_write() {
    let found =
        Check::postgres("SELECT 1").rule(ID, write(&[("audit", 20.0), ("touch", 5.0)], 60.0));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text_of(E::Trigger), Some("audit"), "the slowest named");
    assert_eq!(found[0].quantity(E::Time).unwrap().value, 20.0);
    assert_eq!(
        Check::postgres("SELECT 1").rule(ID, write(&[("audit", 30.0)], 100.0)).len(),
        1,
        "30 %"
    );
}

#[test]
fn quick_triggers_are_fine() {
    // The 13-wal-update fixture's: 0.041 ms of 0.172 (24 %).
    assert!(
        Check::postgres("SELECT 1").rule(ID, write(&[("accounts_audit", 0.041)], 0.172)).is_empty()
    );
    assert!(Check::postgres("SELECT 1").rule(ID, write(&[("audit", 2.0)], 100.0)).is_empty());
    assert!(Check::postgres("SELECT 1").rule(ID, write(&[], 100.0)).is_empty());
}
