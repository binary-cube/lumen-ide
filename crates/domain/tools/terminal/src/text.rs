//! The terminal's text, for the mouse: positions that survive scrolling, the selection and
//! its text, words (double click) and links (`file:line`, URLs).

use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

use crate::GridSnapshot;

/// A cell, in lines counted from the top of the screen when scrolled to the bottom
/// (negative = in history): scrolling doesn't move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct TermPoint {
    pub line: i32,
    pub col: u16,
}

impl GridSnapshot {
    /// The point of visible cell (`row`, `col`).
    pub fn point(&self, row: u16, col: u16) -> TermPoint {
        TermPoint { line: i32::from(row) - self.display_offset as i32, col }
    }

    /// The visible row of `line`, if it is on screen.
    pub fn visible_row(&self, line: i32) -> Option<u16> {
        let row = line + self.display_offset as i32;
        (0..i32::from(self.rows)).contains(&row).then_some(row as u16)
    }

    /// A row as text, one `char` per cell (so char index = column); the cell after a wide
    /// character is a space.
    pub fn row_chars(&self, row: u16) -> String {
        self.row(row).iter().map(|c| if c.ch == '\0' { ' ' } else { c.ch }).collect()
    }
}

/// A snapshot asked with a selection's text ([`crate::TerminalSession::try_snapshot_selecting`]).
#[derive(Debug, Clone, Copy)]
pub struct SelectionAsk<'a> {
    pub selection: &'a TermSelection,
    /// The fingerprint the caller's text was made at (`None`: it has none for it).
    pub made_at: Option<u64>,
}

/// What a snapshot asked with a selection's text gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapped {
    /// The emulator was busy: nothing copied.
    Busy,
    /// The snapshot taken; the text not made (unchanged since `made_at`, or not readable).
    Taken,
    /// The snapshot taken, with the selection's text.
    With(String),
}

/// A selection: from where the mouse was pressed to where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSelection {
    pub anchor: TermPoint,
    pub head: TermPoint,
}

impl TermSelection {
    /// Start and end (inclusive), in reading order.
    pub fn ordered(&self) -> (TermPoint, TermPoint) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The selected columns of `line` (end exclusive), if any.
    pub fn columns(&self, line: i32, cols: u16) -> Option<Range<u16>> {
        let (start, end) = self.ordered();
        if self.is_empty() || line < start.line || line > end.line {
            return None;
        }
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == end.line { end.col + 1 } else { cols };
        (from < to).then(|| from..to.min(cols))
    }

    /// The selected text that is on screen: rows joined by `\n`, without trailing spaces.
    pub fn text(&self, grid: &GridSnapshot) -> String {
        let (start, end) = self.ordered();
        let mut out = Vec::new();
        for line in start.line..=end.line {
            let Some(row) = grid.visible_row(line) else { continue };
            let Some(cols) = self.columns(line, grid.cols) else { continue };
            let chars: Vec<char> = grid.row_chars(row).chars().collect();
            let end = (cols.end as usize).min(chars.len());
            let start = (cols.start as usize).min(end);
            out.push(chars[start..end].iter().collect::<String>().trim_end().to_owned());
        }
        out.join("\n")
    }
}

/// The word around column `col` of `row` (letters, digits and path characters), as columns.
pub fn word_at(row: &str, col: u16) -> Option<Range<u16>> {
    let chars: Vec<char> = row.chars().collect();
    let word = |c: char| c.is_alphanumeric() || "_-./~:@$%+=#".contains(c);
    let col = col as usize;
    if !chars.get(col).copied().is_some_and(word) {
        return None;
    }
    let start = chars[..col].iter().rposition(|c| !word(*c)).map_or(0, |i| i + 1);
    let end = chars[col..].iter().position(|c| !word(*c)).map_or(chars.len(), |i| col + i);
    Some(start as u16..end as u16)
}

/// Where a link in the terminal leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// A path as written (relative to the shell's directory, or absolute), with the line and
    /// column (1-based) when given.
    File {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// A link in a row: its columns and target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermLink {
    pub cols: Range<u16>,
    pub target: LinkTarget,
}

/// The links in a row (`row` as from [`GridSnapshot::row_chars`]): URLs and paths with an
/// extension, optionally followed by a line — `src/A.php:12`, `a.php:12:5`, `a.php(12)`,
/// `a.php on line 12` (PHP's errors). Whether a path exists is for the caller to check.
pub fn links(row: &str) -> Vec<TermLink> {
    static URL: OnceLock<Option<Regex>> = OnceLock::new();
    static FILE: OnceLock<Option<Regex>> = OnceLock::new();
    let url = URL.get_or_init(|| Regex::new(r#"https?://[^\s<>"'`]+"#).ok());
    let file = FILE.get_or_init(|| {
        Regex::new(
            r"(?P<path>(?:[A-Za-z]:[\\/]|[~./\\])?[\w@%+=~.\-]*[\w\-]+(?:[\\/][\w@%+=~.\-]+)*\.[A-Za-z][A-Za-z0-9]{0,7})(?:(?::| on line |\()(?P<line>\d+)(?::(?P<col>\d+))?\)?)?",
        )
        .ok()
    });
    let (Some(url), Some(file)) = (url, file) else { return Vec::new() };
    // Byte offsets → columns (one char per cell).
    let col = |byte: usize| row[..byte].chars().count() as u16;
    let mut found: Vec<TermLink> = Vec::new();
    for m in url.find_iter(row) {
        let text = m.as_str().trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '!', '?']);
        let end = m.start() + text.len();
        found.push(TermLink {
            cols: col(m.start())..col(end),
            target: LinkTarget::Url(text.to_owned()),
        });
    }
    for c in file.captures_iter(row) {
        let Some(whole) = c.get(0) else { continue };
        let cols = col(whole.start())..col(whole.end());
        // Inside a URL (its path): already a link.
        if found.iter().any(|l| l.cols.start < cols.end && cols.start < l.cols.end) {
            continue;
        }
        let number = |name: &str| c.name(name).and_then(|m| m.as_str().parse().ok());
        let path = c.name("path").map_or("", |m| m.as_str()).to_owned();
        found.push(TermLink {
            cols,
            target: LinkTarget::File { path, line: number("line"), column: number("col") },
        });
    }
    found.sort_by_key(|l| l.cols.start);
    found
}

#[cfg(test)]
#[path = "../tests/unit/text.rs"]
mod tests;
