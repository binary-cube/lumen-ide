use super::*;

fn lines(s: &str) -> Vec<&str> {
    s.lines().collect()
}

#[test]
fn finds_changes_additions_and_deletions() {
    let a = lines("a\nb\nc\nd\ne");
    let b = lines("a\nB\nc\ne\nf");
    let chunks = diff_lines(&a, &b);
    assert_eq!(
        chunks,
        [
            Chunk::Equal { left: 0..1, right: 0..1 },
            Chunk::Change { left: 1..2, right: 1..2 },
            Chunk::Equal { left: 2..3, right: 2..3 },
            Chunk::Change { left: 3..4, right: 3..3 },
            Chunk::Equal { left: 4..5, right: 3..4 },
            Chunk::Change { left: 5..5, right: 4..5 },
        ]
    );
    // Composing: left, right, both.
    let picks = [Pick::Right, Pick::Left, Pick::Right];
    assert_eq!(compose(&a, &b, &chunks, &picks), ["a", "B", "c", "d", "e", "f"]);
    assert_eq!(compose(&a, &b, &chunks, &[]), a);
    assert_eq!(compose(&a, &b, &chunks, &[Pick::Right; 3]), b);
}

#[test]
fn identical_and_empty() {
    let a = lines("x\ny");
    assert_eq!(diff_lines(&a, &a), [Chunk::Equal { left: 0..2, right: 0..2 }]);
    let empty: Vec<&str> = Vec::new();
    assert_eq!(diff_lines(&empty, &a), [Chunk::Change { left: 0..0, right: 0..2 }]);
}

#[test]
fn inline_differences() {
    assert_eq!(inline_change("color: red;", "color: blue;"), (7..10, 7..11));
    assert_eq!(inline_change("ăb", "ăc"), (2..3, 2..3));
}

/// The chunks cover both sides in order, equal ones are equal, and the lines changed are as
/// few as can be (checked against the longest common subsequence) — on many small random
/// pairs, few distinct lines so that there are many ways to align them.
#[test]
fn the_diff_is_whole_true_and_shortest() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _ in 0..400 {
        let a: Vec<String> = (0..next(30)).map(|_| next(4).to_string()).collect();
        let b: Vec<String> = (0..next(30)).map(|_| next(4).to_string()).collect();
        let chunks = diff_lines(&a, &b);
        let (mut l, mut r, mut kept) = (0, 0, 0);
        for c in &chunks {
            assert_eq!((c.left().start, c.right().start), (l, r), "contiguous: {chunks:?}");
            if let Chunk::Equal { left, right } = c {
                assert_eq!(a[left.clone()], b[right.clone()]);
                kept += left.len();
            }
            (l, r) = (c.left().end, c.right().end);
        }
        assert_eq!((l, r), (a.len(), b.len()), "whole");
        let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i][j] = if a[i] == b[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        assert_eq!(kept, lcs[0][0], "shortest: {a:?} {b:?}");
    }
}

/// Two 10,000-line files, every other line changed: found (each change its own), in linear
/// memory (the trace of every step took 2 GB here).
#[test]
fn big_files_much_changed_are_diffed_in_little_memory() {
    let a: Vec<String> = (0..10_000).map(|i| format!("line {i}")).collect();
    let b: Vec<String> = (0..10_000)
        .map(|i| if i % 2 == 0 { format!("line {i}") } else { format!("new {i}") })
        .collect();
    let chunks = diff_lines(&a, &b);
    let changes = chunks.iter().filter(|c| c.is_change()).count();
    assert_eq!(changes, 5_000);
}
