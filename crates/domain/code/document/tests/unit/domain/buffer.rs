use super::*;

#[test]
fn snapshot_slices_align_to_characters() {
    // "ț" is 2 bytes: an end in its middle must not panic.
    let snap = TextBuffer::new("aț b").snapshot();
    assert_eq!(snap.slice(0..2), "a");
    assert_eq!(snap.slice(2..5), "ț b");
    assert_eq!(snap.slice(0..100), "aț b");
}

#[test]
fn lines_and_offsets() {
    let b = TextBuffer::new("ab\nșt\n");
    assert_eq!(b.line_count(), 3);
    assert_eq!(b.line_text(1), "șt");
    assert_eq!(b.line_len(1), 3); // `ș` is 2 bytes
    assert_eq!(b.line_start(1), 3);
    assert_eq!(b.line_end(1), 6);
    assert_eq!(b.line_of(4), 1);
    assert_eq!(b.line_text(2), "");
    assert_eq!(b.position(5), TextPosition::new(1, 2));
    assert_eq!(b.offset(TextPosition::new(1, 99)), 6);
}

#[test]
fn replace_and_chars() {
    let mut b = TextBuffer::new("hello world");
    b.replace(0..5, "salut");
    b.replace(11..11, "!");
    assert_eq!(b.to_string(), "salut world!");
    assert_eq!(b.char_before(5), Some('t'));
    assert_eq!(b.char_after(5), Some(' '));
    assert_eq!(b.char_before(0), None);
}

#[test]
fn snapshot_chunks_and_positions() {
    let b = TextBuffer::new("ab\ncd");
    let snap = b.snapshot();
    assert_eq!(snap.chunk_from(3), b"cd");
    assert_eq!(snap.chunk_from(99), b"");
    // Inside a multi-byte character: its remaining bytes, no panic.
    let wide = TextBuffer::new("aș").snapshot();
    assert_eq!(wide.chunk_from(2), &"ș".as_bytes()[1..]);
    assert_eq!(snap.slice(1..4), "b\nc");
    assert_eq!(snap.position(4), TextPosition::new(1, 1));
}

#[test]
fn snapshot_is_independent() {
    let mut b = TextBuffer::new("a");
    let snap = b.snapshot();
    b.replace(0..1, "bc");
    assert_eq!(snap.len(), 1);
    assert_eq!(b.len(), 2);
}

/// The whole text, in a string of exactly its size (across the rope's chunks).
#[test]
fn text_is_copied_at_its_exact_size() {
    let source: String = (0..5000).map(|i| format!("line {i} ăîș\n")).collect();
    let mut buffer = TextBuffer::new(&source);
    buffer.replace(10..10, "inserted ");
    let expected = buffer.to_string();
    let text = buffer.text();
    assert_eq!(text, expected);
    assert_eq!(text.capacity(), text.len());
}

/// Searching over the chunks finds what searching the whole text does: across chunk ends,
/// apart, from any offset (inside a character too), with characters beyond ASCII.
#[test]
fn find_over_the_chunks_is_the_whole_text_s() {
    let text: String =
        (0..6000).map(|i| ["ab", "ș", "aba", "\n", "x", "ăbab"][i * 7 % 6]).collect();
    let buffer = TextBuffer::new(&text);
    assert!(buffer.chunks().count() > 5, "several chunks");
    for needle in ["ab", "aba", "bab", "ș", "șx", "\nx", "ăbabab", "zz", "a"] {
        let all: Vec<usize> = text.match_indices(needle).map(|(i, _)| i).collect();
        assert_eq!(buffer.find_all(needle), all, "{needle:?}");
        for from in (0..text.len()).step_by(997).chain([text.len()]) {
            let floor = buffer.floor_char_boundary(from);
            let expected = text[floor..].find(needle).map(|i| floor + i);
            assert_eq!(buffer.find(needle, from), expected, "{needle:?} from {from}");
        }
    }
    assert!(buffer.find("", 0).is_none() && buffer.find_all("").is_empty());
    assert_eq!(buffer.find("ab", text.len() + 5), None);
}

/// A range kept from an older version (a statement's mark, a symbol's) may end inside a
/// character now: sliced from the char's start, never a panic.
#[test]
fn a_stale_range_is_sliced_on_char_boundaries() {
    let b = TextBuffer::new("ăSELECT 'ș'");
    // `ă` and `ș` are 2 bytes: ends inside each.
    assert_eq!(b.slice(1..2), "ă", "a start inside `ă`: from its start");
    assert_eq!(b.slice(1..3), "ăS");
    assert_eq!(b.slice(0..11), "ăSELECT '");
    assert_eq!(b.slice(0..100), "ăSELECT 'ș'");
    let (start, end) = (11, 3);
    assert_eq!(b.slice(start..end), "", "a range turned around");
}
