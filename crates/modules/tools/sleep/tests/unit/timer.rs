use super::*;
use std::sync::Arc;
use ui_components::sleep::{SleepKind, SleepPart, SleepPlace, SleepTexts};

static TEXTS: SleepTexts = SleepTexts {
    asleep_title: "t",
    resuming: "t",
    by_hand: "t",
    idle: "t",
    freed: "t",
    wake_up: "t",
    how: "t",
    sleep_now: "t",
    going_to_sleep: "t",
    waking_up: "t",
    cant: "t",
    cant_sleeping: "t",
    cant_waking: "t",
};
const IDLE: Duration = Duration::from_secs(60);

/// Quiet for as long as it says.
struct QuietFor(Duration);

impl views::Quiet for QuietFor {
    fn quiet_for(&self) -> Option<Duration> {
        Some(self.0)
    }
}

/// A view of `kind` left alone and quiet for twice the idle time.
fn left_alone(kind: &'static SleepKind, key: &str) -> SleepPart {
    let place = SleepPlace::Editor { name: key.into(), extension: "view-test" };
    let part = SleepPart::new(kind, key.into(), None, place);
    part.started(Some(Arc::new(QuietFor(IDLE * 2))));
    part.slot().lock().touched = Instant::now().checked_sub(IDLE * 2);
    part
}

/// A look orders to sleep only the views left alone of a kind whose sleeping is on; why each
/// other stays awake is kept as last told.
#[test]
fn a_look_orders_only_what_was_left_alone_with_its_kind_on() {
    static ON: SleepKind = SleepKind { id: "test-on", viewer: "v", hint: None, texts: &TEXTS };
    static OFF: SleepKind = SleepKind { id: "test-off", viewer: "v", hint: None, texts: &TEXTS };
    views::configure(&ON, Some(IDLE));
    views::configure(&OFF, None);
    let alone = left_alone(&ON, "look-alone-key");
    let used = left_alone(&ON, "look-used-key");
    used.touch();
    let off = left_alone(&OFF, "look-off-key");
    look(Instant::now());
    assert!(alone.order_waits(), "left alone: ordered");
    assert!(!used.order_waits(), "used lately");
    assert!(!off.order_waits(), "its kind's sleeping off");
    assert_eq!(used.slot().lock().told_awake, Some(Some("used lately")));
    assert_eq!(off.slot().lock().told_awake, Some(Some("its kind's sleeping is off")));
    // May sleep — or, another test's look meanwhile (the views are every test's), ordered.
    let told = alone.slot().lock().told_awake;
    assert!(matches!(told, Some(None | Some("ordered to sleep"))), "{told:?}");
}

/// An order never taken is let go at the next look.
#[test]
fn a_look_lets_go_an_order_never_taken() {
    static KIND: SleepKind = SleepKind { id: "test-stale", viewer: "v", hint: None, texts: &TEXTS };
    views::configure(&KIND, Some(IDLE));
    let part = left_alone(&KIND, "look-stale-key");
    look(Instant::now());
    assert!(part.order_waits());
    part.touch();
    look(Instant::now() + sleep::ORDER_FRESH);
    assert!(!part.order_waits(), "too old: let go (and, touched since, not given again)");
}
