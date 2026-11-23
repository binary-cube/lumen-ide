//! The transitions: a tween moves toward its target and settles there, retargets from where
//! it is (no jump), asks for frames only while it moves; an appearance is whole before it
//! ever opened, starts from nothing when it does.

use std::time::Duration;

use super::*;

/// Long enough that a test's own time doesn't end it.
const LONG: Duration = Duration::from_secs(60);
/// A transition over almost at once, and a wait well past it.
const QUICK: Duration = Duration::from_millis(1);
const PAST: Duration = Duration::from_millis(20);

#[test]
fn a_tween_moves_toward_its_target_and_settles() {
    let mut t = Tween::new(0.0, LONG);
    assert!(!t.animating(), "at rest: no frames");
    assert!(t.retarget(1.0));
    assert!(!t.retarget(1.0), "the same target: nothing new");
    assert!(t.animating());
    assert!(t.value() < 0.1, "barely started");
    assert_eq!(t.target(), 1.0);
    t.set(0.5);
    assert_eq!(t.value(), 0.5, "set: no transition");
    assert!(!t.animating());
    let mut quick = Tween::new(0.0, QUICK);
    quick.retarget(1.0);
    std::thread::sleep(PAST);
    assert_eq!(quick.value(), 1.0, "its time past: there");
    assert!(!quick.animating());
}

/// Turned back halfway, it starts from where it is, not from either end.
#[test]
fn a_tween_retargets_from_where_it_is() {
    let mut t = Tween::new(0.0, LONG);
    t.set(0.4);
    t.retarget(1.0);
    let at = t.value();
    t.retarget(0.0);
    assert!((t.value() - at).abs() < 0.01, "no jump");
}

#[test]
fn an_appearance_is_whole_until_it_opens() {
    let mut a = Appear::default();
    assert_eq!(a.value(), 1.0);
    assert!(!a.animating());
    a.start(LONG);
    assert!(a.value() < 0.1);
    assert!(a.animating());
    a.start(QUICK);
    std::thread::sleep(PAST);
    assert_eq!(a.value(), 1.0);
    assert!(!a.animating());
}
