//! Text changes: [`Edit`] (one replacement) and [`Transaction`] (several, atomically).

use std::ops::Range;

use kernel::TextPosition;

use super::buffer::TextBuffer;

/// Replaces `range` (bytes, in pre-transaction coordinates) with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

impl Edit {
    pub fn new(range: Range<usize>, text: impl Into<String>) -> Self {
        Self { range, text: text.into() }
    }

    pub fn is_noop(&self) -> bool {
        self.range.is_empty() && self.text.is_empty()
    }

    fn delta(&self) -> isize {
        self.text.len() as isize - self.range.len() as isize
    }
}

/// A change applied to the text, with offsets and positions. A transaction's changes
/// are sequential: each is expressed in the coordinates of the text after the previous ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextChange {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
    pub start_point: TextPosition,
    pub old_end_point: TextPosition,
    pub new_end_point: TextPosition,
}

impl TextChange {
    /// Where `offset` ends up after this change (`Assoc::After` for the replaced zone).
    pub fn map(&self, offset: usize) -> usize {
        if offset < self.start {
            offset
        } else if offset >= self.old_end {
            offset - self.old_end + self.new_end
        } else {
            self.new_end
        }
    }
}

/// How a position exactly at the edge of a change is mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// Stays before the inserted text.
    Before,
    /// Moves after the inserted text (the behavior of a typing cursor).
    After,
}

/// A set of edits applied atomically. The edits are sorted and don't overlap;
/// their ranges are all in the coordinates of the text before the transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transaction {
    edits: Vec<Edit>,
}

impl Transaction {
    /// Sorts the edits; those that would overlap the previous one are ignored.
    pub fn new(mut edits: Vec<Edit>) -> Self {
        edits.retain(|e| !e.is_noop());
        edits.sort_by_key(|e| (e.range.start, e.range.end));
        let mut result: Vec<Edit> = Vec::with_capacity(edits.len());
        for edit in edits {
            if result.last().is_some_and(|prev| edit.range.start < prev.range.end) {
                continue;
            }
            result.push(edit);
        }
        Self { edits: result }
    }

    pub fn single(range: Range<usize>, text: impl Into<String>) -> Self {
        Self::new(vec![Edit::new(range, text)])
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Applies the transaction. Returns its inverse (for undo) and the exact changes, in
    /// application order (for incremental parsing and updating folding).
    pub fn apply(&self, buffer: &mut TextBuffer) -> (Transaction, Vec<TextChange>) {
        let mut inverse = Vec::with_capacity(self.edits.len());
        let mut changes = Vec::with_capacity(self.edits.len());
        let mut delta: isize = 0;
        // In ascending order: each edit is applied to the text already modified by
        // the ones before it, so positions (line, column) are read directly from the buffer.
        for edit in &self.edits {
            let start = (edit.range.start as isize + delta) as usize;
            let old_end = (edit.range.end as isize + delta) as usize;
            let removed = buffer.slice(start..old_end);
            let start_point = buffer.position(start);
            let old_end_point = buffer.position(old_end);
            buffer.replace(start..old_end, &edit.text);
            let new_end = start + edit.text.len();
            changes.push(TextChange {
                start,
                old_end,
                new_end,
                start_point,
                old_end_point,
                new_end_point: buffer.position(new_end),
            });
            inverse.push(Edit::new(start..new_end, removed));
            delta += edit.delta();
        }
        (Transaction { edits: inverse }, changes)
    }

    /// Where `offset` ends up after applying the transaction.
    pub fn map(&self, offset: usize, assoc: Assoc) -> usize {
        let mut delta: isize = 0;
        for edit in &self.edits {
            if offset < edit.range.start || (offset == edit.range.start && assoc == Assoc::Before) {
                break;
            }
            if offset <= edit.range.end {
                let new_start = (edit.range.start as isize + delta) as usize;
                return match assoc {
                    Assoc::Before => new_start,
                    Assoc::After => new_start + edit.text.len(),
                };
            }
            delta += edit.delta();
        }
        (offset as isize + delta).max(0) as usize
    }
}

impl Transaction {
    /// [`Self::map`] for many offsets (each cursor of a multi-cursor edit).
    pub fn offsets(&self) -> OffsetMap<'_> {
        let mut before = Vec::with_capacity(self.edits.len() + 1);
        let mut delta = 0isize;
        before.push(0);
        for edit in &self.edits {
            delta += edit.delta();
            before.push(delta);
        }
        OffsetMap { edits: &self.edits, before }
    }
}

/// [`Transaction::map`] for many offsets: the edits' deltas summed once, each offset found by
/// binary searches, not by a walk over the edits (quadratic with thousands of cursors).
#[derive(Debug)]
pub struct OffsetMap<'a> {
    edits: &'a [Edit],
    /// The deltas of the edits before each one (and of all, last).
    before: Vec<isize>,
}

impl OffsetMap<'_> {
    /// Where `offset` ends up: [`Transaction::map`]'s answer.
    pub fn map(&self, offset: usize, assoc: Assoc) -> usize {
        // The edits the walk passes or stops in: those starting before it (or at it, After).
        let passed = self.edits.partition_point(|e| {
            e.range.start < offset || (e.range.start == offset && assoc == Assoc::After)
        });
        // The first of them it's in (their ends ascend: they're sorted, apart).
        let inside = self.edits[..passed].partition_point(|e| e.range.end < offset);
        if let Some(edit) = self.edits[..passed].get(inside) {
            let new_start = (edit.range.start as isize + self.before[inside]) as usize;
            return match assoc {
                Assoc::Before => new_start,
                Assoc::After => new_start + edit.text.len(),
            };
        }
        (offset as isize + self.before[passed]).max(0) as usize
    }
}

/// Offsets moved through changes applied in turn (each in the text the ones before made, as
/// [`Document::changes_since`](crate::Document::changes_since) gives them): what calling
/// [`TextChange::map`] for each change gives, by binary searches over runs of ascending
/// changes (a transaction's), with the lines the changes before an offset added.
#[derive(Debug, Default)]
pub struct ChangeMap {
    runs: Vec<Run>,
}

/// Changes each after the one before (`start >= new_end`): an offset of the text before them
/// is placed among them by their ends in that text.
#[derive(Debug, Default)]
struct Run {
    changes: Vec<TextChange>,
    /// Each change's old end in the text before the run.
    ends: Vec<usize>,
    /// The bytes and the lines the changes before each one added (and all of them, last).
    bytes: Vec<isize>,
    lines: Vec<i64>,
}

impl ChangeMap {
    pub fn new(changes: &[TextChange]) -> Self {
        let mut runs: Vec<Run> = Vec::new();
        for change in changes {
            let follows = runs
                .last()
                .and_then(|r| r.changes.last())
                .is_some_and(|last| change.start >= last.new_end);
            if !follows {
                runs.push(Run { bytes: vec![0], lines: vec![0], ..Run::default() });
            }
            let Some(run) = runs.last_mut() else { continue };
            let (bytes, lines) = (run.bytes[run.bytes.len() - 1], run.lines[run.lines.len() - 1]);
            run.ends.push((change.old_end as isize - bytes) as usize);
            run.bytes.push(bytes + change.new_end as isize - change.old_end as isize);
            run.lines.push(
                lines + i64::from(change.new_end_point.line.0)
                    - i64::from(change.old_end_point.line.0),
            );
            run.changes.push(*change);
        }
        Self { runs }
    }

    /// Where `offset` ends up, and how many lines the changes wholly before it added (those
    /// ending at it or before, as a fold's lines move).
    pub fn map(&self, offset: usize) -> (usize, i64) {
        let (mut at, mut added) = (offset, 0i64);
        for run in &self.runs {
            let k = run.ends.partition_point(|&end| end <= at);
            added += run.lines[k];
            let current = (at as isize + run.bytes[k]).max(0) as usize;
            match run.changes.get(k) {
                Some(change) if current >= change.start => {
                    // Inside it: at its end, then through the changes right after it.
                    let mut moved = change.new_end;
                    for later in &run.changes[k + 1..] {
                        if moved < later.start {
                            break;
                        }
                        if moved >= later.old_end {
                            added += i64::from(later.new_end_point.line.0)
                                - i64::from(later.old_end_point.line.0);
                        }
                        moved = later.map(moved);
                    }
                    at = moved;
                }
                _ => at = current,
            }
        }
        (at, added)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/edit.rs"]
mod tests;
