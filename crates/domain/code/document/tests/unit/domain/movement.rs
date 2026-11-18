use super::*;

#[test]
fn graphemes_cross_lines_and_combined_chars() {
    let b = TextBuffer::new("ae\u{301}\nx"); // "e" + combining accent = one grapheme
    assert_eq!(next_grapheme(&b, 1), 4);
    assert_eq!(prev_grapheme(&b, 4), 1);
    assert_eq!(next_grapheme(&b, 4), 5); // moves to the next line
    assert_eq!(prev_grapheme(&b, 5), 4);
    assert_eq!(prev_grapheme(&b, 0), 0);
}

#[test]
fn word_motions() {
    let b = TextBuffer::new("  $user->name = 1;");
    assert_eq!(word_right(&b, 0), 3); // whitespace, then `$`
    assert_eq!(word_right(&b, 3), 7); // `user`
    assert_eq!(word_left(&b, 7), 3);
    assert_eq!(word_left(&b, 9), 7); // `->`
}

#[test]
fn vertical_motion_keeps_goal_column() {
    let b = TextBuffer::new("abcdef\nab\nabcdef");
    let (o, goal) = move_head(&b, 5, None, Motion::Down, 4);
    assert_eq!(o, 9); // the end of the short line
    let (o, _) = move_head(&b, o, goal, Motion::Down, 4);
    assert_eq!(o, 15);
}

#[test]
fn tabs_count_as_visual_columns() {
    let b = TextBuffer::new("\tx\n    y");
    assert_eq!(visual_column(&b, 1, 4), 4);
    assert_eq!(offset_at_column(&b, 1, 4, 4), 7);
    let (o, _) = move_head(&b, 7, None, Motion::Up, 4);
    assert_eq!(o, 1);
}

#[test]
fn smart_home_toggles() {
    let b = TextBuffer::new("    code");
    assert_eq!(move_head(&b, 6, None, Motion::LineStart, 4).0, 4);
    assert_eq!(move_head(&b, 4, None, Motion::LineStart, 4).0, 0);
}
