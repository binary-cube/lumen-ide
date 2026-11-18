use super::*;

#[test]
fn a_blinking_caret_alone_lets_a_window_turn_quiet() {
    let start = Instant::now();
    let mut activity = Activity::default();
    activity.frame(start);
    assert!(!activity.quiet(start));
    let mut now = start;
    for _ in 0..6 {
        now += Duration::from_millis(500);
        activity.frame(now);
    }
    assert!(activity.quiet(now));
}

#[test]
fn frames_close_together_keep_it_busy() {
    let start = Instant::now();
    let mut activity = Activity::default();
    activity.frame(start);
    let mut now = start;
    for _ in 0..100 {
        now += Duration::from_millis(40);
        activity.frame(now);
        assert!(!activity.quiet(now));
    }
    assert_eq!(activity.quiet_at(), Some(now + QUIET_AFTER));
}

#[test]
fn nothing_drawn_yet_is_not_quiet() {
    assert!(!Activity::default().quiet(Instant::now()));
}

#[test]
fn two_frames_meeting_by_chance_arent_work() {
    let start = Instant::now();
    let mut activity = Activity::default();
    activity.frame(start);
    let quiet = start + QUIET_AFTER + Duration::from_millis(10);
    activity.frame(quiet);
    activity.frame(quiet + Duration::from_millis(100));
    assert!(activity.quiet(quiet + Duration::from_millis(100)));
    activity.frame(quiet + Duration::from_millis(200));
    assert!(!activity.quiet(quiet + Duration::from_millis(200)), "a third close one is");
}
