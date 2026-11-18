//! A file's changes as hunks, as a staging screen shows them: each with its lines and the
//! unified diff that stages, unstages or throws away just it. Pure: no I/O.

use diff::{Chunk, diff_lines};

/// What a line of a hunk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// A line of a hunk: its number on each side it's on (from 1), its text with its line ending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

/// A run of changes with the lines around them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// Where it starts on each side (0-based) and how many lines it has there.
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

/// As a hunk header counts: from 1, the line before when it has none on that side.
fn header_start(start: usize, count: usize) -> usize {
    if count == 0 { start } else { start + 1 }
}

impl DiffHunk {
    /// The lines it covers in the new version (from 1, first and last).
    pub fn new_lines(&self) -> (usize, usize) {
        let first = header_start(self.new_start, self.new_count);
        (first, first + self.new_count.saturating_sub(1))
    }

    /// The lines to name it by (from 1, first and last): in the new version, or in the old one
    /// when it only removes.
    pub fn shown_lines(&self) -> (usize, usize) {
        if self.new_count > 0 {
            return self.new_lines();
        }
        let first = header_start(self.old_start, self.old_count);
        (first, first + self.old_count.saturating_sub(1))
    }

    /// The unified diff of this hunk alone, on file `path`.
    pub fn patch(&self, path: &str) -> String {
        self.patch_lines(path, |_| true, false)
    }

    /// The unified diff of only the changed lines `chosen` says (by their place in `lines`), on
    /// file `path`. The rest stay as they are on the side the patch is applied to: applied
    /// forward that's the old one, taken back (`reverse`) the new one.
    pub fn patch_lines(&self, path: &str, chosen: impl Fn(usize) -> bool, reverse: bool) -> String {
        let mut body = String::new();
        let (mut old_count, mut new_count) = (0, 0);
        let mut put = |sign: char, text: &str| {
            old_count += usize::from(sign != '+');
            new_count += usize::from(sign != '-');
            body.push(sign);
            body.push_str(text);
            if !text.ends_with('\n') {
                body.push_str("\n\\ No newline at end of file\n");
            }
        };
        let mut at = 0;
        while at < self.lines.len() {
            if self.lines[at].kind == LineKind::Context {
                put(' ', &self.lines[at].text);
                at += 1;
                continue;
            }
            // A run of changes: the n-th removed line and the n-th added one are the same
            // place, so what stays of one lands where the other would have.
            let end = self.lines[at..]
                .iter()
                .position(|line| line.kind == LineKind::Context)
                .map_or(self.lines.len(), |n| at + n);
            let side = |kind| (at..end).filter(move |i| self.lines[*i].kind == kind);
            let removed: Vec<usize> = side(LineKind::Removed).collect();
            let added: Vec<usize> = side(LineKind::Added).collect();
            for n in 0..removed.len().max(added.len()) {
                if let Some(&i) = removed.get(n) {
                    match (chosen(i), reverse) {
                        (true, _) => put('-', &self.lines[i].text),
                        (false, false) => put(' ', &self.lines[i].text),
                        (false, true) => {}
                    }
                }
                if let Some(&i) = added.get(n) {
                    match (chosen(i), reverse) {
                        (true, _) => put('+', &self.lines[i].text),
                        (false, true) => put(' ', &self.lines[i].text),
                        (false, false) => {}
                    }
                }
            }
            at = end;
        }
        format!(
            "--- a/{path}\n+++ b/{path}\n@@ -{},{old_count} +{},{new_count} @@\n{body}",
            header_start(self.old_start, old_count),
            header_start(self.new_start, new_count),
        )
    }
}

/// The changes from `old` to `new` as hunks, with `context` lines around each change (changes
/// closer than twice that are one hunk).
pub fn hunks(old: &str, new: &str, context: usize) -> Vec<DiffHunk> {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let changes: Vec<_> = diff_lines(&a, &b)
        .into_iter()
        .filter_map(|chunk| match chunk {
            Chunk::Change { left, right } => Some((left, right)),
            Chunk::Equal { .. } => None,
        })
        .collect();
    let mut groups: Vec<&[_]> = Vec::new();
    let mut from = 0;
    for i in 0..changes.len() {
        let last = i + 1 == changes.len();
        if last || changes[i + 1].0.start - changes[i].0.end > 2 * context {
            groups.push(&changes[from..=i]);
            from = i + 1;
        }
    }
    groups.into_iter().map(|group| hunk(&a, &b, group, context)).collect()
}

type Change = (std::ops::Range<usize>, std::ops::Range<usize>);

fn hunk(a: &[&str], b: &[&str], group: &[Change], context: usize) -> DiffHunk {
    let (first, last) = (&group[0], &group[group.len() - 1]);
    let before = context.min(first.0.start).min(first.1.start);
    let after = context.min(a.len() - last.0.end).min(b.len() - last.1.end);
    let (old_start, new_start) = (first.0.start - before, first.1.start - before);
    let (old_end, new_end) = (last.0.end + after, last.1.end + after);
    let number = |at: usize| u32::try_from(at + 1).unwrap_or(u32::MAX);
    let mut lines = Vec::new();
    let same = |lines: &mut Vec<DiffLine>, old: usize, new: usize, count: usize| {
        for i in 0..count {
            lines.push(DiffLine {
                kind: LineKind::Context,
                old: Some(number(old + i)),
                new: Some(number(new + i)),
                text: a[old + i].to_owned(),
            });
        }
    };
    let (mut old_at, mut new_at) = (old_start, new_start);
    for (left, right) in group {
        same(&mut lines, old_at, new_at, left.start - old_at);
        lines.extend(left.clone().map(|i| DiffLine {
            kind: LineKind::Removed,
            old: Some(number(i)),
            new: None,
            text: a[i].to_owned(),
        }));
        lines.extend(right.clone().map(|i| DiffLine {
            kind: LineKind::Added,
            old: None,
            new: Some(number(i)),
            text: b[i].to_owned(),
        }));
        (old_at, new_at) = (left.end, right.end);
    }
    same(&mut lines, old_at, new_at, old_end - old_at);
    DiffHunk {
        old_start,
        old_count: old_end - old_start,
        new_start,
        new_count: new_end - new_start,
        lines,
    }
}

#[cfg(test)]
#[path = "../tests/unit/staging.rs"]
mod tests;
