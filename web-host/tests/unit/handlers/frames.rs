use std::time::{Duration, Instant};

use super::{BEHIND_RETRY, Frames, changed_in, may_probe};
use cef::Rect;
use web_engine_protocol::{Changed, MAX_CHANGED};

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect { x, y, width, height }
}

/// While its app is behind, a page hands over its first frame, then one every
/// [`BEHIND_RETRY`]; the ones between are dropped.
#[test]
fn a_page_behind_hands_over_a_frame_now_and_then() {
    let now = Instant::now();
    assert!(may_probe(None, now), "the first");
    assert!(!may_probe(Some(now), now + BEHIND_RETRY / 2), "too soon");
    assert!(may_probe(Some(now), now + BEHIND_RETRY), "its time");
    assert!(may_probe(Some(now), now + BEHIND_RETRY + Duration::from_secs(1)));
}

/// CEF's changed rectangles are cut to the frame; what's outside it is nothing.
#[test]
fn changed_rectangles_are_cut_to_the_frame() {
    let changed = |x, y, width, height| Changed { x, y, width, height };
    assert_eq!(changed_in(&rect(10, 20, 30, 40), 100, 100), Some(changed(10, 20, 30, 40)));
    assert_eq!(changed_in(&rect(-5, 90, 20, 50), 100, 100), Some(changed(0, 90, 15, 10)));
    assert_eq!(changed_in(&rect(200, 0, 10, 10), 100, 100), None);
    assert_eq!(changed_in(&rect(0, 0, 0, 10), 100, 100), None);
    assert_eq!(changed_in(&rect(i32::MAX, 0, i32::MAX, 1), 100, 100), None);
}

/// The first frame of each kind is told whole, the next ones by what changed; past
/// [`MAX_CHANGED`] rectangles, whole again.
#[test]
fn a_frame_tells_what_changed_after_the_first() {
    let mut frames = Frames::default();
    let dirty = [rect(0, 0, 10, 10)];
    assert!(frames.changed(false, Some(&dirty), (100, 100)).is_empty(), "the first: whole");
    assert_eq!(frames.changed(false, Some(&dirty), (100, 100)).len(), 1);
    assert!(frames.changed(true, Some(&dirty), (100, 100)).is_empty(), "the popup's first");
    let many = vec![rect(0, 0, 1, 1); MAX_CHANGED + 1];
    assert!(frames.changed(false, Some(&many), (100, 100)).is_empty(), "too many: whole");
    assert_eq!(frames.changed(false, Some(&dirty), (100, 100)).len(), 1);
    assert_eq!((frames.next(), frames.next()), (1, 2), "numbered in order");
}

/// While the app is behind, frames between the probes are dropped: the next of their kind
/// (only theirs) is told whole, and once the app catches up the page is painted again, once.
#[test]
fn frames_dropped_while_behind_are_made_up_for() {
    let mut frames = Frames::default();
    let dirty = [rect(0, 0, 10, 10)];
    for popup in [false, true] {
        frames.changed(popup, Some(&dirty), (100, 100));
    }
    let now = Instant::now();
    assert!(frames.hands_over(false, false, now), "keeping up: every frame");
    assert!(!frames.caught_up(false), "nothing dropped");
    assert!(frames.hands_over(false, true, now), "behind: the first");
    assert!(!frames.hands_over(true, true, now + BEHIND_RETRY / 2), "too soon: dropped");
    assert_eq!(frames.changed(false, Some(&dirty), (100, 100)).len(), 1, "the view's kept");
    assert!(frames.changed(true, Some(&dirty), (100, 100)).is_empty(), "the popup's whole");
    assert!(frames.hands_over(false, true, now + BEHIND_RETRY), "its time");
    assert!(!frames.caught_up(true), "still behind");
    assert!(frames.caught_up(false), "caught up: painted again");
    assert!(!frames.caught_up(false), "once");
}

/// A frame missed (not handed over, its picture let go) makes the next of its kind whole.
#[test]
fn a_frame_missed_makes_the_next_whole() {
    let mut frames = Frames::default();
    let dirty = [rect(0, 0, 10, 10)];
    frames.changed(false, Some(&dirty), (100, 100));
    frames.missed(false);
    assert!(frames.changed(false, Some(&dirty), (100, 100)).is_empty());
    assert_eq!(frames.changed(false, Some(&dirty), (100, 100)).len(), 1);
}
