use std::time::Duration;

use super::*;

/// Tinted, a tab fades in and holds; tinted again in the same tone, nothing changes; let go,
/// it fades out and is dropped — frames only while it changes.
#[test]
fn a_tint_fades_in_holds_and_fades_out_then_is_dropped() {
    let mut tints = TabTints::default();
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let (guide, other) = (kernel::id::new_uuid(), kernel::id::new_uuid());
    assert!(tints.set(guide, Some(TabTone::Accent), start));
    assert!(!tints.set(guide, Some(TabTone::Accent), at(10)), "the same: nothing new");
    assert!(tints.animating(at(10)));
    assert_eq!(tints.of(other, at(10)), None, "another tab (another identity): untouched");
    let still = at(5_000);
    assert_eq!(tints.of(guide, still), Some((TabTone::Accent, 1.0)));
    assert!(!tints.animating(still), "holding: no frames");
    assert!(tints.set(guide, None, still));
    assert!(!tints.set(guide, None, still), "let go once");
    assert!(tints.animating(still + Duration::from_millis(10)));
    let gone = still + Duration::from_secs(1);
    assert_eq!(tints.of(guide, gone), None);
    assert!(!tints.animating(gone));
    tints.purge(gone, |_| true);
    assert!(tints.0.is_empty(), "dropped");
    // A tab no longer open: its tint goes too, faded or not.
    assert!(tints.set(other, Some(TabTone::Warning), gone));
    tints.purge(gone, |_| false);
    assert!(tints.0.is_empty(), "its tab gone: dropped");
}
