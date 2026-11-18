use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder};

fn jit(total: f64, execution: f64) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |_| {})
        .with(|p| {
            p.statement.jit =
                Some(crate::profile::Jit { total_ms: Some(total), ..Default::default() });
            p.statement.execution_ms = Some(execution);
        })
        .build()
}

#[test]
fn compiling_took_most_of_the_time() {
    let found = Check::postgres("SELECT 1").rule(ID, jit(180.0, 240.0));
    assert_eq!(found.len(), 1);
    assert!((found[0].quantity(E::Share).unwrap().value - 75.0).abs() < 1e-9);
    assert_eq!(found[0].suggestion.as_ref().and_then(Suggestion::sql), Some("SET jit = off;"));
    // At the limits: 50 ms and 30 %.
    assert_eq!(Check::postgres("SELECT 1").rule(ID, jit(50.0, 166.0)).len(), 1);
}

#[test]
fn a_small_or_short_jit_is_fine() {
    // The 11-jit fixture's: 6.75 ms of 146 ms.
    assert!(Check::postgres("SELECT 1").rule(ID, jit(6.751, 146.123)).is_empty());
    assert!(Check::postgres("SELECT 1").rule(ID, jit(49.0, 60.0)).is_empty(), "under 50 ms");
    assert!(Check::postgres("SELECT 1").rule(ID, jit(60.0, 1_000.0)).is_empty(), "6 %");
}
