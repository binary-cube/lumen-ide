use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A notification posted (from any thread) wakes the app and is taken once; the probe
/// answers once set. (One test: the state is the process's.)
#[test]
fn posts_wake_and_are_taken_once_and_the_probe_answers() {
    assert!(!busy(), "no probe yet: nothing runs");
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    set_waker(Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    }));
    post(Notification::info("kept awake"));
    assert_eq!(woken.load(Ordering::Relaxed), 1);
    let taken = take_posted();
    assert_eq!(taken.len(), 1);
    assert!(take_posted().is_empty(), "taken once");
    set_busy_probe(Arc::new(|| true));
    assert!(busy());
}
