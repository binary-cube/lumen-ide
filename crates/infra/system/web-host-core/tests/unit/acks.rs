use std::time::{Duration, Instant};

use web_engine_protocol::PageId;

use super::Acks;
use crate::engine::PageKey;
use crate::server::ClientId;

const PAGE: PageKey = PageKey { client: ClientId(1), page: PageId(3) };

/// A frame not copied in time puts its page behind: the next ones aren't waited for, and the
/// page says so until a copy comes; an older frame counts as copied with a newer one.
#[test]
fn a_page_not_copied_in_time_falls_behind_until_a_copy_comes() {
    let acks = Acks::default();
    acks.open(PAGE);
    assert!(!acks.behind(PAGE), "a page opened isn't behind");
    assert!(!acks.wait(PAGE, 1, Duration::from_millis(20)), "not told: given up");
    assert!(acks.behind(PAGE));
    let since = Instant::now();
    assert!(!acks.wait(PAGE, 2, Duration::from_secs(5)));
    assert!(since.elapsed() < Duration::from_millis(50), "not waited for");
    acks.done(PAGE, 2);
    assert!(!acks.behind(PAGE), "caught up");
    assert!(acks.wait(PAGE, 1, Duration::ZERO), "an older one: copied too");
}

/// A page gone: its copies forgotten, a late one not kept, and it's not behind.
#[test]
fn a_page_gone_keeps_nothing() {
    let acks = Acks::default();
    acks.open(PAGE);
    assert!(!acks.wait(PAGE, 1, Duration::ZERO));
    acks.forget(PAGE);
    assert!(!acks.behind(PAGE));
    acks.done(PAGE, 9);
    assert!(!acks.wait(PAGE, 9, Duration::ZERO));
}

/// A copy for a page that was behind wakes the main loop (it paints the page again at
/// once); one for a page keeping up wakes nobody.
#[test]
fn a_page_catching_up_wakes_the_main_loop() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let woken = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&woken);
    let acks = Acks::waking(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    acks.open(PAGE);
    acks.done(PAGE, 1);
    assert_eq!(woken.load(Ordering::SeqCst), 0, "keeping up: nobody woken");
    assert!(!acks.wait(PAGE, 2, Duration::ZERO));
    acks.done(PAGE, 2);
    assert_eq!(woken.load(Ordering::SeqCst), 1, "caught up: woken once");
    acks.done(PAGE, 3);
    assert_eq!(woken.load(Ordering::SeqCst), 1);
}
