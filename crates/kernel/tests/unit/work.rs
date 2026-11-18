use super::*;
use std::sync::mpsc;
use std::time::Instant;

const SHORT: Duration = Duration::from_millis(200);

#[test]
fn jobs_run_on_at_most_the_limit_of_threads() {
    let pool = Pool::new("test-worker", 2, SHORT);
    let (tx, rx) = mpsc::channel();
    let running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let most = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for _ in 0..8 {
        let (tx, running, most) = (tx.clone(), Arc::clone(&running), Arc::clone(&most));
        pool.spawn(Priority::Background, None, move || {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            running.fetch_sub(1, Ordering::SeqCst);
            let _ = tx.send(());
        });
    }
    for _ in 0..8 {
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    assert!(most.load(Ordering::SeqCst) <= 2);
    assert!(pool.workers() <= 2);
}

#[test]
fn the_interactive_jobs_go_first() {
    let pool = Pool::new("test-worker", 1, SHORT);
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel();
    // The only worker busy until the queue is filled.
    pool.spawn(Priority::Background, None, move || {
        let _ = gate_rx.recv();
    });
    for (priority, name) in [
        (Priority::Background, "index"),
        (Priority::Interactive, "blame"),
        (Priority::Background, "prefetch"),
        (Priority::Interactive, "command"),
    ] {
        let tx = tx.clone();
        pool.spawn(priority, None, move || {
            let _ = tx.send(name);
        });
    }
    gate_tx.send(()).unwrap();
    let order: Vec<&str> =
        (0..4).map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
    assert_eq!(order, ["blame", "command", "index", "prefetch"]);
}

#[test]
fn a_job_cancelled_before_it_starts_does_not_run() {
    let pool = Pool::new("test-worker", 1, SHORT);
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel();
    pool.spawn(Priority::Background, None, move || {
        let _ = gate_rx.recv();
    });
    let cancel = Cancel::new();
    let ran = tx.clone();
    pool.spawn(Priority::Background, Some(&cancel), move || {
        let _ = ran.send("cancelled");
    });
    pool.spawn(Priority::Background, None, move || {
        let _ = tx.send("kept");
    });
    cancel.cancel();
    gate_tx.send(()).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "kept");
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
}

#[test]
fn a_panicking_job_does_not_stop_the_pool() {
    let pool = Pool::new("test-worker", 1, SHORT);
    pool.spawn(Priority::Background, None, || panic!("a job's bug"));
    let (tx, rx) = mpsc::channel();
    pool.spawn(Priority::Background, None, move || {
        let _ = tx.send(());
    });
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
fn idle_workers_end() {
    let pool = Pool::new("test-worker", 2, Duration::from_millis(30));
    let (tx, rx) = mpsc::channel();
    pool.spawn(Priority::Background, None, move || {
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let started = Instant::now();
    while pool.workers() > 0 && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(pool.workers(), 0, "no thread kept without work");
}

/// One-off jobs by name share that name's pool: run on its threads, never more than its
/// limit at once.
#[test]
fn named_jobs_share_their_pool() {
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..20 {
        let tx = tx.clone();
        run("test-named", move || {
            let name = std::thread::current().name().map(str::to_owned);
            let _ = tx.send((i, name));
        });
    }
    drop(tx);
    let got: Vec<(i32, Option<String>)> = rx.iter().collect();
    assert_eq!(got.len(), 20);
    assert!(got.iter().all(|(_, n)| n.as_deref() == Some("test-named")));
}

#[test]
fn the_sooner_of_two_waits_does_the_work() {
    let (tx, rx) = mpsc::channel();
    let mut slot = None;
    let later = tx.clone();
    Later::sooner(&mut slot, "test-later", Duration::from_secs(60), move || {
        let _ = later.send("late");
    });
    let late = slot.as_ref().map(Later::due);
    // A sooner reason takes the place of the later one (which is cancelled).
    let soon = tx.clone();
    Later::sooner(&mut slot, "test-later", Duration::from_millis(10), move || {
        let _ = soon.send("soon");
    });
    assert!(slot.as_ref().map(Later::due) < late);
    // A later one leaves the sooner wait as it is.
    Later::sooner(&mut slot, "test-later", Duration::from_secs(60), move || {
        let _ = tx.send("later again");
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok("soon"));
    assert!(rx.recv_timeout(SHORT).is_err(), "only the sooner one ran");
}
