//! The diagnostics modules found in the project (a checker's problems, kept in the
//! project's data: `ProjectDiagnostics`) as editor marks: shown like the editor's own errors
//! and warnings — the same squiggles, scroll strip marks, hover, Structure panel and status
//! bar.

use workbench_model::{Diagnostic, Severity};

use super::*;

/// A diagnostic as an editor mark: its range (the word at its column — or the character
/// there, or the last one at the line's end —, else its line without the leading space),
/// its message with where it comes from (the tool and its rule). `None`: nothing to mark
/// (past the text, an empty line). `word_chars`: what the file's language has in a name
/// besides letters, digits and `_` (`$` in PHP).
pub(super) fn mark_of(
    problem: &Diagnostic,
    buffer: &document::TextBuffer,
    word_chars: &str,
) -> Option<Mark> {
    let line = problem.line as usize;
    if line >= buffer.line_count() {
        return None;
    }
    let (start, end) = (buffer.line_start(line), buffer.line_end(line));
    let text = buffer.slice(start..end);
    let range = match problem.column {
        Some(column) => {
            let at = text.char_indices().nth(column as usize).map_or(text.len(), |(i, _)| i);
            let rest = &text[at..];
            let word = |c: char| c.is_alphanumeric() || c == '_' || word_chars.contains(c);
            let len = rest.char_indices().find(|(_, c)| !word(*c)).map_or(rest.len(), |(i, _)| i);
            let len = if len > 0 { len } else { rest.chars().next().map_or(0, char::len_utf8) };
            if len > 0 {
                start + at..start + at + len
            } else {
                let back = text[..at].chars().next_back().map_or(0, char::len_utf8);
                start + at - back..start + at
            }
        }
        None => start + (text.len() - text.trim_start().len())..end,
    };
    if range.is_empty() {
        return None;
    }
    let source = match &problem.rule {
        Some(rule) => format!("{} · {rule}", problem.source),
        None => problem.source.clone(),
    };
    Some(Mark {
        range,
        message: problem.message.clone(),
        code: Some(source),
        warning: problem.severity != Severity::Error,
        weak: problem.severity == Severity::Info,
        struck: false,
    })
}

/// The ranges the language servers' own diagnostics cover, asked whether another range meets
/// one (the editor's own error there isn't shown twice): sorted by start, with the farthest
/// end so far, so each question is a binary search, not a walk over all of them (a file
/// with hundreds of the servers' diagnostics, each of the editor's marks asked).
#[derive(Debug, Default)]
pub(super) struct Covered {
    /// (start, end), sorted by start.
    ranges: Vec<(usize, usize)>,
    /// The farthest end among `ranges[..=i]`.
    reach: Vec<usize>,
}

impl Covered {
    pub(super) fn new(ranges: impl IntoIterator<Item = std::ops::Range<usize>>) -> Self {
        let mut ranges: Vec<(usize, usize)> =
            ranges.into_iter().map(|r| (r.start, r.end)).collect();
        ranges.sort_unstable();
        let mut farthest = 0;
        let reach = ranges
            .iter()
            .map(|&(_, end)| {
                farthest = farthest.max(end);
                farthest
            })
            .collect();
        Self { ranges, reach }
    }

    /// `range` meets one of them: they overlap, or start at the same place (an empty one
    /// meets what starts where it is).
    pub(super) fn meets(&self, range: &std::ops::Range<usize>) -> bool {
        // Those starting before `range` ends: one of them reaching past its start overlaps.
        let before = self.ranges.partition_point(|&(start, _)| start < range.end);
        let overlap = before > 0 && self.reach[before - 1] > range.start;
        overlap || self.ranges.binary_search_by_key(&range.start, |&(start, _)| start).is_ok()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/diagnostics.rs"]
mod tests;
