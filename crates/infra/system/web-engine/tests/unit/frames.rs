use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

use web_engine_protocol::{Changed, Command, PageId};

use super::*;
use crate::connection::Commands;

/// A queue of `kept` commands, and how many times it was found full.
fn queue(kept: usize) -> (Commands, Receiver<Command>, Arc<AtomicUsize>) {
    let full = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&full);
    let (commands, outgoing) = Commands::new(
        kept,
        Arc::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        }),
    );
    (commands, outgoing, full)
}

/// A frame dropped tells the engine it was copied; what it changed is kept cut to it.
#[test]
fn a_frame_dropped_is_given_back() {
    let (commands, outgoing, _) = queue(4);
    let native = Native::Pixels { stride: 16 };
    let mut frame = ArrivedFrame::new((PageId(3), 9), (4, 2), native, commands);
    let rect = |x, y, width, height| Changed { x, y, width, height };
    frame.told_changed(&[rect(1, 1, 10, 10), rect(9, 0, 1, 1)]);
    assert_eq!(frame.changed, [rect(1, 1, 3, 1)]);
    drop(frame);
    assert_eq!(outgoing.try_recv(), Ok(Command::FrameDone { page: PageId(3), frame: 9 }));
}

/// The commands' queue never grows past its room nor waits: one that doesn't fit says the
/// host stopped reading (the connection ends); once the writer is gone, nothing is said.
#[test]
fn a_full_queue_ends_rather_than_grows() {
    let (commands, outgoing, full) = queue(2);
    for _ in 0..3 {
        commands.send(Command::BeginFrame { page: PageId(1) });
    }
    assert_eq!(full.load(Ordering::Relaxed), 1, "the third didn't fit");
    assert_eq!(outgoing.try_iter().count(), 2);
    drop(outgoing);
    commands.send(Command::BeginFrame { page: PageId(1) });
    assert_eq!(full.load(Ordering::Relaxed), 1, "gone: dropped");
}

#[cfg(target_os = "macos")]
mod mac {
    #![allow(unsafe_code)]

    use std::sync::mpsc::{self, Receiver};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use web_engine_protocol::{Changed, Command, PageId};

    use crate::connection::{PageEvent, Sink};
    use crate::frames::{Arrivals, Meta};

    unsafe extern "C" {
        static mach_task_self_: u32;
        fn mach_port_allocate(task: u32, right: u32, name: *mut u32) -> i32;
        fn mach_port_insert_right(task: u32, name: u32, right: u32, kind: u32) -> i32;
    }

    /// A fresh send right of our own (as a surface's would come).
    fn right() -> mach_ports::Right {
        let mut name = 0;
        // SAFETY: a new receive right in our task, then a send right made from it.
        unsafe {
            let task = mach_task_self_;
            assert_eq!(mach_port_allocate(task, 1, &raw mut name), 0);
            assert_eq!(mach_port_insert_right(task, name, name, 20), 0);
        }
        mach_ports::Right::adopt(name)
    }

    /// What a frame changed, as its message tells it.
    const TOLD: Changed = Changed { x: 0, y: 0, width: 2, height: 1 };

    fn meta(page: u64, number: u64, popup: bool) -> Meta {
        Meta { page: PageId(page), number, size: (4, 2), popup, changed: vec![TOLD] }
    }

    /// Arrivals with a sink keeping the frames it was handed (number, what changed; each
    /// frame dropped at once: given back), the frames given back, and the wakes counted.
    struct Watched {
        arrivals: Arrivals,
        sink: Sink,
        shown: Receiver<(u64, Vec<Changed>)>,
        commands: Receiver<Command>,
        wakes: Arc<std::sync::atomic::AtomicUsize>,
    }

    fn watched() -> Watched {
        let (commands, outgoing) = crate::connection::Commands::new(64, Arc::new(|| {}));
        let (tx, shown) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: Sink = Arc::new(move |event| {
            if let PageEvent::Frame(frame) = event {
                let shown = (frame.number, frame.changed);
                let _ = tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(shown);
            }
        });
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let woken = Arc::clone(&wakes);
        let wake = Arc::new(move || {
            woken.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        Watched { arrivals: Arrivals::new(commands, wake), sink, shown, commands: outgoing, wakes }
    }

    impl Watched {
        fn shown(&self) -> Option<u64> {
            self.shown.try_recv().ok().map(|(number, _)| number)
        }
    }

    fn given_back(commands: &Receiver<Command>) -> Vec<u64> {
        commands
            .try_iter()
            .filter_map(|c| match c {
                Command::FrameDone { frame, .. } => Some(frame),
                _ => None,
            })
            .collect()
    }

    /// A frame is handed on by whichever comes second, its port or its message — the message
    /// never waits; a frame whose port doesn't come is given back, and its port coming late is
    /// let go, never kept.
    #[test]
    fn a_frame_is_handed_on_when_both_halves_came() {
        let w = watched();
        let (arrivals, sink, commands) = (&w.arrivals, &w.sink, &w.commands);
        arrivals.open(1);
        arrivals.arrived(1, 1, right());
        arrivals.frame(meta(1, 1, false), Arc::clone(sink));
        assert_eq!(w.shown(), Some(1), "port first: at once");
        let started = Instant::now();
        arrivals.frame(meta(1, 2, false), Arc::clone(sink));
        assert!(started.elapsed() < Duration::from_millis(50), "the message never waits");
        assert!(w.shown().is_none(), "parked for its port");
        arrivals.arrived(1, 2, right());
        assert_eq!(w.shown(), Some(2), "its port completed it");
        arrivals.frame(meta(1, 3, false), Arc::clone(sink));
        arrivals.expire(Instant::now() + Duration::from_secs(1));
        assert!(given_back(commands).contains(&3), "overdue: given back");
        arrivals.arrived(1, 3, right());
        assert!(w.shown().is_none());
        assert_eq!(arrivals.kept().0, 0, "a late port isn't kept");
    }

    /// A newer frame shown gives back an older one of its kind still parked (never shown after
    /// it); a popup's frame doesn't give back the page's.
    #[test]
    fn an_older_frame_never_follows_a_newer_one() {
        let w = watched();
        let (arrivals, sink, commands) = (&w.arrivals, &w.sink, &w.commands);
        arrivals.open(1);
        arrivals.frame(meta(1, 1, false), Arc::clone(sink));
        arrivals.frame(meta(1, 2, true), Arc::clone(sink));
        arrivals.arrived(1, 3, right());
        arrivals.frame(meta(1, 3, false), Arc::clone(sink));
        assert_eq!(w.shown(), Some(3));
        assert!(given_back(commands).contains(&1), "the page's older one given back");
        arrivals.arrived(1, 2, right());
        assert_eq!(w.shown(), Some(2), "the popup's still came");
        arrivals.arrived(1, 1, right());
        assert!(w.shown().is_none(), "the given back one never shows");
    }

    /// A closed page keeps nothing: its parked frames given back, its ports let go, and what
    /// comes for it later neither shown nor kept; a page has a few ports kept at most.
    #[test]
    fn a_closed_page_keeps_nothing() {
        let w = watched();
        let (arrivals, sink, commands) = (&w.arrivals, &w.sink, &w.commands);
        arrivals.open(1);
        arrivals.frame(meta(1, 1, false), Arc::clone(sink));
        arrivals.arrived(1, 5, right());
        arrivals.forget(1);
        assert_eq!(given_back(commands), [1]);
        arrivals.arrived(1, 6, right());
        arrivals.frame(meta(1, 7, false), Arc::clone(sink));
        assert_eq!(given_back(commands), [7]);
        assert!(w.shown().is_none());
        assert_eq!(arrivals.kept(), (0, 0, 0), "nothing kept for a closed page");
        arrivals.open(2);
        for frame in 1..=6 {
            arrivals.arrived(2, frame, right());
        }
        assert_eq!(arrivals.kept().0, 4, "the oldest let go");
        arrivals.frame(meta(2, 6, false), Arc::clone(sink));
        assert_eq!(w.shown(), Some(6));
        assert_eq!(arrivals.kept().0, 0, "the older ones' messages came already");
    }

    /// A frame given back unseen (overdue, or an older one superseded) makes the next one
    /// of its kind whole (empty `changed`); the other kind keeps what it changed; then what
    /// a frame changed is told again.
    #[test]
    fn a_frame_given_back_unseen_makes_the_next_one_whole() {
        let w = watched();
        w.arrivals.open(1);
        w.arrivals.arrived(1, 1, right());
        w.arrivals.frame(meta(1, 1, false), Arc::clone(&w.sink));
        assert_eq!(w.shown.try_recv().ok(), Some((1, vec![TOLD])), "the first as told");
        w.arrivals.frame(meta(1, 2, false), Arc::clone(&w.sink));
        w.arrivals.expire(Instant::now() + Duration::from_secs(1));
        assert_eq!(given_back(&w.commands), [1, 2], "1 copied, 2 overdue");
        w.arrivals.arrived(1, 3, right());
        w.arrivals.frame(meta(1, 3, false), Arc::clone(&w.sink));
        assert_eq!(w.shown.try_recv().ok(), Some((3, Vec::new())), "after one unseen: whole");
        w.arrivals.arrived(1, 4, right());
        w.arrivals.frame(meta(1, 4, false), Arc::clone(&w.sink));
        assert_eq!(w.shown.try_recv().ok(), Some((4, vec![TOLD])), "then as told again");
        // Superseded: the page's 5 given back for its 6; the popup's 7 unaffected.
        w.arrivals.frame(meta(1, 5, false), Arc::clone(&w.sink));
        w.arrivals.arrived(1, 6, right());
        w.arrivals.frame(meta(1, 6, false), Arc::clone(&w.sink));
        assert_eq!(w.shown.try_recv().ok(), Some((6, Vec::new())), "5 superseded: whole");
        w.arrivals.arrived(1, 7, right());
        w.arrivals.frame(meta(1, 7, true), Arc::clone(&w.sink));
        assert_eq!(w.shown.try_recv().ok(), Some((7, vec![TOLD])), "the popup's own");
    }

    /// A frame's message gives back only its own page's overdue frames: another page's
    /// stay for the ports' thread to give back.
    #[test]
    fn a_frame_expires_only_its_own_page() {
        let w = watched();
        w.arrivals.open(1);
        w.arrivals.open(2);
        w.arrivals.frame(meta(1, 1, false), Arc::clone(&w.sink));
        std::thread::sleep(crate::frames::mac::PORT_WAIT + Duration::from_millis(20));
        w.arrivals.frame(meta(2, 1, false), Arc::clone(&w.sink));
        assert!(given_back(&w.commands).is_empty(), "page 1's overdue frame left alone");
        assert_eq!(w.arrivals.kept().1, 2, "both parked");
        w.arrivals.frame(meta(1, 2, true), Arc::clone(&w.sink));
        assert_eq!(given_back(&w.commands), [1], "its own page's overdue frame given back");
    }

    /// Only a frame parked wakes the ports' thread (its deadline is new); none parked: no
    /// deadline to wait for.
    #[test]
    fn a_parked_frame_wakes_the_ports_thread() {
        let w = watched();
        let wakes = || w.wakes.load(std::sync::atomic::Ordering::Relaxed);
        w.arrivals.open(1);
        assert_eq!(w.arrivals.expire(Instant::now()), None, "nothing parked");
        w.arrivals.arrived(1, 1, right());
        w.arrivals.frame(meta(1, 1, false), Arc::clone(&w.sink));
        assert_eq!(wakes(), 0, "handed on at once");
        w.arrivals.frame(meta(1, 2, false), Arc::clone(&w.sink));
        assert_eq!(wakes(), 1, "parked");
        assert!(w.arrivals.expire(Instant::now()).is_some(), "its deadline");
    }
}
