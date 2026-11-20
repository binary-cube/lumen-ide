use super::*;

const MS: Duration = Duration::from_millis(1);

fn queue() -> Coalescing<&'static str> {
    Coalescing::new(300 * MS, 2000 * MS, 60_000 * MS)
}

#[test]
fn a_key_already_waiting_is_asked_once() {
    let t = Instant::now();
    let mut q = queue();
    assert!(q.push("a.php", t));
    assert!(!q.push("a.php", t + MS), "already waiting");
    assert!(q.push("b.php", t + 2 * MS));
    assert!(!q.push("a.php", t + 3 * MS));
    assert_eq!(q.len(), 2);
    assert_eq!(q.take(t + 303 * MS), Some(Batch::Keys(vec!["a.php", "b.php"])));
    assert_eq!(q.stats(), QueueStats { received: 4, merged: 2, batches: 1 });
    // Handed out: asked again, it waits again.
    assert!(q.push("a.php", t + 400 * MS));
}

#[test]
fn everything_swallows_the_keys() {
    let t = Instant::now();
    let mut q = queue();
    q.push("a.php", t);
    q.push("b.php", t);
    q.push_all(t);
    assert!(!q.push("c.php", t), "everything is asked for already");
    q.push_all(t);
    assert_eq!(q.take(t + 300 * MS), Some(Batch::All));
    assert_eq!(q.stats().merged, 4, "a, b swallowed; c and the second everything merged");
    assert!(q.is_empty() && q.take(t + 10_000 * MS).is_none());
}

#[test]
fn it_waits_for_quiet_but_not_forever() {
    let t = Instant::now();
    let mut q = queue();
    q.push("a.php", t);
    assert_eq!(q.ready_at(t), Some(t + 300 * MS));
    assert!(q.take(t + 299 * MS).is_none(), "still asked for recently");
    // Requests keep coming every 200 ms: the quiet never comes, the limit does.
    for i in 1..20 {
        q.push("a.php", t + i * 200 * MS);
    }
    assert_eq!(q.ready_at(t + 3000 * MS), Some(t + 2000 * MS));
    assert!(q.take(t + 1999 * MS).is_none());
    assert_eq!(q.take(t + 2000 * MS), Some(Batch::Keys(vec!["a.php"])));
}

#[test]
fn an_open_batch_holds_everything_until_it_ends() {
    let t = Instant::now();
    let mut q = queue();
    q.hold(7, t);
    for i in 0..1000 {
        q.push(if i % 2 == 0 { "a.php" } else { "b.php" }, t + i * 10 * MS);
    }
    // Way past the quiet and the limit: held.
    assert!(q.take(t + 20_000 * MS).is_none());
    assert!(q.ready_at(t + 20_000 * MS) > Some(t + 20_000 * MS));
    // Another batch's end changes nothing; this one's ends it: one batch, after the quiet.
    q.release(8, t + 20_000 * MS);
    assert!(q.take(t + 20_000 * MS).is_none());
    q.release(7, t + 20_000 * MS);
    assert!(q.take(t + 20_100 * MS).is_none(), "the quiet after it");
    assert_eq!(q.take(t + 20_300 * MS), Some(Batch::Keys(vec!["a.php", "b.php"])));
    assert_eq!(q.stats().batches, 1);
}

#[test]
fn a_batch_never_ended_lets_go_after_its_limit() {
    let t = Instant::now();
    let mut q = Coalescing::new(300 * MS, 2000 * MS, 5000 * MS);
    q.hold(1, t);
    q.push("a.php", t);
    assert!(q.take(t + 4999 * MS).is_none());
    assert_eq!(q.ready_at(t + 1000 * MS), Some(t + 5000 * MS));
    assert_eq!(q.take(t + 5000 * MS), Some(Batch::Keys(vec!["a.php"])));
}

#[test]
fn a_worker_sleeps_until_a_batch_is_ready_and_ends_when_closed() {
    let q: SharedQueue<String> = SharedQueue::new(20 * MS, 200 * MS, 60_000 * MS);
    let worker = q.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        while let Some(batch) = worker.next() {
            tx.send(batch).unwrap();
        }
    });
    q.push_many(["a".to_owned(), "b".to_owned(), "a".to_owned()]);
    let batch = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(batch, Batch::Keys(vec!["a".to_owned(), "b".to_owned()]));
    q.hold(3);
    q.push("c".to_owned());
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "held");
    q.release(3);
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Batch::Keys(vec!["c".to_owned()]));
    q.close();
    thread.join().unwrap();
    assert_eq!(q.stats().batches, 2);
}

#[test]
fn an_urgent_request_goes_at_once_with_what_waits_unless_held() {
    let t = Instant::now();
    let mut q = queue();
    q.push("a.php", t);
    q.push_urgent("b.php", t + MS);
    assert_eq!(q.take(t + MS), Some(Batch::Keys(vec!["a.php", "b.php"])));
    // The next ones wait for quiet again.
    q.push("c.php", t + 2 * MS);
    assert!(q.take(t + 3 * MS).is_none());
    // Held: even urgent waits for the batch's end.
    q.hold(9, t + 4 * MS);
    q.push_urgent("d.php", t + 5 * MS);
    assert!(q.take(t + 6 * MS).is_none());
    q.release(9, t + 7 * MS);
    assert_eq!(q.take(t + 7 * MS), Some(Batch::Keys(vec!["c.php", "d.php"])));
}

/// The latest job per key runs (a key asked again while waiting: once, the newest); one
/// that panics is skipped, the queue goes on; the UI is woken when results are ready.
#[test]
fn the_latest_job_per_key_runs() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&woken);
    let (gate_tx, gate) = std::sync::mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let (started_tx, started) = std::sync::mpsc::channel::<()>();
    let started_tx = Mutex::new(started_tx);
    let latest = Latest::new(
        "test-latest",
        move |(key, version): (u8, u32)| {
            // The first job says it began, then waits until the others are asked.
            if version == 0 {
                let _ = started_tx.lock().unwrap().send(());
                let _ = gate.lock().unwrap().recv();
            }
            assert!(key != 9, "a job that panics");
            (key, version)
        },
        Arc::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    latest.request(0, (0, 0));
    // The worker busy with it before the others are asked: they wait together.
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    latest.request(1, (1, 1));
    latest.request(1, (1, 2));
    latest.request(9, (9, 1));
    latest.request(2, (2, 1));
    gate_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut done = Vec::new();
    while done.len() < 3 && Instant::now() < deadline {
        done.extend(latest.take());
        std::thread::sleep(Duration::from_millis(5));
    }
    done.sort_unstable();
    assert_eq!(done, [(0, 0), (1, 2), (2, 1)]);
    assert!(woken.load(std::sync::atomic::Ordering::Relaxed) > 0);
}

#[test]
fn a_forced_request_goes_even_during_a_batch() {
    let t = Instant::now();
    let mut q = queue();
    q.hold(9, t);
    q.push("a.php", t + MS);
    q.push_forced("b.php", t + 2 * MS);
    // Only what the user asked for: the batch's own (a paste still writing) stays held.
    assert_eq!(q.take(t + 2 * MS), Some(Batch::Keys(vec!["b.php"])));
    // The batch still holds the next ones.
    q.push("c.php", t + 3 * MS);
    assert!(q.take(t + 4 * MS).is_none());
    // Ended: the rest goes after its quiet, once.
    q.release(9, t + 5 * MS);
    assert!(q.take(t + 6 * MS).is_none());
    assert_eq!(
        q.take(t + 5 * MS + Duration::from_secs(5)),
        Some(Batch::Keys(vec!["a.php", "c.php"]))
    );
}

/// Outside a batch, a forced key takes what's waiting with it (one trip, as an urgent one).
#[test]
fn a_forced_request_outside_a_batch_takes_what_waits() {
    let t = Instant::now();
    let mut q = queue();
    q.push("a.php", t);
    q.push_forced("b.php", t + MS);
    assert_eq!(q.take(t + MS), Some(Batch::Keys(vec!["a.php", "b.php"])));
    assert!(q.is_empty());
}
