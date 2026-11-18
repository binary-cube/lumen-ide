use super::*;

#[test]
fn small_fluctuations_are_ignored() {
    let a = Sample { cpu: 0.2, ram_bytes: 80 << 20, ..Sample::default() };
    assert!(!significant(
        &a,
        &Sample { cpu: 0.9, ram_bytes: (80 << 20) + (1 << 20), ..Sample::default() }
    ));
    assert!(significant(&a, &Sample { cpu: 1.5, ram_bytes: 80 << 20, ..Sample::default() }));
    assert!(significant(&a, &Sample { cpu: 0.2, ram_bytes: 83 << 20, ..Sample::default() }));
}

#[test]
fn measures_current_process() {
    let mut system = System::new();
    let pid = sysinfo::get_current_pid().unwrap();
    let sample = measure(&mut system, pid).unwrap();
    assert!(sample.ram_bytes > 1024 * 1024);
}

/// A monitor measuring every few milliseconds, counting its looks.
fn counted() -> (ResourceMonitor, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let looks = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&looks);
    let monitor = ResourceMonitor::spawn(Arc::new(|| {}), Duration::from_millis(5), move || {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        // Each look a different figure: every one is shown.
        Some(Sample { cpu: n as f32 * 2.0, ram_bytes: 0, ..Sample::default() })
    })
    .unwrap();
    (monitor, looks)
}

fn looks_during(looks: &std::sync::atomic::AtomicUsize, wait: Duration) -> usize {
    let before = looks.load(std::sync::atomic::Ordering::SeqCst);
    std::thread::sleep(wait);
    looks.load(std::sync::atomic::Ordering::SeqCst) - before
}

#[test]
fn nothing_is_measured_while_every_window_is_hidden() {
    let (mut monitor, looks) = counted();
    assert!(looks_during(&looks, Duration::from_millis(60)) > 0);
    monitor.set_visible(false);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(looks_during(&looks, Duration::from_millis(80)), 0);
    // Shown again: measured at once, then at its pace.
    monitor.set_visible(true);
    assert!(looks_during(&looks, Duration::from_millis(60)) > 0);
    assert!(monitor.latest().is_some());
}

#[test]
fn the_thread_ends_with_the_last_window_even_hidden() {
    let (mut monitor, looks) = counted();
    let shared = Arc::downgrade(&monitor.shared);
    monitor.set_visible(false);
    drop(monitor);
    std::thread::sleep(Duration::from_millis(50));
    // The thread let its share go: nothing holds the monitor any more.
    assert!(shared.upgrade().is_none());
    assert_eq!(looks_during(&looks, Duration::from_millis(30)), 0);
}

/// The programs under the app: every descendant once (a cycle in a stale table counted
/// once), the app itself not.
#[test]
fn the_programs_under_the_app_are_counted_once() {
    let pid = Pid::from_u32;
    let mut children = std::collections::HashMap::new();
    children.insert(pid(1), vec![pid(2), pid(3)]);
    children.insert(pid(2), vec![pid(4)]);
    children.insert(pid(4), vec![pid(2)]);
    let (count, bytes) = descendants(&children, pid(1), |p| u64::from(p.as_u32()) * 100);
    assert_eq!((count, bytes), (3, 900));
    assert_eq!(descendants(&children, pid(9), |_| 1), (0, 0), "none");
    // A change in what they hold is a change to show.
    let a = Sample { programs: 3, programs_bytes: 10 << 20, ..Sample::default() };
    assert!(significant(&a, &Sample { programs: 4, ..a }));
    assert!(!significant(&a, &Sample { programs_bytes: (10 << 20) + 1, ..a }));
}
