use proptest::prelude::*;

use super::*;

const SAMPLE: &str = "ab\ncd\n\né😀x\nț";

#[test]
fn lines_are_split_on_newlines() {
    let lines = Lines::new(SAMPLE.to_owned());
    assert_eq!(lines.line_count(), 5);
    assert_eq!(lines.line(0), "ab");
    assert_eq!(lines.line(2), "");
    assert_eq!(lines.line(3), "é😀x");
    assert_eq!(lines.line(4), "ț");
    assert_eq!(lines.line(9), "");
}

#[test]
fn a_carriage_return_before_a_newline_isnt_on_the_line() {
    let lines = Lines::new("ab\r\ncd".to_owned());
    assert_eq!(lines.line(0), "ab");
    assert_eq!(lines.offset(Position::new(0, 9), Encoding::Utf16), 2);
    assert_eq!(lines.offset(Position::new(1, 1), Encoding::Utf16), 5);
}

#[test]
fn utf16_counts_surrogate_pairs_as_two() {
    let lines = Lines::new(SAMPLE.to_owned());
    let line3 = SAMPLE.find('é').unwrap();
    let after_emoji = line3 + "é😀".len();
    assert_eq!(lines.position(after_emoji, Encoding::Utf16), Position::new(3, 3));
    assert_eq!(lines.position(after_emoji, Encoding::Utf32), Position::new(3, 2));
    assert_eq!(lines.position(after_emoji, Encoding::Utf8), Position::new(3, 6));
    assert_eq!(lines.offset(Position::new(3, 3), Encoding::Utf16), after_emoji);
    assert_eq!(lines.offset(Position::new(3, 2), Encoding::Utf32), after_emoji);
    assert_eq!(lines.offset(Position::new(3, 6), Encoding::Utf8), after_emoji);
}

#[test]
fn a_column_inside_a_character_falls_to_its_start() {
    let lines = Lines::new(SAMPLE.to_owned());
    let line3 = SAMPLE.find('é').unwrap();
    // The middle of the emoji's surrogate pair.
    assert_eq!(lines.offset(Position::new(3, 2), Encoding::Utf16), line3 + "é".len());
    // The middle of `é`'s two bytes.
    assert_eq!(lines.offset(Position::new(3, 1), Encoding::Utf8), line3);
    // A byte inside a character, as a position: its start.
    assert_eq!(lines.position(line3 + 1, Encoding::Utf16), Position::new(3, 0));
}

#[test]
fn positions_past_the_end_are_clamped() {
    let lines = Lines::new(SAMPLE.to_owned());
    assert_eq!(lines.offset(Position::new(0, 99), Encoding::Utf16), 2);
    assert_eq!(lines.offset(Position::new(99, 0), Encoding::Utf16), SAMPLE.len());
    assert_eq!(lines.position(SAMPLE.len() + 10, Encoding::Utf16), Position::new(4, 1));
}

#[test]
fn an_empty_text_has_one_empty_line() {
    let lines = Lines::new(String::new());
    assert_eq!(lines.line_count(), 1);
    assert_eq!(lines.position(0, Encoding::Utf16), Position::new(0, 0));
    assert_eq!(lines.offset(Position::new(3, 3), Encoding::Utf16), 0);
}

#[test]
fn a_text_ending_with_a_newline_has_an_empty_last_line() {
    let lines = Lines::new("a\n".to_owned());
    assert_eq!(lines.line_count(), 2);
    assert_eq!(lines.position(2, Encoding::Utf16), Position::new(1, 0));
}

#[test]
fn replacing_reindexes_the_lines_after() {
    let mut lines = Lines::new("one\ntwo\nthree".to_owned());
    lines.replace(4..7, "2\nand a half");
    assert_eq!(lines.text(), "one\n2\nand a half\nthree");
    assert_eq!(lines.line_count(), 4);
    assert_eq!(lines.line(3), "three");
    lines.replace(0..lines.text().len(), "");
    assert_eq!(lines.line_count(), 1);
}

#[test]
fn the_differing_span_is_found() {
    assert_eq!(differing("abc", "abc"), None);
    assert_eq!(differing("abc", "abXc"), Some((2, 2, 3)));
    assert_eq!(differing("abc", "ac"), Some((1, 2, 1)));
    assert_eq!(differing("", "x"), Some((0, 0, 1)));
    assert_eq!(differing("x", ""), Some((0, 1, 0)));
    // Repeated characters: the prefix wins, the suffix never overlaps it.
    assert_eq!(differing("aaa", "aaaa"), Some((3, 3, 4)));
    assert_eq!(differing("aaaa", "aa"), Some((2, 4, 2)));
}

#[test]
fn the_differing_span_keeps_to_characters() {
    // `é` (c3 a9) and `è` (c3 a8) share their first byte.
    let (start, old_end, new_end) = differing("aé", "aè").unwrap();
    assert_eq!((start, old_end, new_end), (1, 3, 3));
    // Shared last byte: `á` (c3 a1) and `ġ` (c4 a1).
    let (start, old_end, new_end) = differing("áz", "ġz").unwrap();
    assert_eq!((start, old_end, new_end), (0, 2, 2));
}

#[test]
fn a_mirror_sends_one_change_in_the_old_positions() {
    let mut mirror = Mirror::new("let 😀 = 1;\nfoo".to_owned(), 1);
    let change = mirror.update("let 😀 = 12;\nfoo", 2, Encoding::Utf16).unwrap();
    // `let ` is 4 units, the emoji 2, ` = 1` 4 more: the insertion at column 10.
    assert_eq!(
        change,
        Change { start: Position::new(0, 10), end: Position::new(0, 10), text: "2".into() }
    );
    assert_eq!(mirror.version, 2);
    assert_eq!(mirror.update("let 😀 = 12;\nfoo", 3, Encoding::Utf16), None);
    assert_eq!(mirror.version, 3);
    let change = mirror.update("foo", 4, Encoding::Utf16).unwrap();
    assert_eq!(
        change,
        Change { start: Position::new(0, 0), end: Position::new(1, 0), text: String::new() }
    );
}

/// Applies `change` (in `encoding`) to `text`, as a server would.
fn applied(text: &str, change: &Change, encoding: Encoding) -> String {
    let lines = Lines::new(text.to_owned());
    let range = lines.bytes(change.start, change.end, encoding);
    let mut out = text.to_owned();
    out.replace_range(range, &change.text);
    out
}

fn text_strategy() -> impl Strategy<Value = String> {
    // Plain letters, newlines, two-byte, three-byte and four-byte characters.
    proptest::collection::vec(prop_oneof!["a", "b", "\n", "é", "ț", "中", "😀", "🎉"], 0..40)
        .prop_map(|parts| parts.concat())
}

fn encoding_strategy() -> impl Strategy<Value = Encoding> {
    prop_oneof![Just(Encoding::Utf8), Just(Encoding::Utf16), Just(Encoding::Utf32)]
}

proptest! {
    #[test]
    fn every_offset_round_trips(text in text_strategy(), encoding in encoding_strategy()) {
        let lines = Lines::new(text.clone());
        for (offset, _) in text.char_indices().chain([(text.len(), ' ')]) {
            let position = lines.position(offset, encoding);
            prop_assert_eq!(lines.offset(position, encoding), offset);
        }
    }

    #[test]
    fn utf16_columns_match_the_standard_library(text in text_strategy()) {
        let lines = Lines::new(text.clone());
        for (offset, _) in text.char_indices() {
            let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
            let units = text[line_start..offset].encode_utf16().count();
            prop_assert_eq!(lines.position(offset, Encoding::Utf16).character as usize, units);
        }
    }

    #[test]
    fn a_mirrors_change_applied_gives_the_new_text(
        old in text_strategy(),
        new in text_strategy(),
        encoding in encoding_strategy(),
    ) {
        let mut mirror = Mirror::new(old.clone(), 0);
        match mirror.update(&new, 1, encoding) {
            Some(change) => prop_assert_eq!(applied(&old, &change, encoding), new.clone()),
            None => prop_assert_eq!(&old, &new),
        }
        prop_assert_eq!(mirror.text(), new.as_str());
        // Its lines are those of the new text.
        prop_assert_eq!(mirror.lines(), &Lines::new(new));
    }

    #[test]
    fn edits_one_after_another_keep_the_mirror_in_step(
        start in text_strategy(),
        steps in proptest::collection::vec(text_strategy(), 1..6),
    ) {
        let mut server = start.clone();
        let mut mirror = Mirror::new(start, 0);
        for (i, next) in steps.iter().enumerate() {
            if let Some(change) = mirror.update(next, i as u64 + 1, Encoding::Utf16) {
                server = applied(&server, &change, Encoding::Utf16);
            }
            prop_assert_eq!(&server, next);
        }
    }
}
