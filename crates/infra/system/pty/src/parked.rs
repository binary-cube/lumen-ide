//! A terminal's history in a compact form, while the terminal isn't shown (see `parking`).
//!
//! The emulator keeps every history row as full-width cells of 24 bytes (~5 KB a row at 200
//! columns, made ahead by a thousand). Parked, a row is its text (UTF-8, one character per
//! cell), its runs of the same colors and attributes, its length and the cells it uses;
//! the rare cell with more (a combining mark, an underline's color, a link) keeps that as it
//! was. Rows come back exactly as they went: the same characters, colors, attributes, wide
//! characters and wrapped lines.
//!
//! All the rows of one park share a few buffers (no allocation per row).

use std::sync::Arc;

use alacritty_terminal::grid::Row;
use alacritty_terminal::term::cell::{Cell, CellExtra, Flags};
use alacritty_terminal::vte::ansi::Color;

/// Cells in a row with the same colors and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    cells: u16,
    fg: Color,
    bg: Color,
    flags: Flags,
}

/// Where a row's parts end in its chunk's buffers.
#[derive(Debug, Clone, Copy)]
struct RowEnd {
    /// The end of its text (bytes).
    text: u32,
    /// The end of its runs.
    runs: u32,
    /// Its cells up to the last one that isn't blank (the rest are).
    used: u16,
    /// Its cells.
    len: u16,
}

/// The rows of one park, oldest first, at the width they had.
#[derive(Debug, Default)]
struct Chunk {
    columns: usize,
    text: String,
    runs: Vec<Run>,
    rows: Vec<RowEnd>,
    /// The cells with more than a character and colors: (row, column, what more).
    extras: Vec<(u32, u16, Arc<CellExtra>)>,
    /// Its oldest rows let go (past the history's limit).
    skip: usize,
}

impl Chunk {
    fn len(&self) -> usize {
        self.rows.len() - self.skip
    }

    fn bytes(&self) -> usize {
        self.text.capacity()
            + self.runs.capacity() * std::mem::size_of::<Run>()
            + self.rows.capacity() * std::mem::size_of::<RowEnd>()
            + self.extras.capacity() * std::mem::size_of::<(u32, u16, Arc<CellExtra>)>()
    }

    fn push(&mut self, row: &Row<Cell>) {
        let blank = Cell::default();
        let cells = &row[..];
        let used = cells.iter().rposition(|c| *c != blank).map_or(0, |i| i + 1);
        let index = self.rows.len() as u32;
        let mut run: Option<Run> = None;
        for (col, cell) in cells[..used].iter().enumerate() {
            self.text.push(cell.c);
            match &mut run {
                Some(r) if r.fg == cell.fg && r.bg == cell.bg && r.flags == cell.flags => {
                    r.cells += 1;
                }
                _ => {
                    self.runs.extend(run.take());
                    run = Some(Run { cells: 1, fg: cell.fg, bg: cell.bg, flags: cell.flags });
                }
            }
            if let Some(extra) = &cell.extra {
                self.extras.push((index, col as u16, Arc::clone(extra)));
            }
        }
        self.runs.extend(run);
        self.rows.push(RowEnd {
            text: self.text.len() as u32,
            runs: self.runs.len() as u32,
            used: used as u16,
            len: cells.len() as u16,
        });
    }

    /// Row `index` (of all its rows, those skipped too) as the emulator had it.
    fn row(&self, index: usize) -> Row<Cell> {
        let end = self.rows[index];
        let (text_start, runs_start) = match index.checked_sub(1) {
            Some(before) => (self.rows[before].text as usize, self.rows[before].runs as usize),
            None => (0, 0),
        };
        let mut cells = Vec::with_capacity(usize::from(end.len));
        let mut chars = self.text[text_start..end.text as usize].chars();
        for run in &self.runs[runs_start..end.runs as usize] {
            for _ in 0..run.cells {
                let c = chars.next().unwrap_or(' ');
                cells.push(Cell { c, fg: run.fg, bg: run.bg, flags: run.flags, extra: None });
            }
        }
        cells.resize_with(usize::from(end.len), Cell::default);
        let index = index as u32;
        let first = self.extras.partition_point(|(row, _, _)| *row < index);
        for (_, col, extra) in self.extras[first..].iter().take_while(|(row, _, _)| *row == index) {
            if let Some(cell) = cells.get_mut(usize::from(*col)) {
                cell.extra = Some(Arc::clone(extra));
            }
        }
        Row::from_vec(cells, usize::from(end.used))
    }
}

/// A terminal's parked history: its rows, oldest first, in the parks they were made in.
#[derive(Debug, Default)]
pub(crate) struct Parked {
    chunks: Vec<Chunk>,
}

impl Parked {
    /// The rows parked.
    pub(crate) fn len(&self) -> usize {
        self.chunks.iter().map(Chunk::len).sum()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// What they hold in memory (bytes).
    pub(crate) fn bytes(&self) -> usize {
        self.chunks.iter().map(Chunk::bytes).sum()
    }

    /// `rows` (oldest first, `columns` wide) parked after those already here: they're newer.
    pub(crate) fn push(&mut self, rows: &[Row<Cell>], columns: usize) {
        if rows.is_empty() {
            return;
        }
        let mut chunk = Chunk {
            columns,
            text: String::with_capacity(rows.len() * 64),
            runs: Vec::with_capacity(rows.len() * 2),
            rows: Vec::with_capacity(rows.len()),
            ..Chunk::default()
        };
        for row in rows {
            chunk.push(row);
        }
        chunk.text.shrink_to_fit();
        chunk.runs.shrink_to_fit();
        chunk.extras.shrink_to_fit();
        self.chunks.push(chunk);
    }

    /// The oldest rows let go until at most `most` are left.
    pub(crate) fn keep(&mut self, most: usize) {
        let mut extra = self.len().saturating_sub(most);
        while extra > 0 {
            let Some(oldest) = self.chunks.first_mut() else { return };
            if oldest.len() <= extra {
                extra -= oldest.len();
                self.chunks.remove(0);
            } else {
                oldest.skip += extra;
                extra = 0;
            }
        }
    }

    /// All its rows as the emulator had them, oldest first, each with the width it had
    /// (parks made at different widths: runs of each width), the parked ones let go.
    pub(crate) fn take(&mut self) -> Vec<(usize, Vec<Row<Cell>>)> {
        let mut out: Vec<(usize, Vec<Row<Cell>>)> = Vec::new();
        for chunk in std::mem::take(&mut self.chunks) {
            let rows = (chunk.skip..chunk.rows.len()).map(|i| chunk.row(i));
            match out.last_mut() {
                Some((columns, same)) if *columns == chunk.columns => same.extend(rows),
                _ => out.push((chunk.columns, rows.collect())),
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../tests/unit/parked.rs"]
pub(crate) mod tests;
