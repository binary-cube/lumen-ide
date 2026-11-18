use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{OpKind, Parallel};
use crate::profile::testing::{Check, ProfileBuilder, ran};

fn gather(planned: u32, launched: Option<u32>, depth: ProfileDepth) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", depth)
        .child(OpKind::Gather, "Gather", |n| {
            ran(n, 0.0, 60.0, 3.0, 1.0);
            n.parallel = Some(Parallel { planned: Some(planned), launched, ..Parallel::default() });
        })
        .node(OpKind::SeqScan, "Parallel Seq Scan", |n| ran(n, 0.0, 50.0, 1000.0, 1.0))
        .build()
}

#[test]
fn fewer_workers_than_planned() {
    let found = Check::postgres("SELECT 1")
        .rule(ID, gather(4, Some(1), ProfileDepth::Actual { timing: true }));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].quantity(E::Planned).unwrap().value, 4.0);
    assert_eq!(found[0].quantity(E::Launched).unwrap().value, 1.0);
}

#[test]
fn all_launched_or_not_run_is_fine() {
    let act = ProfileDepth::Actual { timing: true };
    assert!(Check::postgres("SELECT 1").rule(ID, gather(2, Some(2), act)).is_empty());
    assert!(Check::postgres("SELECT 1").rule(ID, gather(2, None, act)).is_empty(), "unknown");
    assert!(
        Check::postgres("SELECT 1")
            .rule(ID, gather(2, Some(0), ProfileDepth::Estimated))
            .is_empty()
    );
}
