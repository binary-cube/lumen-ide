use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
struct Fake(&'static str, AtomicU64);

impl ProfileSource for Fake {
    fn owner(&self) -> &'static str {
        self.0
    }

    fn read(&self) -> Vec<Reading> {
        vec![Reading::new("waiting", ReadingKind::Level, self.1.load(Ordering::Relaxed))]
    }

    fn started(&self) {
        self.1.store(7, Ordering::Relaxed);
    }
}

/// A source registered is found (one of the same owner replaced), told when a recording
/// starts, read; removed, gone.
#[test]
fn sources_register_by_owner_and_are_read() {
    register(Arc::new(Fake("tests.one", AtomicU64::new(0))));
    register(Arc::new(Fake("tests.one", AtomicU64::new(1))));
    let found: Vec<_> = sources().into_iter().filter(|s| s.owner() == "tests.one").collect();
    assert_eq!(found.len(), 1, "replaced, not added");
    assert_eq!(found[0].read()[0].value, 1);
    found[0].started();
    assert_eq!(found[0].read(), [Reading::new("waiting", ReadingKind::Level, 7)]);
    assert_eq!(ReadingKind::Micros.unit(), "us");
    unregister("tests.one");
    assert!(sources().iter().all(|s| s.owner() != "tests.one"));
}
