use super::*;
use crate::collector::FrameLabels;
use crate::collector::key;
use std::sync::atomic::{AtomicUsize, Ordering};
use workbench_model::ResourceUse;

/// A window's picture, counting how it's read.
#[derive(Default)]
struct Fake {
    own: AtomicUsize,
    all: AtomicUsize,
    both: AtomicUsize,
}

impl Fake {
    fn picture(name: &str) -> ResourceSnapshot {
        ResourceSnapshot {
            memory: 1000,
            parts: vec![ResourceUse::new(name, "app")],
            ..ResourceSnapshot::default()
        }
    }
}

impl ResourceSource for Fake {
    fn snapshot(&self) -> ResourceSnapshot {
        self.own.fetch_add(1, Ordering::Relaxed);
        Self::picture("own")
    }

    fn snapshot_all(&self) -> ResourceSnapshot {
        self.all.fetch_add(1, Ordering::Relaxed);
        Self::picture("all")
    }

    fn snapshot_both(&self) -> (ResourceSnapshot, ResourceSnapshot) {
        self.both.fetch_add(1, Ordering::Relaxed);
        (Self::picture("own"), Self::picture("all"))
    }
}

fn labels() -> FrameLabels {
    FrameLabels {
        other_threads: "Other".into(),
        unattributed: "Unattributed".into(),
        outside_heap: "Code, GPU, system".into(),
    }
}

const FAST: Duration = Duration::from_millis(20);

fn until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The view's next look, waited for.
fn next(view: &Subscription) -> Tick {
    let mut tick = None;
    assert!(until(|| {
        tick = view.tables.take();
        tick.is_some()
    }));
    tick.expect("a look")
}

fn source() -> (Arc<Fake>, Arc<dyn ResourceSource>) {
    let fake = Arc::new(Fake::default());
    let source: Arc<dyn ResourceSource> = fake.clone();
    (fake, source)
}

fn info(source: &Arc<dyn ResourceSource>) -> HubInfo {
    Hub::info(source).expect("a hub")
}

/// Hidden from the start, nothing runs; shown, it looks and its tables come; hidden, it
/// sleeps and lets go of the source; shown again, it looks again.
#[test]
fn a_view_shown_is_looked_for_and_a_hidden_one_sleeps() {
    let (_fake, source) = source();
    let view = Hub::subscribe_every(&source, labels(), false, FAST);
    // (The test holds it twice: as itself and as a source.)
    let held = |source: &Arc<dyn ResourceSource>| Arc::strong_count(source) - 2;
    assert_eq!((info(&source).thread, info(&source).looks), (false, 0), "hidden: no thread");
    assert_eq!(held(&source), 0);
    view.set_shown(true);
    assert!(info(&source).thread && info(&source).holds);
    assert!(held(&source) > 0, "shown: the source is held (the window fills in its parts)");
    let tick = next(&view);
    assert!(tick.now.lines.iter().any(|l| l.name == "own") && !tick.now.all);
    // Its history has the look.
    assert_eq!(
        view.history().lock().unwrap().total().get(crate::collector::Metric::Memory).last(),
        Some(1000.0)
    );
    view.set_shown(false);
    assert!(!info(&source).holds);
    // A look under way may still hold it a moment.
    assert!(until(|| held(&source) == 0), "hidden: the source let go");
    std::thread::sleep(FAST * 3);
    let looks = info(&source).looks;
    std::thread::sleep(FAST * 5);
    assert_eq!(info(&source).looks, looks, "asleep while hidden");
    assert!(info(&source).thread, "asleep, not ended");
    view.set_shown(true);
    assert!(until(|| info(&source).looks > looks), "shown again: it looks again");
}

/// Two views of one window: one thread, one look a tick; the second shown gets the last
/// table at once (not from nothing).
#[test]
fn two_views_share_one_thread_and_a_new_one_gets_what_was_seen() {
    let (fake, source) = source();
    let first = Hub::subscribe_every(&source, labels(), false, Duration::from_secs(60));
    first.set_shown(true);
    next(&first);
    let second = Hub::subscribe_every(&source, labels(), false, FAST);
    assert_eq!(info(&source).views, 2, "the same hub");
    second.set_shown(true);
    let given = second.tables.take().expect("the last table at once");
    assert!(given.now.lines.iter().any(|l| l.name == "own"));
    assert_eq!(info(&source).looks, 1, "nothing looked again for it");
    assert_eq!(fake.own.load(Ordering::Relaxed), 1);
    // Another window's source: a hub of its own.
    let (_other, other) = self::source();
    let theirs = Hub::subscribe_every(&other, labels(), false, FAST);
    assert_eq!(info(&other).views, 1);
    assert_eq!(info(&source).views, 2);
    drop(theirs);
}

/// One view of this project, one of every project: one read a look, both served.
#[test]
fn different_scopes_are_read_once() {
    let (fake, source) = source();
    let own = Hub::subscribe_every(&source, labels(), false, FAST);
    let all = Hub::subscribe_every(&source, labels(), true, FAST);
    own.set_shown(true);
    all.set_shown(true);
    let mine = next(&own);
    let every = next(&all);
    assert!(until(|| fake.both.load(Ordering::Relaxed) >= 2));
    // (The very first look may have come before the second view was shown.)
    assert!(fake.own.load(Ordering::Relaxed) <= 1);
    assert_eq!(fake.all.load(Ordering::Relaxed), 0, "never read apart while both are shown");
    let mine = own.tables.take().unwrap_or(mine);
    let every = all.tables.take().unwrap_or(every);
    assert!(!mine.now.all && mine.now.lines.iter().any(|l| l.name == "own"));
    assert!(every.now.all && every.now.lines.iter().any(|l| l.name == "all"));
    // Each scope its own history.
    let own_history = own.history();
    let all_history = all.history();
    assert!(own_history.lock().unwrap().part(&key("app", "own", None)).is_some());
    assert!(all_history.lock().unwrap().part(&key("app", "all", None)).is_some());
    assert!(own_history.lock().unwrap().part(&key("app", "all", None)).is_none());
    // Switching a view's scope: it gets that scope's table at once.
    own.set_all(true);
    let switched = own.tables.take().expect("the scope's last table");
    assert!(switched.now.all);
    assert!(Arc::ptr_eq(&own.history(), &all_history), "its history follows its scope");
}

/// The last view gone: the thread ends and the source is let go.
#[test]
fn the_last_view_gone_ends_the_thread() {
    let (_fake, source) = source();
    let alone = Arc::strong_count(&source);
    let a = Hub::subscribe_every(&source, labels(), false, FAST);
    let b = Hub::subscribe_every(&source, labels(), false, FAST);
    a.set_shown(true);
    b.set_shown(true);
    next(&a);
    drop(a);
    assert!(info(&source).thread, "one view left");
    drop(b);
    assert!(until(|| Hub::info(&source).is_none_or(|i| !i.thread)), "ended");
    assert!(until(|| Arc::strong_count(&source) == alone), "the source let go");
    // A new view later: a new hub, hidden until shown.
    let c = Hub::subscribe_every(&source, labels(), false, FAST);
    assert_eq!((info(&source).views, info(&source).thread), (1, false));
    c.set_shown(true);
    next(&c);
}

/// A view that never takes its looks (a screen's widget reads only the history) holds the
/// newest one, not one a look: hours of them mustn't pile up.
#[test]
fn looks_not_taken_are_replaced_not_kept() {
    let (_fake, source) = source();
    let widget = Hub::subscribe_every(&source, labels(), false, Duration::from_millis(2));
    widget.set_shown(true);
    let first = next(&widget);
    assert!(until(|| info(&source).looks > first.generation + 20), "it went on looking");
    let newest = widget.tables.take().expect("the newest look");
    assert!(newest.generation > first.generation + 20);
    assert!(widget.tables.take().is_none(), "only one waited: the older ones were replaced");
}
