use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What's told the app (from any thread) wakes it and is taken once, in order; the probe
/// answers once set. (One test: the state is the process's.)
#[test]
fn posts_wake_and_are_taken_once_and_the_probe_answers() {
    assert!(!busy(), "no probe yet: nothing runs");
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    set_waker(Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    }));
    let shown = Notification::info("kept awake");
    post(shown.clone());
    assert_eq!(woken.load(Ordering::Relaxed), 1);
    let progress = Notification::progress("downloading");
    post_update(progress.clone());
    post_dismiss(progress.id);
    assert_eq!(woken.load(Ordering::Relaxed), 3, "each wakes the app");
    let taken = take_told();
    assert_eq!(
        taken,
        [Told::Show(shown.clone()), Told::Update(progress.clone()), Told::Dismiss(progress.id)],
        "in order"
    );
    assert_eq!(
        taken.iter().map(Told::id).collect::<Vec<_>>(),
        [shown.id, progress.id, progress.id]
    );
    assert!(take_told().is_empty(), "taken once");
    set_busy_probe(Arc::new(|| true));
    assert!(busy());
}
