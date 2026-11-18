use super::*;
use proptest::prelude::*;

#[test]
fn multiple_edits_use_original_coordinates() {
    let mut b = TextBuffer::new("aaa bbb ccc");
    let tx = Transaction::new(vec![Edit::new(8..11, "Z"), Edit::new(0..3, "XX")]);
    let (inverse, _) = tx.apply(&mut b);
    assert_eq!(b.to_string(), "XX bbb Z");
    inverse.apply(&mut b);
    assert_eq!(b.to_string(), "aaa bbb ccc");
}

#[test]
fn changes_are_sequential_with_positions() {
    let mut b = TextBuffer::new("ab\ncd\nef");
    let tx = Transaction::new(vec![Edit::new(1..1, "X\nY"), Edit::new(6..7, "ZZ")]);
    let (_, changes) = tx.apply(&mut b);
    assert_eq!(b.to_string(), "aX\nYb\ncd\nZZf");
    assert_eq!(changes[0].start, 1);
    assert_eq!(changes[0].new_end, 4);
    assert_eq!(changes[0].new_end_point, TextPosition::new(1, 1));
    // The second edit is expressed after the first (offset 6 becomes 9).
    assert_eq!((changes[1].start, changes[1].old_end, changes[1].new_end), (9, 10, 11));
    assert_eq!(changes[1].start_point, TextPosition::new(3, 0));
    assert_eq!(changes[1].map(20), 21);
    assert_eq!(changes[1].map(5), 5);
}

#[test]
fn overlapping_edits_are_dropped() {
    let tx = Transaction::new(vec![Edit::new(0..5, "a"), Edit::new(3..6, "b")]);
    assert_eq!(tx.edits().len(), 1);
}

#[test]
fn map_positions() {
    let tx = Transaction::new(vec![Edit::new(2..2, "xyz"), Edit::new(5..7, "")]);
    assert_eq!(tx.map(0, Assoc::After), 0);
    assert_eq!(tx.map(2, Assoc::Before), 2);
    assert_eq!(tx.map(2, Assoc::After), 5);
    assert_eq!(tx.map(4, Assoc::After), 7);
    assert_eq!(tx.map(6, Assoc::After), 8); // inside the deletion
    assert_eq!(tx.map(9, Assoc::After), 10);
}

fn edits_strategy(len: usize) -> impl Strategy<Value = Vec<(usize, usize, String)>> {
    prop::collection::vec((0..=len, 0..=len, "[a-zăș\\n]{0,4}"), 0..6)
}

proptest! {
    #[test]
    fn apply_then_inverse_restores_text(text in "[a-zăîș \\n]{0,40}", raw in edits_strategy(40)) {
        let mut buffer = TextBuffer::new(&text);
        let boundaries: Vec<usize> = (0..=text.len()).filter(|&i| text.is_char_boundary(i)).collect();
        let snap = |i: usize| *boundaries.iter().min_by_key(|&&b| b.abs_diff(i)).unwrap_or(&0);
        let edits = raw.into_iter().map(|(a, b, t)| {
            let (a, b) = (snap(a.min(text.len())), snap(b.min(text.len())));
            Edit::new(a.min(b)..a.max(b), t)
        }).collect();
        let tx = Transaction::new(edits);
        let (inverse, _) = tx.apply(&mut buffer);
        let after = buffer.to_string();
        let (redo, _) = inverse.apply(&mut buffer);
        prop_assert_eq!(buffer.to_string(), text);
        redo.apply(&mut buffer);
        prop_assert_eq!(buffer.to_string(), after);
    }
}

/// A small generator of edits (deterministic).
struct Seed(u64);

impl Seed {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize % n.max(1)
    }

    /// Up to `count` sorted, apart edits over a text of `len` bytes (ASCII).
    fn edits(&mut self, len: usize, count: usize) -> Vec<Edit> {
        let pieces = ["", "a", "\n", "bc\nd", "\n\n"];
        let mut edits = Vec::new();
        let mut at = 0;
        for _ in 0..count {
            let start = (at + self.next(8)).min(len);
            let end = (start + self.next(4)).min(len);
            edits.push(Edit::new(start..end, pieces[self.next(pieces.len())]));
            at = end + self.next(2);
            if at > len {
                break;
            }
        }
        edits
    }
}

/// Many offsets through a transaction: the binary searches give the walk's answers.
#[test]
fn offsets_map_as_the_transaction_does() {
    let mut seed = Seed(5);
    for _ in 0..200 {
        let len = 200;
        let count = 1 + seed.next(40);
        let tx = Transaction::new(seed.edits(len, count));
        let offsets = tx.offsets();
        for offset in 0..=len {
            for assoc in [Assoc::Before, Assoc::After] {
                assert_eq!(offsets.map(offset, assoc), tx.map(offset, assoc), "{offset} {assoc:?}");
            }
        }
    }
}

/// Offsets through changes applied in turn (several transactions' worth, as a parse worker
/// gets them): the change map gives what mapping through each change in turn gives, and
/// the lines a fold starting there moves by.
#[test]
fn a_change_map_maps_as_the_changes_in_turn() {
    let mut seed = Seed(11);
    for _ in 0..150 {
        let mut buffer = TextBuffer::new(&"line of text\n".repeat(20));
        let mut changes = Vec::new();
        for _ in 0..1 + seed.next(3) {
            let count = 1 + seed.next(30);
            let tx = Transaction::new(seed.edits(buffer.len(), count));
            changes.extend(tx.apply(&mut buffer).1);
        }
        let map = ChangeMap::new(&changes);
        for offset in 0..=260 {
            let (mut at, mut lines) = (offset, 0i64);
            for c in &changes {
                if at >= c.old_end {
                    lines += i64::from(c.new_end_point.line.0) - i64::from(c.old_end_point.line.0);
                }
                at = c.map(at);
            }
            assert_eq!(map.map(offset), (at, lines), "offset {offset}");
        }
    }
}
