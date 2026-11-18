use super::*;
use std::sync::atomic::Ordering;

use power::Keep;

#[test]
fn the_hold_is_taken_once_and_let_go() {
    let blocker = Arc::new(fake::Blocker::default());
    let keeper = Keeper::new(blocker.clone());
    assert!(!keeper.is_held() && keeper.since().is_none());
    keeper.set(Some(Keep::default())).unwrap();
    keeper.set(Some(Keep::default())).unwrap();
    assert_eq!(blocker.live.load(Ordering::Relaxed), 1, "once for the whole app");
    assert!(keeper.is_held() && keeper.since().is_some());
    keeper.set(None).unwrap();
    assert_eq!(blocker.live.load(Ordering::Relaxed), 0, "let go");
    assert!(!keeper.is_held());
}

#[test]
fn a_refusal_holds_nothing() {
    let blocker = Arc::new(fake::Blocker::default());
    blocker.refuse.store(true, Ordering::Relaxed);
    let keeper = Keeper::new(blocker);
    assert!(matches!(keeper.set(Some(Keep::default())), Err(PowerError::Unsupported(_))));
    assert!(!keeper.is_held());
}

/// The screen asked too (or no longer) while held: held again on it, at once, without a
/// moment let go; since when it's held stays.
#[test]
fn the_screen_asked_is_held_again() {
    let blocker = Arc::new(fake::Blocker::default());
    let keeper = Keeper::new(blocker.clone());
    keeper.set(Some(Keep::default())).unwrap();
    let since = keeper.since();
    keeper.set(Some(Keep { screen: true })).unwrap();
    assert!(blocker.screen.load(Ordering::Relaxed));
    assert_eq!(blocker.live.load(Ordering::Relaxed), 1, "the old one let go after");
    assert_eq!(keeper.since(), since);
}
