use super::*;

#[test]
fn byte_range_normalizes_reversed_bounds() {
    let r = ByteRange::new(ByteOffset(10), ByteOffset(4));
    assert_eq!(r.start(), ByteOffset(4));
    assert_eq!(r.end(), ByteOffset(10));
    assert_eq!(r.len(), 6);
}

#[test]
fn byte_range_is_half_open() {
    let r = ByteRange::from(2..5);
    assert!(r.contains(ByteOffset(2)));
    assert!(!r.contains(ByteOffset(5)));
    assert!(!ByteRange::empty(ByteOffset(3)).contains(ByteOffset(3)));
}

#[test]
fn byte_range_intersection() {
    let a = ByteRange::from(0..5);
    assert!(a.intersects(&ByteRange::from(4..8)));
    assert!(!a.intersects(&ByteRange::from(5..8)));
}

#[test]
fn positions_order_by_line_then_column() {
    assert!(TextPosition::new(1, 0) > TextPosition::new(0, 99));
    assert!(TextPosition::new(1, 2) < TextPosition::new(1, 3));
}

#[test]
fn text_range_normalizes_and_displays_one_based() {
    let r = TextRange::new(TextPosition::new(3, 1), TextPosition::new(0, 0));
    assert_eq!(r.start(), TextPosition::new(0, 0));
    assert_eq!(TextPosition::new(0, 0).to_string(), "1:1");
}

#[test]
fn one_line_joins_lines_and_runs_of_whitespace() {
    assert_eq!(one_line("$\n    a = 1"), "$ a = 1");
    assert_eq!(one_line("a\r\n\tb  \u{a0} c"), "a b c");
    assert_eq!(one_line("  padded\n"), "padded");
    assert_eq!(one_line(""), "");
    assert_eq!(one_line(" \n\t "), "");
}

#[test]
fn one_line_borrows_when_nothing_changes() {
    use std::borrow::Cow;
    assert!(matches!(one_line("Optional parameter $a comes first"), Cow::Borrowed(_)));
    assert!(matches!(one_line("  trimmed only "), Cow::Borrowed("trimmed only")));
    assert!(matches!(one_line("two  spaces"), Cow::Owned(_)));
    assert!(matches!(one_line("a\nb"), Cow::Owned(_)));
}

#[test]
fn a_long_text_is_cut_in_the_middle() {
    assert_eq!(shorten_middle("short.php", 20), "short.php");
    let long = "asdfasdf asdf asdfasd fasdf fsad.php";
    let cut = shorten_middle(long, 15);
    assert_eq!(cut.chars().count(), 15);
    assert_eq!(cut, "asdfasd…sad.php");
    // By characters, not bytes.
    assert_eq!(shorten_middle("ăîșțâăîșțâ", 5), "ăî…țâ");
}
