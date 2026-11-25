use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::{ACTIVE_FOR, Pump};

/// A pump counting its wakes.
fn counted() -> (Pump, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let woken = Arc::clone(&wakes);
    let pump = Pump::unregistered(move || {
        woken.fetch_add(1, Ordering::SeqCst);
    });
    (pump, wakes)
}

/// What's due at the start is taken once; then nothing is due until asked again.
#[test]
fn the_work_due_is_taken_once() {
    let (pump, _) = counted();
    assert!(pump.take_due(), "due as the pump starts");
    assert!(!pump.take_due(), "taken");
    assert_eq!(pump.until_due(), Duration::MAX, "nothing scheduled");
    pump.ask(0);
    assert!(pump.take_due());
}

/// A delay keeps the work until it's due; the sooner of two asked wins.
#[test]
fn the_sooner_of_what_was_asked_is_kept() {
    let (pump, _) = counted();
    pump.take_due();
    pump.ask(60_000);
    assert!(!pump.take_due(), "not due yet");
    assert!(pump.until_due() > Duration::from_secs(50));
    pump.ask(1);
    assert!(pump.until_due() <= Duration::from_millis(1));
    pump.ask(60_000);
    assert!(pump.until_due() <= Duration::from_millis(1), "a later one changes nothing");
    std::thread::sleep(Duration::from_millis(2));
    assert!(pump.take_due());
}

/// A delay too long to tell (CEF's `i64` of milliseconds) is no overflow: as good as never.
#[test]
fn a_huge_delay_is_never_an_overflow() {
    let (pump, _) = counted();
    pump.take_due();
    pump.ask(i64::MAX);
    assert!(!pump.take_due());
    assert!(pump.until_due() > Duration::from_secs(60 * 60 * 24 * 365));
    pump.ask(i64::MIN);
    assert!(pump.take_due(), "a negative delay: now");
}

/// From another thread, a schedule sooner than the one the loop sleeps for wakes it — a
/// delayed one too; one no sooner doesn't. On the loop's own thread nothing is woken (it
/// looks again before it sleeps).
#[test]
fn a_sooner_schedule_from_another_thread_wakes_the_loop() {
    let (pump, wakes) = counted();
    pump.take_due();
    pump.ask(0);
    assert_eq!(wakes.load(Ordering::SeqCst), 0, "the loop's own thread");
    pump.take_due();
    std::thread::spawn(move || {
        pump.ask(50);
        pump.ask(100);
        pump.ask(10);
    })
    .join()
    .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 2, "the 50 ms one, then the sooner 10 ms one");
}

/// Something that happened keeps the quick pace for a while.
#[test]
fn a_page_touched_is_active_for_a_while() {
    let (pump, _) = counted();
    assert!(pump.active(), "touched as it starts");
    pump.mark();
    assert!(pump.active());
    assert!(ACTIVE_FOR > Duration::from_millis(100));
}
