use super::*;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn the_output_s_clock_reads_as_a_date_and_a_time() {
    assert_eq!(utc_text(UNIX_EPOCH), "1970-01-01 00:00:00.000");
    let at = UNIX_EPOCH + Duration::from_millis(1_793_440_823_698);
    assert_eq!(utc_text(at), "2026-10-31 10:00:23.698");
    let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
    assert_eq!(utc_text(leap), "2000-02-29 00:00:00.000");
}

/// The app's clock is the local time of day: the same shape, its milliseconds kept (the
/// offset is the machine's).
#[test]
fn the_app_s_clock_is_the_local_time() {
    let at = UNIX_EPOCH + Duration::from_millis(1_793_440_823_698);
    let local = clock_text(at);
    assert_eq!(local.len(), "2026-10-31 10:00:23.698".len(), "{local}");
    assert!(local.ends_with(":23.698"), "{local}");
    let offset = chrono::Local::now().offset().local_minus_utc();
    if offset == 0 {
        assert_eq!(local, utc_text(at));
    }
}

/// Each project's consoles run on their own pool: one whose every worker is busy (a
/// project's long queries) never keeps another's job waiting.
#[test]
fn each_runtime_has_its_own_pool() {
    use std::sync::mpsc;
    let (busy, other) = (Runtime::pool(), Runtime::pool());
    let (release, held) = mpsc::channel::<()>();
    let held = std::sync::Arc::new(std::sync::Mutex::new(held));
    for _ in 0..kernel::work::default_limit() + 1 {
        let held = std::sync::Arc::clone(&held);
        busy.spawn(move || {
            let _ = held.lock().map(|h| h.recv_timeout(std::time::Duration::from_secs(10)));
        });
    }
    let (done, ran) = mpsc::channel();
    other.spawn(move || done.send(()).unwrap());
    let ran = ran.recv_timeout(std::time::Duration::from_secs(5));
    for _ in 0..kernel::work::default_limit() + 1 {
        let _ = release.send(());
    }
    assert!(ran.is_ok(), "another project's job waited for this one's pool");
}
