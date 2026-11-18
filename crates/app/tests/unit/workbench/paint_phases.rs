use super::*;
use std::time::Duration;

#[test]
fn the_parts_are_told_slowest_first_and_short_ones_left_out() {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let marks = vec![("start", at(0)), ("a", at(10)), ("short", at(11)), ("b", at(61))];
    let phases = PaintPhases { marks, frames: 1 };
    assert_eq!(phases.text(), "b=50ms a=10ms");
}

#[test]
fn nothing_is_measured_after_the_first_frames() {
    let mut phases = PaintPhases::default();
    for _ in 0..FRAMES {
        phases.begin();
        phases.mark("a");
        assert_eq!(phases.marks.len(), 2);
    }
    phases.begin();
    phases.mark("a");
    assert!(phases.marks.is_empty());
}
