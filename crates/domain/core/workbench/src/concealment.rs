//! Concealment: the parts of a document a module asks to be drawn hidden — a `.env` file's
//! secrets — until the pointer is over them or the caret is on their line. Only the drawing
//! changes: the text stays as it is (copied, saved, searched whole).
//!
//! A module contributes a [`Concealment`] for its languages; the app asks it again only
//! when the document's version or the contribution's [`Concealment::generation`] changes,
//! and the editor draws each hidden character as a mark of the same length in bytes
//! ([`mask`]): columns, the caret and the colors stay where they are.

use std::fmt::Debug;
use std::ops::Range;

/// A module's hidden parts of the documents of its languages.
pub trait Concealment: Send + Sync + Debug {
    /// The languages (their ids) whose documents it hides parts of.
    fn languages(&self) -> &'static [&'static str];
    /// Changes when what it hides would (its setting turned on or off): every document of
    /// its languages is asked again then.
    fn generation(&self) -> u64;
    /// The byte ranges of `text` drawn hidden, sorted and apart; none: nothing hidden.
    fn concealed(&self, text: &str) -> Vec<Range<usize>>;
}

/// The ranges among `concealed` still hidden: not those `revealed` says (the pointer over
/// it, the caret on its line).
pub fn hidden(
    concealed: &[Range<usize>],
    revealed: impl Fn(&Range<usize>) -> bool,
) -> Vec<Range<usize>> {
    concealed.iter().filter(|r| !revealed(r)).cloned().collect()
}

/// Draws the parts of `line` (a line of the text, starting at byte `start` of it) that
/// `hidden` covers as marks: `*` for a one-byte character, and a mark as long in bytes for
/// the others (`·`, `•`), so every byte stays where it was. Whitespace (a tab's width) and
/// four-byte characters are kept.
pub fn mask(line: &mut String, start: usize, hidden: &[Range<usize>]) {
    let end = start + line.len();
    let first = hidden.partition_point(|r| r.end <= start);
    let covered: Vec<Range<usize>> = hidden[first..]
        .iter()
        .take_while(|r| r.start < end)
        .map(|r| r.start.max(start) - start..r.end.min(end) - start)
        .filter(|r| !r.is_empty())
        .collect();
    if covered.is_empty() {
        return;
    }
    let masked: String = line
        .char_indices()
        .map(|(i, c)| match covered.iter().any(|r| r.contains(&i)) {
            false => c,
            true if c.is_whitespace() => c,
            true => match c.len_utf8() {
                1 => '*',
                2 => '·',
                3 => '•',
                _ => c,
            },
        })
        .collect();
    *line = masked;
}

#[cfg(test)]
#[path = "../tests/unit/concealment.rs"]
mod tests;
