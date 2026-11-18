//! Search in a document (plain text or regex, by line).

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use super::buffer::TextBuffer;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// A line of a text with matches: its number (0-based), where it starts in the text (bytes),
/// its text (without the line break) and the matches in it (byte ranges in the line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    pub line: u32,
    pub start: usize,
    pub text: String,
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid regular expression: {0}")]
pub struct SearchError(String);

/// A compiled search, ready to use on any buffer.
#[derive(Debug, Clone)]
pub struct Matcher {
    regex: Regex,
}

impl Matcher {
    /// `Ok(None)` for an empty pattern.
    pub fn new(query: &SearchQuery) -> Result<Option<Self>, SearchError> {
        if query.pattern.is_empty() {
            return Ok(None);
        }
        let mut pattern =
            if query.regex { query.pattern.clone() } else { regex::escape(&query.pattern) };
        if query.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!query.case_sensitive)
            .size_limit(1 << 22)
            .build()
            .map_err(|e| SearchError(e.to_string()))?;
        Ok(Some(Self { regex }))
    }

    /// All matches on lines `lines` (absolute offsets), appended to `out`.
    pub fn find_in_lines(
        &self,
        buffer: &TextBuffer,
        lines: Range<usize>,
        out: &mut Vec<Range<usize>>,
    ) {
        let mut text = String::new();
        for line in lines.start..lines.end.min(buffer.line_count()) {
            buffer.write_line(line, &mut text);
            let start = buffer.line_start(line);
            out.extend(
                self.regex
                    .find_iter(&text)
                    .filter(|m| !m.is_empty())
                    .map(|m| start + m.start()..start + m.end()),
            );
        }
    }

    /// The lines of `text` with matches, in order, at most `limit` matches in all (the last
    /// line's cut there). A text without any is passed over at once.
    pub fn lines_matching(&self, text: &str, limit: usize) -> Vec<LineMatch> {
        let mut out = Vec::new();
        if limit == 0 || !self.regex.is_match(text) {
            return out;
        }
        let (mut count, mut start) = (0, 0);
        for (n, whole) in text.split('\n').enumerate() {
            let line_start = start;
            start += whole.len() + 1;
            let line = whole.strip_suffix('\r').unwrap_or(whole);
            let ranges: Vec<Range<usize>> = self
                .regex
                .find_iter(line)
                .filter(|m| !m.is_empty())
                .map(|m| m.start()..m.end())
                .take(limit - count)
                .collect();
            if ranges.is_empty() {
                continue;
            }
            count += ranges.len();
            let number = u32::try_from(n).unwrap_or(u32::MAX);
            out.push(LineMatch { line: number, start: line_start, text: line.to_owned(), ranges });
            if count >= limit {
                break;
            }
        }
        out
    }

    /// All matches in the document.
    pub fn find_all(&self, buffer: &TextBuffer) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        self.find_in_lines(buffer, 0..buffer.line_count(), &mut out);
        out
    }

    /// The replacement text for a match (`$1`, `${name}` work in regex mode).
    pub fn replacement(&self, matched: &str, replace: &str, regex: bool) -> String {
        if !regex {
            return replace.to_owned();
        }
        self.regex.replace(matched, replace).into_owned()
    }
}

/// The index of the first match starting at or after `offset` (wrapping from the start).
pub fn next_match(matches: &[Range<usize>], offset: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().position(|m| m.start >= offset).unwrap_or(0))
}

/// The index of the last match starting before `offset` (wrapping from the end).
pub fn prev_match(matches: &[Range<usize>], offset: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().rposition(|m| m.start < offset).unwrap_or(matches.len() - 1))
}

#[cfg(test)]
#[path = "../../tests/unit/domain/search.rs"]
mod tests;
