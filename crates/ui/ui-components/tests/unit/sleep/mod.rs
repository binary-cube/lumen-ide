use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEXTS: SleepTexts = SleepTexts {
    asleep_title: "test.asleep_title",
    resuming: "test.resuming",
    by_hand: "test.by_hand",
    idle: "test.idle",
    freed: "test.freed",
    wake_up: "test.wake_up",
    how: "test.how",
    sleep_now: "test.sleep_now",
    going_to_sleep: "test.going_to_sleep",
    waking_up: "test.waking_up",
    cant: "test.cant",
    cant_sleeping: "test.cant_sleeping",
    cant_waking: "test.cant_waking",
};
static KIND: SleepKind =
    SleepKind { id: "test-view", viewer: "test.viewer", hint: None, texts: &TEXTS };
const TYPED: Veto = Veto {
    id: "typed",
    menu: "test.sleep_typed",
    refusal: "test.cant_typed",
    by_hand: true,
    never: false,
};
const IDLE: Duration = Duration::from_secs(60);

/// Quiet for as long as it says.
struct QuietFor(Duration);

impl Quiet for QuietFor {
    fn quiet_for(&self) -> Option<Duration> {
        Some(self.0)
    }
}

/// Every state published by any test (each looks at its own key's).
static PUBLISHED: Mutex<Vec<SleepStateChanged>> = Mutex::new(Vec::new());

fn listen() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        set_announcer(Some(Arc::new(|_, event: &SleepStateChanged| {
            lock(&PUBLISHED).push(event.clone());
        })));
    });
}

fn published(key: &str) -> Vec<Option<SleepState>> {
    lock(&PUBLISHED).iter().filter(|e| e.key == key).map(|e| e.state).collect()
}

/// A started view known by `key`, left alone and quiet for twice the idle time.
fn left_alone(key: &str) -> SleepPart {
    listen();
    let place = SleepPlace::Editor { name: key.into(), extension: "view-test" };
    let part = SleepPart::new(&KIND, key.into(), None, place);
    part.started(Some(Arc::new(QuietFor(IDLE * 2))));
    part.slot().lock().touched = Instant::now().checked_sub(IDLE * 2);
    part
}

/// A view is found by its key while it lives, and told gone when it's dropped.
#[test]
fn a_view_is_known_by_its_key_while_it_lives() {
    let part = left_alone("sleep-live-key");
    assert!(find("sleep-live-key").is_some());
    drop(part);
    assert!(find("sleep-live-key").is_none());
    assert_eq!(published("sleep-live-key").last(), Some(&None), "told gone");
}

/// Left alone and quiet, it's ordered to sleep and its window woken; its poll takes the
/// order, and it falls asleep saying why and what it freed.
#[test]
fn left_alone_it_is_ordered_to_sleep_and_takes_the_order() {
    let mut part = left_alone("sleep-idle-key");
    let woken = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    part.give_waker(
        &(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }) as Waker),
    );
    part.slot().set_memory(Some(2048));
    assert!(part.slot().sleep_idle(IDLE));
    assert_eq!(woken.load(Ordering::SeqCst), 1, "its window woken");
    assert_eq!(part.can_sleep(), CanSleep::Sleeping);
    assert!(part.order_waits());
    let why = part.take_order().unwrap();
    assert_eq!(why, SleepWhy::Idle(IDLE));
    part.fell_asleep(why);
    assert!(part.asleep());
    assert_eq!(part.slept().unwrap().freed, Some(2048), "what it held, said freed");
    assert_eq!(part.tab_icon(), Some(ASLEEP_ICON));
    let last = published("sleep-idle-key").last().copied().flatten().unwrap();
    assert!(last.asleep && !last.running && last.memory.is_none());
}

/// Touched, vetoed or busy lately, it isn't ordered; an idle order taken after a touch is
/// let go, the user's own isn't.
#[test]
fn anything_of_the_user_or_its_own_keeps_it_awake() {
    let part = left_alone("sleep-awake-key");
    part.touch();
    assert!(!part.slot().sleep_idle(IDLE), "touched now");
    let part = left_alone("sleep-veto-key");
    part.set_veto(Some(TYPED));
    assert!(!part.slot().sleep_idle(IDLE), "its own reason");
    assert_eq!(part.can_sleep(), CanSleep::NotNow(TYPED));
    assert_eq!(part.sleep_by_hand(), Err(CanSleep::NotNow(TYPED)));
    let part = left_alone("sleep-touched-key");
    assert!(part.slot().sleep_idle(IDLE));
    std::thread::sleep(Duration::from_millis(2));
    part.touch();
    assert_eq!(part.take_order(), None, "touched since: let go");
    assert_eq!(part.can_sleep(), CanSleep::Yes, "the order gone");
    part.touch();
    assert_eq!(part.sleep_by_hand(), Ok(()), "the user's own: whatever");
    assert_eq!(part.take_order(), Some(SleepWhy::ByHand));
}

/// Its tab's menu follows how it stands: put to sleep, greyed out with why, on its way,
/// wake up (greyed out while it wakes), nothing while it never can.
#[test]
fn its_menu_says_what_can_be_done_now() {
    let mut part = left_alone("sleep-menu-key");
    let row = |part: &SleepPart| match part.tab_menu().first() {
        Some(crate::ViewMenuEntry::Item { id, look, .. }) => Some((id.clone(), !look.disabled)),
        _ => None,
    };
    assert_eq!(row(&part), Some((SLEEP_ACTION.to_owned(), true)));
    part.set_veto(Some(TYPED));
    assert_eq!(row(&part), Some((SLEEP_ACTION.to_owned(), false)));
    part.set_veto(Some(Veto { never: true, ..TYPED }));
    assert_eq!(row(&part), None);
    part.set_veto(None);
    part.fell_asleep(SleepWhy::ByHand);
    assert_eq!(row(&part), Some((WAKE_ACTION.to_owned(), true)));
    part.waking(true);
    assert_eq!(row(&part), Some((WAKE_ACTION.to_owned(), false)), "waking: greyed out");
    part.waking(false);
    assert!(part.slept().is_none(), "back");
}

/// Woken from elsewhere only while asleep; the wake is taken once.
#[test]
fn a_wake_from_elsewhere_is_taken_once() {
    let mut part = left_alone("sleep-wake-key");
    assert!(!part.slot().wake(), "awake: nothing to wake");
    part.fell_asleep(SleepWhy::ByHand);
    assert!(part.slot().wake());
    assert!(part.order_waits());
    assert!(part.take_wake());
    assert!(!part.take_wake());
}

/// A state is published once per change, again when asked.
#[test]
fn a_state_is_published_once_per_change() {
    let part = left_alone("sleep-told-key");
    let told = published("sleep-told-key").len();
    part.announce();
    part.announce();
    assert_eq!(published("sleep-told-key").len(), told, "unchanged: not told");
    part.slot().announce_again();
    assert_eq!(published("sleep-told-key").len(), told + 1);
}

/// An order not taken in time is let go by the timer's look.
#[test]
fn an_order_not_taken_in_time_is_let_go() {
    let part = left_alone("sleep-stale-key");
    assert!(part.slot().sleep_idle(IDLE));
    assert!(!part.slot().drop_stale_order(Instant::now()));
    assert!(part.slot().drop_stale_order(Instant::now() + ::sleep::ORDER_FRESH));
    assert!(!part.order_waits());
}

/// A kind's idle time is what its module last told; never told: off.
#[test]
fn each_kind_sleeps_after_what_its_module_says() {
    static OTHER: SleepKind =
        SleepKind { id: "test-other", viewer: "test.viewer", hint: None, texts: &TEXTS };
    assert_eq!(idle_for("test-other"), None);
    configure(&OTHER, Some(IDLE));
    assert_eq!(idle_for("test-other"), Some(IDLE));
    configure(&OTHER, None);
    assert_eq!(idle_for("test-other"), None);
}

/// The keyboard's view is what the commands act on: drawn focused, it is; drawn without,
/// no longer.
#[test]
fn the_focused_view_is_the_one_drawn_with_the_keyboard() {
    let part = left_alone("sleep-focus-key");
    part.focus(true);
    assert!(focused().is_some_and(|s| Arc::ptr_eq(&s, part.slot())));
    part.focus(false);
    assert!(focused().is_none());
}

/// A sleeping view's picture is drawn at its own size whatever its area became: never
/// stretched into a larger one, nor shrunk into a smaller one (cut at its edge there).
#[test]
fn a_sleeping_view_s_picture_is_never_stretched() {
    let picture = render_api::ImageData::held(300, 200, vec![0; 300 * 200 * 4]);
    use kernel::geometry::Rect;
    for area in [Rect::new(10.0, 20.0, 900.0, 700.0), Rect::new(10.0, 20.0, 120.0, 80.0)] {
        let mut scene = render_api::Scene::default();
        let layer = scene.push_layer(Some(area));
        paint_picture(layer, area, Some(&picture), render_api::Rgba::rgb(0xffffff));
        let drawn = &scene.layers()[0].images;
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].bounds, Rect::new(10.0, 20.0, 300.0, 200.0), "its own size: {area:?}");
    }
}
