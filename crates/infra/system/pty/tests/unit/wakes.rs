use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn the_first_wake_after_quiet_goes_at_once_the_rest_once_at_the_interval_s_end() {
    let mut pace = Pace::default();
    let t0 = Instant::now();
    assert_eq!(pace.ask(t0), Wake::Now);
    // Within the interval: one wake put off to its end, the rest ride on it.
    let ms = Duration::from_millis;
    assert_eq!(pace.ask(t0 + ms(1)), Wake::At(t0 + INTERVAL));
    assert_eq!(pace.ask(t0 + ms(2)), Wake::Coming);
    assert_eq!(pace.ask(t0 + ms(50)), Wake::Coming);
    pace.fired(t0 + INTERVAL);
    // Right after it: put off again; after a quiet interval: at once.
    assert_eq!(pace.ask(t0 + INTERVAL + ms(1)), Wake::At(t0 + INTERVAL * 2));
    pace.fired(t0 + INTERVAL * 2);
    assert_eq!(pace.ask(t0 + INTERVAL * 3), Wake::Now);
}

/// A reader marking the screen changed as fast as it can (a flood of output), a UI taking
/// the change on each wake, as the session's `mark_dirty` / `take_dirty` do: the wakes in
/// `run_for`, paced or not, and whether the last change was taken.
fn flood(paced: bool, run_for: Duration) -> (usize, bool) {
    let dirty = Arc::new(AtomicBool::new(false));
    let wakes = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = channel::<()>();
    let tx = Mutex::new(tx);
    let counted = Arc::clone(&wakes);
    let waker: Waker = Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
        let _ = tx.lock().unwrap().send(());
    });
    let ui_dirty = Arc::clone(&dirty);
    let ui = std::thread::spawn(move || {
        while rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            ui_dirty.swap(false, Ordering::AcqRel);
        }
    });
    let gate = Wakes::new(Arc::clone(&waker));
    let mark = || {
        if !dirty.swap(true, Ordering::AcqRel) {
            if paced { gate.wake() } else { waker() }
        }
    };
    let end = Instant::now() + run_for;
    while Instant::now() < end {
        mark();
        std::hint::spin_loop();
    }
    // The last chunk, then quiet: it must be taken.
    mark();
    let deadline = Instant::now() + Duration::from_millis(500);
    while dirty.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    let taken = !dirty.load(Ordering::Acquire);
    let count = wakes.load(Ordering::Relaxed);
    drop(gate);
    drop(waker);
    ui.join().unwrap();
    (count, taken)
}

/// A flood of output wakes the UI at most once per interval (about 125 a second), not at
/// the rate the UI takes the change (tens of thousands a second unpaced).
#[test]
fn a_flood_of_output_wakes_the_ui_at_most_once_per_interval() {
    let run_for = Duration::from_millis(500);
    let (unpaced, _) = flood(false, run_for);
    let (paced, taken) = flood(true, run_for);
    let per_second = |n: usize| n as f64 / run_for.as_secs_f64();
    eprintln!(
        "wakes per second: unpaced {:.0}, paced {:.0}",
        per_second(unpaced),
        per_second(paced)
    );
    assert!(taken, "the last output wasn't shown");
    let most = (run_for.as_millis() / INTERVAL.as_millis()) as usize + 3;
    assert!(paced <= most, "{paced} wakes in {run_for:?}");
    assert!(paced >= 10, "{paced} wakes in {run_for:?}: the screen should follow the output");
    assert!(unpaced > paced, "unpaced {unpaced}, paced {paced}");
}

/// The output that comes right after a wake (within the interval) is still told: by the
/// wake put off to the interval's end.
#[test]
fn the_last_output_always_wakes() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let gate = Wakes::new(Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));
    gate.wake();
    assert_eq!(wakes.load(Ordering::Relaxed), 1, "the first goes at once");
    gate.wake();
    gate.wake();
    let deadline = Instant::now() + Duration::from_millis(500);
    while wakes.load(Ordering::Relaxed) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wakes.load(Ordering::Relaxed), 2, "one wake for both, at the interval's end");
    std::thread::sleep(INTERVAL * 3);
    assert_eq!(wakes.load(Ordering::Relaxed), 2, "no more than one");
    // Quiet since: at once again.
    gate.wake();
    assert_eq!(wakes.load(Ordering::Relaxed), 3);
}
