use super::*;

/// A moment to measure from (the test's clock: the domain itself never reads one).
#[allow(clippy::disallowed_methods)]
fn a_moment() -> Instant {
    Instant::now()
}

const IDLE: Duration = Duration::from_secs(3600);

/// A view left alone and quiet for `untouched` and `quiet`, `now` far enough on.
fn facts(untouched: Duration, quiet: Duration) -> (SleepFacts, Instant) {
    let now = a_moment() + IDLE * 3;
    let facts = SleepFacts {
        touched: Some(now - untouched),
        quiet_for: Some(quiet),
        ..SleepFacts::default()
    };
    (facts, now)
}

const TYPED: Veto = Veto {
    id: "text typed, not sent",
    menu: "menu",
    refusal: "refusal",
    by_hand: true,
    never: false,
};
const AT_WORK: Veto = Veto { id: "at work", by_hand: false, ..TYPED };
const ENDED: Veto = Veto { id: "ended", never: true, ..TYPED };

/// Only a view untouched and quiet for the whole idle time sleeps; anything else keeps it
/// awake, each reason named.
#[test]
fn only_an_untouched_quiet_view_sleeps() {
    let (f, now) = facts(IDLE, IDLE);
    assert_eq!(awake_because(&f, IDLE, now), None);
    let (f, now) = facts(IDLE / 2, IDLE);
    assert_eq!(awake_because(&f, IDLE, now), Some("used lately"));
    let (f, now) = facts(IDLE, IDLE / 2);
    assert_eq!(awake_because(&f, IDLE, now), Some("active lately"));
    let (mut f, now) = facts(IDLE, IDLE);
    f.quiet_for = None;
    assert_eq!(awake_because(&f, IDLE, now), Some("active lately"), "its quiet not known");
    let (mut f, now) = facts(IDLE, IDLE);
    f.veto = Some(AT_WORK);
    assert_eq!(awake_because(&f, IDLE, now), Some("at work"), "its own reason");
    let (mut f, now) = facts(IDLE, IDLE);
    f.touched = None;
    assert_eq!(awake_because(&f, IDLE, now), Some("never started"));
    let (mut f, now) = facts(IDLE, IDLE);
    f.asleep = true;
    assert_eq!(awake_because(&f, IDLE, now), Some("asleep"));
    let (mut f, now) = facts(IDLE, IDLE);
    f.ordered = true;
    assert_eq!(awake_because(&f, IDLE, now), Some("ordered to sleep"));
    let (mut f, now) = facts(IDLE, IDLE);
    f.waking = true;
    assert_eq!(awake_because(&f, IDLE, now), Some("waking"));
}

/// By hand: waking and going to sleep first, asleep or never-able hides it, a veto that holds
/// against the user greys it out saying why, one that doesn't (work under way) lets it.
#[test]
fn by_hand_it_sleeps_unless_its_own_reason_holds_against_the_user() {
    let (f, _) = facts(Duration::ZERO, Duration::ZERO);
    assert_eq!(can_sleep(&f), CanSleep::Yes, "used and busy lately: the user chose");
    assert_eq!(can_sleep(&SleepFacts { veto: Some(AT_WORK), ..f }), CanSleep::Yes);
    assert_eq!(can_sleep(&SleepFacts { veto: Some(TYPED), ..f }), CanSleep::NotNow(TYPED));
    assert_eq!(can_sleep(&SleepFacts { veto: Some(ENDED), ..f }), CanSleep::No);
    assert_eq!(can_sleep(&SleepFacts { asleep: true, ..f }), CanSleep::No);
    assert_eq!(can_sleep(&SleepFacts { touched: None, ..f }), CanSleep::No, "never started");
    assert_eq!(can_sleep(&SleepFacts { ordered: true, asleep: true, ..f }), CanSleep::Sleeping);
    assert_eq!(can_sleep(&SleepFacts { waking: true, ordered: true, ..f }), CanSleep::Waking);
}

/// An order is taken only fresh; an idle one only if the view wasn't touched since, the
/// user's own whatever.
#[test]
fn an_order_holds_while_fresh_and_untouched() {
    let at = a_moment();
    let later = at + Duration::from_secs(1);
    assert!(order_holds(at, false, Some(at), later));
    assert!(order_holds(at, false, None, later));
    assert!(!order_holds(at, false, Some(later), later), "touched since");
    assert!(order_holds(at, true, Some(later), later), "by hand: whatever");
    assert!(!order_holds(at, true, None, at + ORDER_FRESH), "too old");
}
