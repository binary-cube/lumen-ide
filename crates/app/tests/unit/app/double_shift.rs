use super::*;

const SHIFT: Modifiers = Modifiers { shift: true, ctrl: false, alt: false, meta: false };
const NONE: Modifiers = Modifiers { shift: false, ctrl: false, alt: false, meta: false };

#[test]
fn two_quick_presses_fire_once() {
    let mut d = DoubleShift::default();
    let t = Instant::now();
    assert!(!d.modifiers(SHIFT, t));
    assert!(!d.modifiers(NONE, t));
    assert!(!d.modifiers(SHIFT, t + Duration::from_millis(150)));
    assert!(d.modifiers(NONE, t + Duration::from_millis(200)));
    // A third press starts over.
    assert!(!d.modifiers(SHIFT, t + Duration::from_millis(250)));
    assert!(!d.modifiers(NONE, t + Duration::from_millis(300)));
}

#[test]
fn slow_presses_or_a_key_in_between_do_not_fire() {
    let mut d = DoubleShift::default();
    let t = Instant::now();
    d.modifiers(SHIFT, t);
    d.modifiers(NONE, t);
    d.modifiers(SHIFT, t + Duration::from_millis(900));
    assert!(!d.modifiers(NONE, t + Duration::from_millis(950)));
    d.modifiers(SHIFT, t + Duration::from_millis(1000));
    d.cancel();
    d.modifiers(SHIFT, t + Duration::from_millis(1100));
    assert!(!d.modifiers(NONE, t + Duration::from_millis(1150)));
}
