use super::*;

fn matcher(pattern: &str, regex: bool, case: bool, word: bool) -> Matcher {
    let q = SearchQuery { pattern: pattern.into(), regex, case_sensitive: case, whole_word: word };
    Matcher::new(&q).unwrap().unwrap()
}

#[test]
fn plain_search_is_escaped_and_case_insensitive_by_default() {
    let b = TextBuffer::new("a.b A.B\naxb");
    assert_eq!(matcher("a.b", false, false, false).find_all(&b), vec![0..3, 4..7]);
    assert_eq!(matcher("a.b", false, true, false).find_all(&b), vec![0..3]);
}

#[test]
fn regex_and_whole_word() {
    let b = TextBuffer::new("$user $username\n$user");
    assert_eq!(matcher(r"\$user\b", true, true, false).find_all(&b), vec![0..5, 16..21]);
    assert_eq!(matcher("user", false, true, true).find_all(&b), vec![1..5, 17..21]);
}

#[test]
fn invalid_regex_is_an_error_and_empty_pattern_matches_nothing() {
    let q = SearchQuery { pattern: "(".into(), regex: true, ..Default::default() };
    assert!(Matcher::new(&q).is_err());
    assert!(Matcher::new(&SearchQuery::default()).unwrap().is_none());
}

#[test]
fn regex_replacement_uses_groups() {
    let m = matcher(r"(\w+)@(\w+)", true, true, false);
    assert_eq!(m.replacement("ion@site", "$2/$1", true), "site/ion");
    assert_eq!(m.replacement("ion@site", "$2/$1", false), "$2/$1");
}

#[test]
fn next_and_prev_wrap_around() {
    let matches = [2..3, 8..9];
    assert_eq!(next_match(&matches, 3), Some(1));
    assert_eq!(next_match(&matches, 9), Some(0));
    assert_eq!(prev_match(&matches, 8), Some(0));
    assert_eq!(prev_match(&matches, 1), Some(1));
}

/// A text's lines with matches (any line break), each with where they are; at most as many
/// as asked for; a text without one passed over.
#[test]
fn a_text_s_matching_lines_with_their_places() {
    let m = matcher("cart", false, false, true);
    let text = "<?php\r\n$cart = new Cart();\n// no\nreturn $cart;";
    let lines = m.lines_matching(text, 10);
    assert_eq!(lines.iter().map(|l| l.line).collect::<Vec<_>>(), [1, 3]);
    assert_eq!(lines[0].text, "$cart = new Cart();", "without its line break");
    assert_eq!(lines[0].ranges, [1..5, 12..16]);
    assert_eq!(&text[lines[0].start + 1..lines[0].start + 5], "cart", "where it is in the text");
    assert_eq!(&text[lines[1].start..][..7], "return ");
    assert_eq!(m.lines_matching(text, 2).iter().map(|l| l.ranges.len()).sum::<usize>(), 2);
    assert!(m.lines_matching("nothing here", 10).is_empty());
    let case = matcher("cart", false, true, true);
    let ranges = &case.lines_matching(text, 10)[0].ranges;
    assert!(ranges.len() == 1 && ranges[0] == (1..5), "{ranges:?}");
}
