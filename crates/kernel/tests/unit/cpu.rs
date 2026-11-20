use super::*;

/// Work on this thread counts; waiting doesn't.
#[test]
fn a_thread_s_work_counts_and_its_sleep_does_not() {
    let start = thread_time().expect("the system tells a thread's CPU time");
    let mut x = 0u64;
    let until = std::time::Instant::now() + Duration::from_millis(60);
    while std::time::Instant::now() < until {
        x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
    }
    let worked = thread_time().unwrap() - start;
    assert!(worked >= Duration::from_millis(20), "busy for 60 ms: {worked:?}");
    let before = thread_time().unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let slept = thread_time().unwrap() - before;
    // Almost nothing: Windows counts a thread's time by its clock's ticks (15.6 ms), so a
    // tick or so may show; far less than the work's.
    assert!(slept < Duration::from_millis(32), "asleep: almost nothing ({slept:?})");
    assert!(slept < worked, "asleep less than at work ({slept:?} of {worked:?})");
}
