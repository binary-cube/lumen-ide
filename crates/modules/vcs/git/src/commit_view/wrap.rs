//! The changes' rows when lines wrap ([`ui_components::TextOptions::word_wrap`]): a code
//! line takes the rows `text_options::wrap_rows` gives it at the code's width (a hunk's
//! header stays one). Kept as each row's first screen row and where each of its parts
//! starts (in columns, a tab four), made again when the changes or the width change;
//! off, every row is one high and none of this is made.

use std::ops::Range;

use super::*;

/// What the rows were made for: the changes (their reading's id), the columns.
type WrapKey = (u64, usize);

/// The changes' wrapped rows (see the module's doc).
#[derive(Debug, Default)]
pub(super) struct DiffWrap {
    key: Option<WrapKey>,
    /// The columns a row holds (0: not wrapping).
    pub(super) columns: usize,
    /// Each row's first screen row; one more at the end: all of them.
    firsts: Vec<u32>,
    /// Each row's parts' first columns (empty: one part, from 0).
    starts: Vec<Vec<usize>>,
}

impl DiffWrap {
    /// Made again for `diff` at `columns` (0: off) when either changed.
    pub(super) fn refresh(&mut self, diff: &FileDiff, columns: usize) {
        let key = (diff.id, columns);
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.columns = columns;
        self.firsts.clear();
        self.starts.clear();
        if columns == 0 {
            return;
        }
        let (mut drawn, mut parts): (String, Vec<Range<usize>>) = (String::new(), Vec::new());
        let mut at: u32 = 0;
        for row in &diff.rows {
            self.firsts.push(at);
            let DiffRow::Line(h, l) = *row else {
                self.starts.push(Vec::new());
                at += 1;
                continue;
            };
            let line = diff.hunks[h].lines[l].text.trim_end_matches(['\n', '\r']);
            drawn.clear();
            for c in line.chars() {
                if c == '\t' {
                    drawn.push_str("    ");
                } else {
                    drawn.push(c);
                }
            }
            ui_components::text_options::wrap_rows(&drawn, columns, 4, &mut parts);
            let column = |byte: usize| drawn[..byte].chars().count();
            let starts: Vec<usize> = if parts.len() > 1 {
                parts.iter().map(|p| column(p.start)).collect()
            } else {
                Vec::new()
            };
            at = at.saturating_add(parts.len() as u32);
            self.starts.push(starts);
        }
        self.firsts.push(at);
    }

    /// Lines wrap now.
    pub(super) fn on(&self) -> bool {
        self.columns > 0 && !self.firsts.is_empty()
    }

    /// Row `row`'s first screen row.
    pub(super) fn first(&self, row: usize) -> u32 {
        if !self.on() {
            return row as u32;
        }
        self.firsts.get(row).copied().unwrap_or_else(|| self.firsts.last().copied().unwrap_or(0))
    }

    /// How many screen rows row `row` takes.
    pub(super) fn rows(&self, row: usize) -> u32 {
        if !self.on() {
            return 1;
        }
        self.first(row + 1).saturating_sub(self.first(row)).max(1)
    }

    /// All the screen rows of `rows` rows.
    pub(super) fn total(&self, rows: usize) -> u32 {
        if self.on() { self.firsts.last().copied().unwrap_or(0) } else { rows as u32 }
    }

    /// The row at screen row `screen`, and which of its parts.
    pub(super) fn at(&self, screen: u32) -> (usize, usize) {
        if !self.on() {
            return (screen as usize, 0);
        }
        let last = self.firsts.len().saturating_sub(2);
        let row = self.firsts.partition_point(|&f| f <= screen).saturating_sub(1).min(last);
        (row, screen.saturating_sub(self.first(row)) as usize)
    }

    /// Row `row`'s parts' first columns, and where each ends (the last at `width`).
    pub(super) fn parts(&self, row: usize, width: usize) -> Vec<(usize, usize)> {
        let starts = self.starts.get(row).filter(|s| !s.is_empty());
        let Some(starts) = starts else { return vec![(0, width)] };
        starts
            .iter()
            .enumerate()
            .map(|(k, &from)| (from, starts.get(k + 1).copied().unwrap_or(width)))
            .collect()
    }
}
