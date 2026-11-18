use super::*;
use crate::profile::node::OpKind;
use crate::profile::testing::{Check, ProfileBuilder};
use crate::profile::{ProfileDepth, ProfileNote};

fn profile(missing: bool) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "Seq Scan", |_| {})
        .with(|p| {
            if missing {
                p.note(ProfileNote::NoStatementStats);
            }
        })
        .build()
}

#[test]
fn a_server_without_its_history_is_said_once() {
    let found = Check::postgres("SELECT 1").rule(ID, profile(true));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Info);
    assert!(found[0].suggestion.is_none(), "nothing installed by us");
    assert!(found[0].is_problem());
}

#[test]
fn nothing_when_it_has_one_or_none_was_asked() {
    assert!(Check::postgres("SELECT 1").rule(ID, profile(false)).is_empty());
}
