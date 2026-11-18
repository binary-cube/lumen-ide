use database::profile::testing::ProfileBuilder;
use database::profile::{OpKind, ProfileDepth, derive};

use super::*;

fn stored(name: &str) -> StoredProfile {
    let profile = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, name, |_| {})
        .build();
    StoredProfile::new(Arc::new(derive(profile)), format!("SELECT * FROM {name}"), 0..10)
}

#[test]
fn the_oldest_unpinned_go_first_and_the_newest_stays() {
    let mut profiles = Profiles::default();
    let first = stored("a");
    let first_id = first.id();
    profiles.push(first, 2, usize::MAX);
    profiles.pin(first_id, true);
    let second = stored("b");
    let second_id = second.id();
    profiles.push(second, 2, usize::MAX);
    profiles.push(stored("c"), 2, usize::MAX);
    assert_eq!(profiles.len(), 2);
    assert!(profiles.get(first_id).is_some(), "pinned: kept");
    assert!(profiles.get(second_id).is_none(), "the oldest unpinned went");
    assert_eq!(profiles.latest().map(|p| p.report.profile.nodes[0].name.as_str()), Some("c"));
    // Past the memory: down to the pinned and the newest, never below.
    let one = profiles.latest().map(|p| p.bytes).unwrap_or(0);
    profiles.push(stored("d"), 20, one);
    assert_eq!(
        profiles.iter().map(|p| p.report.profile.nodes[0].name.as_str()).collect::<Vec<_>>(),
        ["a", "d"]
    );
    assert!(profiles.bytes() > one, "the pinned and the newest stay past the limit");
    let generation = profiles.generation;
    profiles.purge();
    assert_eq!(profiles.len(), 1, "the caches cleared: only the pinned");
    assert!(profiles.generation > generation);
    profiles.remove(first_id);
    assert!(profiles.is_empty());
}
