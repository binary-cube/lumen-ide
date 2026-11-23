use super::*;

/// Moving and showing nothing for long: told once, with the parts' names; a frame showing
/// something (or nothing moving) starts over; a short transition is never told.
#[test]
fn a_part_moving_unseen_is_told_once() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut unseen = UnseenMotion::default();
    let names = || vec!["task-manager"];
    assert_eq!(unseen.frame(at(0), (true, true), names), None);
    assert_eq!(unseen.frame(at(1_000), (true, true), names), None, "not long yet");
    assert_eq!(unseen.frame(at(2_100), (true, true), names), Some(vec!["task-manager"]));
    assert_eq!(unseen.frame(at(3_000), (true, true), names), None, "told once");
    assert_eq!(unseen.frame(at(3_100), (true, false), names), None, "a frame shown: over");
    assert_eq!(unseen.frame(at(3_200), (true, true), names), None, "starts over");
    assert_eq!(unseen.frame(at(5_300), (true, true), names), Some(vec!["task-manager"]));
    let mut short = UnseenMotion::default();
    for ms in (0..400).step_by(16) {
        assert_eq!(short.frame(at(ms), (true, true), names), None);
    }
    assert_eq!(short.frame(at(420), (false, true), names), None, "a transition's end");
}
