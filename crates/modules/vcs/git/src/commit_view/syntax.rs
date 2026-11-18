//! A file's text colored for the changes' pane: the same grammars as the editor, asked once for
//! the whole file (a line alone can't tell it's inside a comment), kept per line shown.

use std::ops::Range;

use super::*;

/// What's longer than this isn't colored (its changes are still shown).
const MAX_BYTES: usize = 1024 * 1024;

/// A part of a line: its bytes and the capture coloring it (`None`: the code's own color).
pub(super) type Piece = (Range<usize>, Option<Arc<str>>);

/// A text's captures, as "which one owns each byte".
#[derive(Debug)]
pub(super) struct Colored<'a> {
    text: &'a str,
    /// 0: none; otherwise the place in `names`, plus one.
    owners: Vec<u32>,
    names: Vec<Arc<str>>,
    starts: Vec<usize>,
}

impl<'a> Colored<'a> {
    /// `text` under `captures`, in the order they apply (a later one wins).
    pub(super) fn new(text: &'a str, captures: Vec<(Range<usize>, Arc<str>)>) -> Self {
        let mut owners = vec![0; text.len()];
        let mut names: Vec<Arc<str>> = Vec::new();
        for (range, name) in captures {
            let at = names.iter().position(|n| *n == name).unwrap_or_else(|| {
                names.push(name);
                names.len() - 1
            });
            let end = range.end.min(text.len());
            if let Some(bytes) = owners.get_mut(range.start..end) {
                bytes.fill(u32::try_from(at + 1).unwrap_or(0));
            }
        }
        let starts =
            std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
        Self { text, owners, names, starts }
    }

    /// `text` as its language colors it, if it has one and isn't too long.
    pub(super) fn of(path: &str, text: &'a str) -> Option<Self> {
        if text.len() > MAX_BYTES {
            return None;
        }
        let extension = path.rsplit_once('.')?.1;
        let captures = ui_components::snippets::highlighter()?.highlight(extension, text)?;
        Some(Self::new(text, captures))
    }

    /// Line `line`'s pieces (from 0, without its ending; bytes counted from its start).
    pub(super) fn line(&self, line: usize) -> Vec<Piece> {
        let Some(&start) = self.starts.get(line) else { return Vec::new() };
        let end = self.starts.get(line + 1).copied().unwrap_or(self.text.len());
        let shown = self.text[start..end].trim_end_matches(['\n', '\r']).len();
        let owners = &self.owners[start..start + shown];
        let mut pieces = Vec::new();
        let mut from = 0;
        for at in 1..=owners.len() {
            if at == owners.len() || owners[at] != owners[from] {
                let name = owners[from].checked_sub(1).map(|n| Arc::clone(&self.names[n as usize]));
                pieces.push((from..at, name));
                from = at;
            }
        }
        pieces
    }
}

/// Each hunk's lines colored: what `old` and `new` say of them (nothing, without a grammar).
pub(super) fn color_hunks(
    path: &str,
    old: &str,
    new: &str,
    found: &[DiffHunk],
) -> Vec<Vec<Vec<Piece>>> {
    let (before, after) = (Colored::of(path, old), Colored::of(path, new));
    let number = |n: Option<u32>| n.and_then(|n| usize::try_from(n).ok()).map(|n| n - 1);
    let pieces = |line: &version_control::staging::DiffLine| {
        match line.kind {
            LineKind::Added => after.as_ref().zip(number(line.new)),
            _ => before.as_ref().zip(number(line.old)),
        }
        .map_or_else(Vec::new, |(colored, at)| colored.line(at))
    };
    found.iter().map(|hunk| hunk.lines.iter().map(pieces).collect()).collect()
}

#[cfg(test)]
#[path = "../../tests/unit/commit_view/syntax.rs"]
mod tests;
