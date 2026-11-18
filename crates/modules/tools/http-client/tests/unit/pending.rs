//! A result made on a worker, taken once by a poll; the app woken when it's ready.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[test]
fn a_result_is_taken_once_and_wakes_the_app() {
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    let wake: Wake = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let mut pending = Pending::default();
    pending.set_waker(&Some(wake));
    assert!(pending.take().is_none(), "nothing started");
    pending.start("test-pending", || 42);
    let started = Instant::now();
    let mut got = None;
    while got.is_none() && started.elapsed() < Duration::from_secs(10) {
        got = pending.take();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(got, Some(42));
    assert!(pending.take().is_none(), "once");
    while woken.load(Ordering::SeqCst) == 0 && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(woken.load(Ordering::SeqCst), 1, "woken when it was ready");
}
