//! The stack's rules: identical messages merged, at most four shown with the rest waiting
//! in order, each kind's time, time standing still under the mouse, work in progress
//! turned into a success, closing, the history kept to its last fifty.

// The tests give the stack the moments it keeps.
#![allow(clippy::disallowed_methods)]

use super::*;

const TICK: Duration = Duration::from_millis(100);

/// `ms` milliseconds passed, in ticks (as frames would).
fn run(n: &mut Notifications, ms: u64, paused: bool) {
    for _ in 0..ms / 100 {
        n.advance(TICK, paused);
    }
}

fn titles(n: &Notifications) -> Vec<String> {
    n.shown().filter(|t| !t.is_leaving()).map(|t| t.notification.title.clone()).collect()
}

#[test]
fn identical_messages_merge_into_one_with_a_count() {
    let mut n = Notifications::new();
    let now = Instant::now();
    n.show(Notification::success("Saved."), now);
    run(&mut n, 2000, false);
    let second = n.show(Notification::success("Saved."), now);
    assert_eq!(titles(&n), ["Saved."]);
    let toast = n.shown().next().unwrap();
    assert_eq!(toast.count, 2);
    assert_eq!(toast.id(), second, "it takes the newer one's id");
    assert_eq!(toast.elapsed, Duration::ZERO, "its time starts again");
    // Another kind, or another detail: not the same.
    n.show(Notification::error("Saved."), now);
    n.show(Notification::success("Saved.").with_detail("elsewhere"), now);
    assert_eq!(n.shown().count(), 3);
    // A key: the same whatever it says.
    n.show(Notification::info("1 file").with_key("upload"), now);
    n.show(Notification::info("2 files").with_key("upload"), now);
    assert_eq!(titles(&n).last().map(String::as_str), Some("2 files"));
    assert_eq!(n.shown().count(), 4);
}

#[test]
fn at_most_four_are_shown_the_rest_wait_in_order() {
    let mut n = Notifications::new();
    let now = Instant::now();
    for i in 0..6 {
        n.show(Notification::error(format!("e{i}")), now);
    }
    assert_eq!(titles(&n), ["e0", "e1", "e2", "e3"]);
    assert_eq!(n.waiting(), 2);
    let first = n.shown().next().unwrap().id();
    assert!(n.dismiss(first));
    // The one closed leaves (its transition), the next one waiting takes its room.
    assert_eq!(titles(&n), ["e1", "e2", "e3"]);
    run(&mut n, 200, false);
    assert_eq!(titles(&n), ["e1", "e2", "e3", "e4"]);
    assert_eq!(n.waiting(), 1);
    assert!(n.get(first).is_none(), "gone once it left");
    // One waiting closed goes at once.
    let last = n.history().next().unwrap().id;
    assert!(n.dismiss(last));
    assert_eq!(n.waiting(), 0);
}

#[test]
fn each_kind_stays_its_time() {
    let cases = [
        (Notification::success("s"), Some(SHORT)),
        (Notification::info("i"), Some(SHORT)),
        (Notification::warning("w"), Some(LONG)),
        (Notification::error("e"), None),
        (Notification::progress("p"), None),
        (Notification::success("a").with_action("Undo", CommandId("edit.undo")), None),
        (
            Notification::error("t").with_timeout(Timeout::After(Duration::from_secs(2))),
            Some(Duration::from_secs(2)),
        ),
        (Notification::info("n").with_timeout(Timeout::Never), None),
    ];
    for (notification, expected) in cases {
        assert_eq!(notification.duration(), expected, "{}", notification.title);
    }
    let mut n = Notifications::new();
    let now = Instant::now();
    n.show(Notification::success("s"), now);
    n.show(Notification::warning("w"), now);
    n.show(Notification::error("e"), now);
    run(&mut n, 3900, false);
    assert_eq!(titles(&n), ["s", "w", "e"]);
    run(&mut n, 200, false);
    assert_eq!(titles(&n), ["w", "e"], "the success left after 4 s");
    run(&mut n, 4000, false);
    assert_eq!(titles(&n), ["e"], "the warning after 8 s");
    run(&mut n, 60_000, false);
    assert_eq!(titles(&n), ["e"], "an error stays");
    assert!(!n.ticking(), "nothing counts down: no frames for the ring");
}

#[test]
fn time_stands_still_under_the_mouse() {
    let mut n = Notifications::new();
    n.show(Notification::info("i"), Instant::now());
    run(&mut n, 3000, false);
    let remaining = n.shown().next().unwrap().remaining().unwrap();
    assert!((remaining - 0.25).abs() < 0.01, "{remaining}");
    run(&mut n, 10_000, true);
    assert_eq!(titles(&n), ["i"], "paused");
    assert_eq!(n.shown().next().unwrap().remaining(), Some(remaining));
    run(&mut n, 1100, false);
    assert!(titles(&n).is_empty());
}

#[test]
fn work_in_progress_turns_into_a_success() {
    let mut n = Notifications::new();
    let now = Instant::now();
    let work = Notification::progress("Exporting…");
    let id = work.id;
    n.show(work, now);
    run(&mut n, 30_000, false);
    assert_eq!(titles(&n), ["Exporting…"], "it stays while it works");
    assert!(n.ticking(), "its spinner turns");
    n.update(Notification::success("Exported.").with_detail("users.csv").with_id(id), now);
    let toast = n.get(id).unwrap();
    assert_eq!(toast.notification.kind, NotificationKind::Success);
    assert_eq!(toast.notification.detail.as_deref(), Some("users.csv"));
    assert_eq!(n.shown().count(), 1, "in its place");
    run(&mut n, 4100, false);
    assert!(n.is_empty() || titles(&n).is_empty(), "then it goes as a success does");
    // Updated once gone: shown again.
    n.update(Notification::error("Failed.").with_id(id), now);
    assert_eq!(titles(&n), ["Failed."]);
    assert_eq!(n.history().next().unwrap().title, "Failed.");
}

#[test]
fn closing_all_and_the_transitions() {
    let mut n = Notifications::new();
    let now = Instant::now();
    for i in 0..5 {
        n.show(Notification::info(format!("i{i}")), now);
    }
    let toast = n.shown().next().unwrap();
    assert_eq!(toast.presence(), 0.0, "it enters");
    run(&mut n, 300, false);
    assert_eq!(n.shown().next().unwrap().presence(), 1.0);
    assert!(!n.transitioning());
    let generation = n.generation();
    assert!(n.dismiss_all());
    assert!(n.generation() > generation);
    assert_eq!(n.waiting(), 0);
    assert!(n.transitioning(), "they leave");
    run(&mut n, 300, false);
    assert!(n.is_empty());
    assert!(!n.dismiss_all(), "none left");
}

#[test]
fn the_history_keeps_the_last_fifty_newest_first() {
    let mut n = Notifications::new();
    let now = Instant::now();
    for i in 0..60 {
        n.show(Notification::error(format!("e{i}")), now);
    }
    assert_eq!(n.history().count(), HISTORY);
    assert_eq!(n.history().next().unwrap().title, "e59");
    assert_eq!(n.history().last().unwrap().title, "e10");
    // A merged one moves to the top, counted.
    n.show(Notification::error("e0"), now);
    let top = n.history().next().unwrap();
    assert_eq!((top.title.as_str(), top.count), ("e0", 2));
    assert_eq!(n.history().filter(|r| r.title == "e0").count(), 1);
}

#[test]
fn frozen_time_passes_for_none() {
    let mut n = Notifications::new();
    let id = n.show(Notification::success("s"), Instant::now());
    n.freeze(true);
    assert_eq!(n.get(id).unwrap().presence(), 1.0, "entered at once");
    n.set_elapsed(id, Duration::from_secs(1));
    run(&mut n, 10_000, false);
    assert_eq!(n.get(id).unwrap().remaining(), Some(0.75));
    assert!(!n.ticking() && !n.transitioning());
}

#[test]
fn positions_read_back_and_place_the_stack() {
    use settings_model::SettingType;
    for position in ToastPosition::ALL {
        assert_eq!(ToastPosition::from_value(&position.to_value()), Some(position));
    }
    assert_eq!(ToastPosition::default(), ToastPosition::TopCenter);
    assert!(ToastPosition::TopRight.top() && !ToastPosition::BottomCenter.top());
    assert!(ToastPosition::BottomCenter.centered() && !ToastPosition::BottomRight.centered());
    assert_eq!(ToastPosition::parse("middle"), None);
}

#[test]
fn ids_never_repeat_and_actions_stop_at_two() {
    let a = Notification::info("a");
    let b = Notification::info("a");
    assert_ne!(a.id, b.id);
    let many = Notification::info("x")
        .with_action("1", CommandId("a"))
        .with_action("2", CommandId("b"))
        .with_action("3", CommandId("c"));
    assert_eq!(many.actions.len(), MAX_ACTIONS);
    assert_eq!(Notification::info("x").with_detail("").detail, None);
}
