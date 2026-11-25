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
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

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

/// What of a parked history stays in memory once its rows are on disk ([`Parked::encode`]):
/// each chunk's width, its rows let go, its rare cells with more (combining marks, links).
#[derive(Debug, Default)]
pub(crate) struct Kept {
    columns: usize,
    skip: usize,
    extras: Vec<(u32, u16, Arc<CellExtra>)>,
}

impl Kept {
    /// What it holds in memory (bytes).
    pub(crate) fn bytes(kept: &[Kept]) -> usize {
        kept.iter()
            .map(|k| {
                std::mem::size_of::<Kept>()
                    + k.extras.capacity() * std::mem::size_of::<(u32, u16, Arc<CellExtra>)>()
            })
            .sum()
    }
}

/// The encoded form's first bytes (its version).
const MAGIC: &[u8; 4] = b"PRK1";

/// The named colors, by their place in the encoded form (the emulator's own enum has gaps).
const NAMED: [NamedColor; 29] = [
    NamedColor::Black,
    NamedColor::Red,
    NamedColor::Green,
    NamedColor::Yellow,
    NamedColor::Blue,
    NamedColor::Magenta,
    NamedColor::Cyan,
    NamedColor::White,
    NamedColor::BrightBlack,
    NamedColor::BrightRed,
    NamedColor::BrightGreen,
    NamedColor::BrightYellow,
    NamedColor::BrightBlue,
    NamedColor::BrightMagenta,
    NamedColor::BrightCyan,
    NamedColor::BrightWhite,
    NamedColor::Foreground,
    NamedColor::Background,
    NamedColor::Cursor,
    NamedColor::DimBlack,
    NamedColor::DimRed,
    NamedColor::DimGreen,
    NamedColor::DimYellow,
    NamedColor::DimBlue,
    NamedColor::DimMagenta,
    NamedColor::DimCyan,
    NamedColor::DimWhite,
    NamedColor::BrightForeground,
    NamedColor::DimForeground,
];

fn put_color(out: &mut Vec<u8>, color: Color) {
    match color {
        Color::Named(named) => {
            let at = NAMED.iter().position(|n| *n == named).unwrap_or(16);
            out.extend_from_slice(&[0, at as u8]);
        }
        Color::Spec(rgb) => out.extend_from_slice(&[1, rgb.r, rgb.g, rgb.b]),
        Color::Indexed(i) => out.extend_from_slice(&[2, i]),
    }
}

/// Reads the encoded form, every length checked.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn color(&mut self) -> Option<Color> {
        Some(match self.u8()? {
            0 => Color::Named(*NAMED.get(usize::from(self.u8()?))?),
            1 => {
                let rgb = self.take(3)?;
                Color::Spec(Rgb { r: rgb[0], g: rgb[1], b: rgb[2] })
            }
            2 => Color::Indexed(self.u8()?),
            _ => return None,
        })
    }
}

impl Chunk {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.text.len() as u32).to_le_bytes());
        out.extend_from_slice(self.text.as_bytes());
        out.extend_from_slice(&(self.runs.len() as u32).to_le_bytes());
        for run in &self.runs {
            out.extend_from_slice(&run.cells.to_le_bytes());
            put_color(out, run.fg);
            put_color(out, run.bg);
            out.extend_from_slice(&run.flags.bits().to_le_bytes());
        }
        out.extend_from_slice(&(self.rows.len() as u32).to_le_bytes());
        for row in &self.rows {
            out.extend_from_slice(&row.text.to_le_bytes());
            out.extend_from_slice(&row.runs.to_le_bytes());
            out.extend_from_slice(&row.used.to_le_bytes());
            out.extend_from_slice(&row.len.to_le_bytes());
        }
    }

    /// A chunk read back, with what stayed in memory; `None` if anything doesn't hold (a
    /// row's end past its text or off a character's boundary, its runs out of order).
    fn decode(input: &mut Reader<'_>, kept: Kept) -> Option<Self> {
        let len = input.u32()? as usize;
        let text = String::from_utf8(input.take(len)?.to_vec()).ok()?;
        let count = input.u32()? as usize;
        let mut runs = Vec::with_capacity(count.min(input.0.len()));
        for _ in 0..count {
            let cells = input.u16()?;
            let (fg, bg) = (input.color()?, input.color()?);
            let flags = Flags::from_bits_truncate(input.u16()?);
            runs.push(Run { cells, fg, bg, flags });
        }
        let count = input.u32()? as usize;
        let mut rows = Vec::with_capacity(count.min(input.0.len()));
        let (mut text_at, mut runs_at) = (0u32, 0u32);
        for _ in 0..count {
            let row = RowEnd {
                text: input.u32()?,
                runs: input.u32()?,
                used: input.u16()?,
                len: input.u16()?,
            };
            let whole = row.text >= text_at
                && row.runs >= runs_at
                && row.used <= row.len
                && text.is_char_boundary(row.text as usize)
                && (row.runs as usize) <= runs.len();
            if !whole {
                return None;
            }
            (text_at, runs_at) = (row.text, row.runs);
            rows.push(row);
        }
        if kept.skip > rows.len() || kept.extras.iter().any(|(r, _, _)| *r as usize >= rows.len()) {
            return None;
        }
        Some(Self { columns: kept.columns, text, runs, rows, extras: kept.extras, skip: kept.skip })
    }
}

impl Parked {
    /// Its rows as bytes (what goes to disk: their text, colors and ends) and what stays in
    /// memory ([`Kept`]); it's emptied.
    pub(crate) fn encode(&mut self) -> (Vec<u8>, Vec<Kept>) {
        let chunks = std::mem::take(&mut self.chunks);
        let mut out = Vec::with_capacity(chunks.iter().map(Chunk::bytes).sum::<usize>() + 8);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
        let mut kept = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            chunk.encode(&mut out);
            kept.push(Kept { columns: chunk.columns, skip: chunk.skip, extras: chunk.extras });
        }
        (out, kept)
    }

    /// What [`Self::encode`] gave, put back together; `None` if it doesn't hold.
    pub(crate) fn decode(bytes: &[u8], kept: Vec<Kept>) -> Option<Self> {
        let mut input = Reader(bytes);
        if input.take(MAGIC.len())? != MAGIC || input.u32()? as usize != kept.len() {
            return None;
        }
        let chunks = kept
            .into_iter()
            .map(|kept| Chunk::decode(&mut input, kept))
            .collect::<Option<Vec<_>>>()?;
        input.0.is_empty().then_some(Self { chunks })
    }

    /// `newer`'s rows put after its own (they were parked later).
    pub(crate) fn append(&mut self, mut newer: Parked) {
        self.chunks.append(&mut newer.chunks);
    }
}

#[cfg(test)]
#[path = "../tests/unit/parked.rs"]
pub(crate) mod tests;
