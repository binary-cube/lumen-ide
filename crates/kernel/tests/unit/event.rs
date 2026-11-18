use super::*;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, PartialEq)]
struct Saved(u32);
impl DomainEvent for Saved {}

#[derive(Debug)]
struct Closed;
impl DomainEvent for Closed {}

#[test]
fn delivers_only_to_matching_type() {
    let bus = EventBus::new();
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let _sub = bus.subscribe(move |e: &Saved| sink.lock().unwrap().push(e.0));

    bus.publish(&Saved(7));
    bus.publish(&Closed);

    assert_eq!(*received.lock().unwrap(), vec![7]);
}

#[test]
fn dropping_subscription_unsubscribes() {
    let bus = EventBus::new();
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    let sub = bus.subscribe(move |_: &Closed| {
        c.fetch_add(1, Ordering::Relaxed);
    });

    bus.publish(&Closed);
    drop(sub);
    bus.publish(&Closed);

    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn handler_can_publish_without_deadlock() {
    let bus = EventBus::new();
    let inner = bus.clone();
    let _a = bus.subscribe(move |_: &Closed| inner.publish(&Saved(1)));
    let got = Arc::new(AtomicUsize::new(0));
    let g = Arc::clone(&got);
    let _b = bus.subscribe(move |_: &Saved| {
        g.fetch_add(1, Ordering::Relaxed);
    });

    bus.publish(&Closed);

    assert_eq!(got.load(Ordering::Relaxed), 1);
}

#[test]
fn subscription_outliving_bus_is_harmless() {
    let bus = EventBus::new();
    let sub = bus.subscribe(|_: &Closed| {});
    drop(bus);
    drop(sub);
}

#[test]
fn an_end_guard_runs_once_when_dropped() {
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&ran);
    let guard = Subscription::on_end(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    assert_eq!(ran.load(std::sync::atomic::Ordering::Relaxed), 0);
    drop(guard);
    assert_eq!(ran.load(std::sync::atomic::Ordering::Relaxed), 1);
}
