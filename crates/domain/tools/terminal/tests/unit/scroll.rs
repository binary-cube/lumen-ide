use super::*;

const LINE: f32 = 20.0;

/// A trackpad's small steps add up to a smooth position: the emulator moves by whole lines,
/// the rest is the view's shift — nothing lost, both ways.
#[test]
fn small_steps_add_up_to_lines_and_a_shift() {
    let mut s = SmoothScroll::default();
    let (mut offset, history) = (0usize, 100usize);
    let step = |s: &mut SmoothScroll, offset: &mut usize, dy: f32| {
        let lines = s.wheel(dy, LINE, *offset, history);
        *offset = (*offset as i32 + lines) as usize;
        lines
    };
    assert_eq!(step(&mut s, &mut offset, -5.0), 0, "a quarter line: shifted only");
    assert_eq!(s.shift(LINE), 5.0);
    assert!(s.partial());
    for _ in 0..3 {
        step(&mut s, &mut offset, -5.0);
    }
    assert_eq!((offset, s.shift(LINE)), (1, 0.0), "four quarters: one line, exactly");
    step(&mut s, &mut offset, -30.0);
    assert_eq!((offset, s.shift(LINE)), (2, 10.0));
    // Back down past the bottom: at the bottom exactly (following the output again).
    step(&mut s, &mut offset, 1000.0);
    assert_eq!((offset, s.shift(LINE)), (0, 0.0));
    assert!(!s.partial());
}

/// A mouse wheel's notch (three lines) moves three whole lines, nothing partial.
#[test]
fn a_wheel_notch_moves_whole_lines() {
    let mut s = SmoothScroll::default();
    assert_eq!(s.wheel(-3.0 * LINE, LINE, 0, 100), 3);
    assert!(!s.partial());
    assert_eq!(s.wheel(3.0 * LINE, LINE, 3, 100), -3);
}

/// At the history's top nothing is above: exactly there, never shifted past it; with no
/// history, nothing moves.
#[test]
fn the_history_s_ends_are_exact() {
    let mut s = SmoothScroll::default();
    assert_eq!(s.wheel(-10_000.0, LINE, 0, 50), 50);
    assert!(!s.partial());
    let mut s = SmoothScroll::default();
    assert_eq!(s.wheel(-7.0, LINE, 0, 0), 0);
    assert!(!s.partial());
    // The emulator back at its bottom by itself (output followed, a key): settled.
    let mut s = SmoothScroll::default();
    s.wheel(-7.0, LINE, 0, 10);
    s.settle(0, 0);
    assert!(!s.partial());
}
