//! An editor's folding state: which regions are collapsed and which lines are visible.
//!
//! The available regions come from the syntax engine (for any language, including the
//! embedded ones). A collapsed region is anchored at its start offset, so it
//! stays collapsed after edits above it.

use std::collections::HashSet;

use document::{Selection, Selections, TextChange};
use languages::FoldRange;

/// More changes than this are mapped with a [`document::ChangeMap`].
const MANY_CHANGES: usize = 16;

#[derive(Debug, Default, Clone)]
pub struct Folds {
    available: Vec<FoldRange>,
    /// The anchors (start offset) of the collapsed regions.
    collapsed: Vec<usize>,
    /// The hidden line ranges `[first, last]`, sorted, non-overlapping.
    hidden: Vec<(u32, u32)>,
    /// How many lines the ranges in `hidden` hide in total.
    hidden_total: u32,
    /// How many lines the ranges before each of `hidden` hide (one more: all of them): a
    /// row's line and a line's row found by a binary search, asked for every row shown.
    hidden_before: Vec<u32>,
    /// (start line, index in `available`), sorted by line: `fold_at` is called
    /// for every visible row, on every frame and mouse move.
    by_line: Vec<(u32, usize)>,
    /// Bumped each time the hidden lines change (what is cached from them follows it).
    generation: u64,
}

impl Folds {
    /// The new regions from the syntax engine. Collapsed regions that no longer exist
    /// (e.g. the block was deleted) are forgotten.
    pub fn set_available(&mut self, folds: &[FoldRange]) {
        self.available.clear();
        self.available.extend_from_slice(folds);
        // Looked up in a set: after Collapse All, thousands of anchors against as many
        // regions, at every parse.
        if !self.collapsed.is_empty() {
            let starts: HashSet<usize> = folds.iter().map(|f| f.start_byte).collect();
            self.collapsed.retain(|a| starts.contains(a));
        }
        // On the same line, the first region in the list wins (as before, with `find`).
        self.by_line.clear();
        self.by_line.extend(self.available.iter().enumerate().map(|(i, f)| (f.start_line, i)));
        self.by_line.sort_by_key(|&(line, i)| (line, i));
        self.by_line.dedup_by_key(|&mut (line, _)| line);
        self.rebuild();
    }

    /// The anchors follow the text after edits (until the new regions arrive).
    pub fn map_changes(&mut self, changes: &[TextChange]) {
        // Many changes (a multi-cursor edit): each anchor placed among them at once, not
        // moved by each in turn (changes × regions).
        if changes.len() > MANY_CHANGES {
            let map = document::ChangeMap::new(changes);
            for anchor in &mut self.collapsed {
                *anchor = map.map(*anchor).0;
            }
            for fold in &mut self.available {
                fold.start_byte = map.map(fold.start_byte).0;
                fold.end_byte = map.map(fold.end_byte).0.max(fold.start_byte);
            }
            return;
        }
        for change in changes {
            for anchor in &mut self.collapsed {
                *anchor = change.map(*anchor);
            }
            for fold in &mut self.available {
                if fold.start_byte >= change.start {
                    fold.start_byte = change.map(fold.start_byte);
                }
                fold.end_byte = change.map(fold.end_byte).max(fold.start_byte);
            }
        }
    }

    /// Changes whenever the hidden lines change (and only then).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The hidden line ranges `[first, last]`, sorted, non-overlapping.
    pub fn hidden_ranges(&self) -> &[(u32, u32)] {
        &self.hidden
    }

    /// The lines `[first, last]` each collapsed region shows as one row (its first line and
    /// those it hides), sorted, non-overlapping.
    pub fn folded_lines(&self) -> Vec<(usize, usize)> {
        self.hidden.iter().map(|&(a, b)| (a.saturating_sub(1) as usize, b as usize)).collect()
    }

    /// The selections taking the collapsed regions they reach whole: one that
    /// gets to the end of a folded row's first line (where its `⋯` is) goes on to the end
    /// of its last line; an empty one stays.
    pub fn widen(&self, buffer: &document::TextBuffer, selections: &Selections) -> Selections {
        if self.hidden.is_empty() {
            return selections.clone();
        }
        let rows = self.folded_lines();
        selections.transform(|s| {
            if s.is_empty() {
                return *s;
            }
            let r = s.range();
            let (mut start, mut end) = (r.start, r.end);
            let first = rows.partition_point(|&(_, last)| buffer.line_end(last) < start);
            for &(first, last) in &rows[first..] {
                let (tail, row_end) = (buffer.line_end(first), buffer.line_end(last));
                if tail > end {
                    break;
                }
                // Started inside what's hidden: from where the row's `⋯` is.
                if start > tail && start < row_end {
                    start = tail;
                }
                if start <= tail && end < row_end {
                    end = row_end;
                }
            }
            if s.head >= s.anchor {
                Selection { anchor: start, head: end, goal: s.goal }
            } else {
                Selection { anchor: end, head: start, goal: s.goal }
            }
        })
    }

    pub fn has_any(&self) -> bool {
        !self.available.is_empty()
    }

    /// The region that starts on the given line.
    pub fn fold_at(&self, line: u32) -> Option<&FoldRange> {
        let i = self.by_line.binary_search_by_key(&line, |&(l, _)| l).ok()?;
        self.available.get(self.by_line[i].1)
    }

    pub fn is_collapsed(&self, fold: &FoldRange) -> bool {
        self.collapsed.contains(&fold.start_byte)
    }

    /// Collapses / expands the region starting on `line`. `false` if there is none.
    pub fn toggle(&mut self, line: u32) -> bool {
        let Some(fold) = self.fold_at(line) else { return false };
        let anchor = fold.start_byte;
        match self.collapsed.iter().position(|a| *a == anchor) {
            Some(i) => {
                self.collapsed.swap_remove(i);
            }
            None => self.collapsed.push(anchor),
        }
        self.rebuild();
        true
    }

    /// The innermost region spanning more than one line around `line` (its block: the
    /// highlighted indent guide).
    pub fn innermost(&self, line: u32) -> Option<&FoldRange> {
        self.available
            .iter()
            .filter(|f| f.start_line <= line && line <= f.end_line && f.start_line < f.end_line)
            .min_by_key(|f| f.end_line - f.start_line)
    }

    /// Collapses the innermost region containing the line (the "Fold" command).
    pub fn collapse_around(&mut self, line: u32) -> bool {
        let candidate = self
            .available
            .iter()
            .filter(|f| {
                f.start_line <= line
                    && line <= f.end_line
                    && !self.collapsed.contains(&f.start_byte)
            })
            .min_by_key(|f| f.end_line - f.start_line)
            .map(|f| f.start_byte);
        match candidate {
            Some(anchor) => {
                self.collapsed.push(anchor);
                self.rebuild();
                true
            }
            None => false,
        }
    }

    /// Expands the collapsed regions that contain the line or start on it.
    pub fn expand_around(&mut self, line: u32) -> bool {
        let before = self.collapsed.len();
        let available = &self.available;
        self.collapsed.retain(|a| {
            !available
                .iter()
                .any(|f| f.start_byte == *a && f.start_line <= line && line <= f.end_line)
        });
        let changed = self.collapsed.len() != before;
        if changed {
            self.rebuild();
        }
        changed
    }

    /// Expands the regions hiding the line (e.g. the cursor got there).
    pub fn reveal(&mut self, line: u32) -> bool {
        if !self.is_hidden(line) {
            return false;
        }
        let available = &self.available;
        self.collapsed.retain(|a| {
            !available
                .iter()
                .any(|f| f.start_byte == *a && f.start_line < line && line <= f.end_line)
        });
        self.rebuild();
        true
    }

    pub fn collapse_all(&mut self) {
        self.collapsed = self.available.iter().map(|f| f.start_byte).collect();
        self.rebuild();
    }

    pub fn expand_all(&mut self) {
        self.collapsed.clear();
        if !self.hidden.is_empty() {
            self.hidden.clear();
            self.generation += 1;
        }
        self.hidden_total = 0;
        self.hidden_before.clear();
    }

    pub fn is_hidden(&self, line: u32) -> bool {
        let i = self.hidden.partition_point(|(_, end)| *end < line);
        self.hidden.get(i).is_some_and(|(start, _)| *start <= line)
    }

    /// The line shown for `line`: itself, or for a hidden one the line its collapsed region
    /// starts on (where the cursor goes when its region collapses).
    pub fn shown_line(&self, line: u32) -> u32 {
        let i = self.hidden.partition_point(|(_, end)| *end < line);
        match self.hidden.get(i) {
            Some(&(start, _)) if start <= line => start.saturating_sub(1),
            _ => line,
        }
    }

    /// How many rows are displayed for a document with `lines` lines.
    pub fn visible_rows(&self, lines: u32) -> u32 {
        lines.saturating_sub(self.hidden_total)
    }

    /// The displayed row of a visible line (for a hidden line: the region's row).
    pub fn row_of(&self, line: u32) -> u32 {
        // The ranges wholly above it, then the one it's in (if hidden).
        let i = self.hidden.partition_point(|(_, b)| *b < line);
        let row = line.saturating_sub(self.before(i));
        match self.hidden.get(i) {
            Some(&(a, _)) if a <= line => row.saturating_sub(line - a + 1),
            _ => row,
        }
    }

    /// The document line displayed on row `row`.
    pub fn line_of(&self, row: u32) -> u32 {
        // The ranges starting at or before the row's line: each one's start, less what those
        // above it hide, is the row it would be shown on (growing from one range to the next).
        let (mut low, mut high) = (0, self.hidden.len());
        while low < high {
            let mid = low + (high - low) / 2;
            if self.hidden[mid].0.saturating_sub(self.before(mid)) <= row {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        row + self.before(low)
    }

    /// The lines hidden by the ranges before range `i` (all of them past the end).
    fn before(&self, i: usize) -> u32 {
        self.hidden_before.get(i).or(self.hidden_before.last()).copied().unwrap_or(0)
    }

    /// The next visible line up / down from `line`.
    pub fn next_visible(&self, line: u32, down: bool, lines: u32) -> Option<u32> {
        let row = self.row_of(line) as i64 + if down { 1 } else { -1 };
        if row < 0 || row >= self.visible_rows(lines) as i64 {
            return None;
        }
        Some(self.line_of(row as u32))
    }

    fn rebuild(&mut self) {
        let collapsed: HashSet<usize> = self.collapsed.iter().copied().collect();
        let mut ranges: Vec<(u32, u32)> = self
            .available
            .iter()
            .filter(|f| collapsed.contains(&f.start_byte))
            .map(|f| (f.start_line + 1, f.end_line))
            .filter(|(a, b)| a <= b)
            .collect();
        ranges.sort_unstable();
        let mut hidden: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
        for (a, b) in ranges {
            match hidden.last_mut() {
                Some((_, end)) if a <= *end + 1 => *end = (*end).max(b),
                _ => hidden.push((a, b)),
            }
        }
        if hidden != self.hidden {
            self.hidden = hidden;
            self.generation += 1;
        }
        self.hidden_total = self.hidden.iter().map(|(a, b)| b - a + 1).sum();
        self.hidden_before.clear();
        self.hidden_before.push(0);
        let mut total = 0;
        for (a, b) in &self.hidden {
            total += b - a + 1;
            self.hidden_before.push(total);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/folding.rs"]
mod tests;
