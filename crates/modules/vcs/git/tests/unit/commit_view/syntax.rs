use super::*;

fn name(capture: &str) -> Arc<str> {
    capture.into()
}

#[test]
fn a_line_is_cut_where_its_color_changes_and_keeps_its_uncolored_parts() {
    let text = "if a\nb\n";
    let colored = Colored::new(text, vec![(0..2, name("keyword"))]);
    assert_eq!(colored.line(0), vec![(0..2, Some(name("keyword"))), (2..4, None)]);
    assert_eq!(colored.line(1), vec![(0..1, None)]);
}

#[test]
fn a_later_capture_wins_over_an_earlier_one() {
    let colored = Colored::new("abcd", vec![(0..4, name("string")), (1..2, name("escape"))]);
    let pieces = colored.line(0);
    assert_eq!(pieces[1], (1..2, Some(name("escape"))));
    assert_eq!(pieces.len(), 3);
}

#[test]
fn a_capture_spanning_lines_colors_each_without_their_ending() {
    let colored = Colored::new("/* a\r\nb */\n", vec![(0..10, name("comment"))]);
    assert_eq!(colored.line(0), vec![(0..4, Some(name("comment")))]);
    assert_eq!(colored.line(1), vec![(0..4, Some(name("comment")))]);
}

#[test]
fn a_capture_past_the_end_or_a_line_that_is_not_there_is_harmless() {
    let colored = Colored::new("ab", vec![(1..99, name("string"))]);
    assert_eq!(colored.line(0).len(), 2);
    assert!(colored.line(5).is_empty());
}

#[test]
fn a_hunks_lines_take_their_color_from_the_side_they_are_on() {
    let (old, new) = ("one\n", "two\n");
    let found = hunks(old, new, 0);
    // Without a grammar registered for `.unknown`, every line is simply uncolored.
    let colored = color_hunks("f.unknown", old, new, &found);
    assert_eq!(colored[0].len(), found[0].lines.len());
    assert!(colored[0].iter().all(Vec::is_empty));
}
