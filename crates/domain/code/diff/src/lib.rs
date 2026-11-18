//! Diff bounded context: the differences between two texts, by line (Myers algorithm,
//! O((N + M)·D) time, linear space), with modified lines paired and the differences within them; then
//! composing a new text, choosing left, right or both for each difference
//! (as when resolving a merge conflict); and merging two versions from their common
//! ancestor ([`merge3`]).

use std::ops::Range;
use std::path::PathBuf;

mod merge3;

pub use merge3::{MergeChunks, Region, RegionKind, merge3};

/// A diff chunk: equal lines or a difference (lines from the left replaced by
/// lines from the right; either side may be empty: addition / deletion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    Equal { left: Range<usize>, right: Range<usize> },
    Change { left: Range<usize>, right: Range<usize> },
}

impl Chunk {
    pub fn left(&self) -> Range<usize> {
        match self {
            Self::Equal { left, .. } | Self::Change { left, .. } => left.clone(),
        }
    }

    pub fn right(&self) -> Range<usize> {
        match self {
            Self::Equal { right, .. } | Self::Change { right, .. } => right.clone(),
        }
    }

    pub fn is_change(&self) -> bool {
        matches!(self, Self::Change { .. })
    }
}

/// At most how many operations Myers does before giving up on a minimal diff (very
/// different files): the rest becomes a single difference.
const MAX_EDIT: usize = 20_000;

/// The differences between lines `a` and `b`: a shortest edit (Myers), found in linear
/// space (the middle snake, each half again: memory as the lines, not as lines × edits —
/// two 10,000-line files much changed took 2 GB the other way), the lines compared as
/// numbers (each distinct text one).
pub fn diff_lines<S: AsRef<str>>(a: &[S], b: &[S]) -> Vec<Chunk> {
    let (a, b) = numbered(a, b);
    let mut out = Vec::new();
    differ(&a, &b, (0, 0), &mut out);
    merge(out)
}

/// Each line as a number, the same for the same text (on both sides).
fn numbered<'a, S: AsRef<str>>(a: &'a [S], b: &'a [S]) -> (Vec<u32>, Vec<u32>) {
    let mut ids: std::collections::HashMap<&'a str, u32> = std::collections::HashMap::new();
    let mut number = |lines: &'a [S]| -> Vec<u32> {
        lines
            .iter()
            .map(|line| {
                let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                *ids.entry(line.as_ref()).or_insert(next)
            })
            .collect()
    };
    (number(a), number(b))
}

/// The chunks of `a` against `b` (lines `at` of the whole), into `out`: the common prefix
/// and suffix, and between them the halves on each side of the middle snake. Sides that
/// differ by more than [`MAX_EDIT`] edits are one difference.
fn differ(a: &[u32], b: &[u32], at: (usize, usize), out: &mut Vec<Chunk>) {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix =
        a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    if prefix > 0 {
        out.push(Chunk::Equal { left: at.0..at.0 + prefix, right: at.1..at.1 + prefix });
    }
    let (ma, mb) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let (ia, ib) = (at.0 + prefix, at.1 + prefix);
    let whole = Chunk::Change { left: ia..ia + ma.len(), right: ib..ib + mb.len() };
    if ma.is_empty() || mb.is_empty() {
        out.push(whole);
    } else {
        match middle(ma, mb) {
            Some((x, y)) if (x, y) != (0, 0) && (x, y) != (ma.len(), mb.len()) => {
                differ(&ma[..x], &mb[..y], (ia, ib), out);
                differ(&ma[x..], &mb[y..], (ia + x, ib + y), out);
            }
            _ => out.push(whole),
        }
    }
    if suffix > 0 {
        let (sa, sb) = (at.0 + a.len() - suffix, at.1 + b.len() - suffix);
        out.push(Chunk::Equal { left: sa..sa + suffix, right: sb..sb + suffix });
    }
}

/// Where a shortest edit of `a` into `b` crosses its middle: searched from both ends at
/// once until the two paths meet (Myers' linear-space refinement). `None`: they differ by
/// more than [`MAX_EDIT`] edits (each path goes half of them).
fn middle(a: &[u32], b: &[u32]) -> Option<(usize, usize)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m + 1) / 2;
    let offset = max;
    let len = (2 * max + 2) as usize;
    let (mut forward, mut backward) = (vec![-1isize; len], vec![-1isize; len]);
    forward[(offset + 1) as usize] = 0;
    backward[(offset + 1) as usize] = 0;
    let delta = n - m;
    // An odd delta: the forward path finds the meeting; even: the backward one.
    let front = delta % 2 != 0;
    let (mut k1start, mut k1end, mut k2start, mut k2end) = (0, 0, 0, 0);
    for d in 0..max.min(MAX_EDIT.div_ceil(2) as isize) {
        let mut k1 = -d + k1start;
        while k1 <= d - k1end {
            let i = (offset + k1) as usize;
            let mut x1 = if k1 == -d || (k1 != d && forward[i - 1] < forward[i + 1]) {
                forward[i + 1]
            } else {
                forward[i - 1] + 1
            };
            let mut y1 = x1 - k1;
            while x1 < n && y1 < m && a[x1 as usize] == b[y1 as usize] {
                x1 += 1;
                y1 += 1;
            }
            forward[i] = x1;
            if x1 > n {
                k1end += 2;
            } else if y1 > m {
                k1start += 2;
            } else if front {
                let j = offset + delta - k1;
                if (0..len as isize).contains(&j) && backward[j as usize] != -1 {
                    let x2 = n - backward[j as usize];
                    if x1 >= x2 {
                        return Some((x1 as usize, y1 as usize));
                    }
                }
            }
            k1 += 2;
        }
        let mut k2 = -d + k2start;
        while k2 <= d - k2end {
            let i = (offset + k2) as usize;
            let mut x2 = if k2 == -d || (k2 != d && backward[i - 1] < backward[i + 1]) {
                backward[i + 1]
            } else {
                backward[i - 1] + 1
            };
            let mut y2 = x2 - k2;
            while x2 < n && y2 < m && a[(n - x2 - 1) as usize] == b[(m - y2 - 1) as usize] {
                x2 += 1;
                y2 += 1;
            }
            backward[i] = x2;
            if x2 > n {
                k2end += 2;
            } else if y2 > m {
                k2start += 2;
            } else if !front {
                let j = offset + delta - k2;
                if (0..len as isize).contains(&j) && forward[j as usize] != -1 {
                    let x1 = forward[j as usize];
                    let y1 = offset + x1 - j;
                    if x1 >= n - x2 {
                        return Some((x1 as usize, y1 as usize));
                    }
                }
            }
            k2 += 2;
        }
    }
    None
}

/// Merges adjacent chunks of the same kind.
fn merge(chunks: Vec<Chunk>) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    for c in chunks {
        if c.left().is_empty() && c.right().is_empty() {
            continue;
        }
        match (out.last_mut(), &c) {
            (Some(Chunk::Equal { left, right }), Chunk::Equal { left: l, right: r })
            | (Some(Chunk::Change { left, right }), Chunk::Change { left: l, right: r }) => {
                left.end = l.end;
                right.end = r.end;
            }
            _ => out.push(c),
        }
    }
    out
}

/// The difference within a modified line: the ranges (bytes) that differ on the
/// left and on the right, after the common prefix and suffix.
pub fn inline_change(left: &str, right: &str) -> (Range<usize>, Range<usize>) {
    let prefix: usize = left
        .char_indices()
        .zip(right.chars())
        .find(|((_, a), b)| a != b)
        .map_or(left.len().min(right.len()), |((i, _), _)| i);
    let max = (left.len() - prefix).min(right.len() - prefix);
    let mut suffix =
        left.bytes().rev().zip(right.bytes().rev()).take(max).take_while(|(a, b)| a == b).count();
    while !left.is_char_boundary(left.len() - suffix)
        || !right.is_char_boundary(right.len() - suffix)
    {
        suffix -= 1;
    }
    (prefix..left.len() - suffix, prefix..right.len() - suffix)
}

/// What is taken from a difference in the composed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pick {
    #[default]
    Left,
    Right,
    /// Left first, then right.
    Both,
    /// Nothing (both are deleted).
    None,
}

/// The composed text: the equal parts, and at each difference, what was chosen (`picks[i]` for
/// the `i`-th difference; missing = left).
pub fn compose<S: AsRef<str>>(a: &[S], b: &[S], chunks: &[Chunk], picks: &[Pick]) -> Vec<String> {
    let mut out = Vec::new();
    let mut change = 0;
    for chunk in chunks {
        match chunk {
            Chunk::Equal { left, .. } => {
                out.extend(a[left.clone()].iter().map(|s| s.as_ref().to_owned()))
            }
            Chunk::Change { left, right } => {
                let pick = picks.get(change).copied().unwrap_or_default();
                change += 1;
                if matches!(pick, Pick::Left | Pick::Both) {
                    out.extend(a[left.clone()].iter().map(|s| s.as_ref().to_owned()));
                }
                if matches!(pick, Pick::Right | Pick::Both) {
                    out.extend(b[right.clone()].iter().map(|s| s.as_ref().to_owned()));
                }
            }
        }
    }
    out
}

/// The extension of the view that shows a [`CompareInput`] (a virtual tab's): the compare
/// module's viewer opens it, whoever asks for a comparison.
pub const COMPARE_VIEW: &str = "view-diff";

/// What is compared: two files (the path and text of each, as in the editor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareInput {
    pub left: (PathBuf, String),
    pub right: (PathBuf, String),
    /// What each side is called, instead of its file name (e.g. `A.php · HEAD`).
    pub labels: [Option<String>; 2],
    /// A side that can't be written (a version from git): results go to the other one.
    pub read_only: [bool; 2],
}

impl CompareInput {
    /// Two files, both writable, named after their files.
    pub fn files(left: (PathBuf, String), right: (PathBuf, String)) -> Self {
        Self { left, right, labels: [None, None], read_only: [false, false] }
    }
}

/// A file to resolve in three parts: ours on the left and theirs on the right (read-only),
/// the result in the middle, written to `result`. What one side changed alone is taken
/// already; what both changed differently waits for a choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeInput {
    /// The file written (the conflicted one).
    pub result: PathBuf,
    /// The common ancestor's text (empty when both sides added the file).
    pub base: String,
    pub ours: String,
    pub theirs: String,
    /// What each side is called (`HEAD`, the commit being replayed).
    pub labels: [String; 2],
}

#[cfg(test)]
#[path = "../tests/unit/merge3.rs"]
mod merge3_tests;

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
