//! Formatters built into a language's module ([`TextFormatter`]): they lay the editor's
//! text out themselves — no program to find, nothing saved first — the whole file, a
//! selection, or what's at the cursor (SQL's statement). "Reformat File" runs one when the
//! project chose no formatter for the file's language. Pure: what to change is returned,
//! the caller changes it.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// What is laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatScope {
    /// The whole text.
    Whole,
    /// What a selection touches (bytes).
    Range(Range<usize>),
    /// What's at the cursor (bytes): a SQL file's statement.
    At(usize),
}

/// A text to lay out, as the editor has it.
#[derive(Debug, Clone)]
pub struct FormatRequest {
    /// The project it's laid out in (a module may know more of its files: a console's
    /// dialect).
    pub project: Option<kernel::ProjectId>,
    pub path: PathBuf,
    /// Its language's id (`sql`).
    pub language: String,
    /// Its text, unsaved changes included.
    pub text: Arc<str>,
    pub scope: FormatScope,
}

/// A change of a text: `range` (bytes) replaced by `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub text: String,
}

/// What laying a text out gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formatted {
    /// This change; `left` parts were left as written (unfinished code).
    Changed { edit: TextEdit, left: usize },
    /// Laid out already.
    Unchanged { left: usize },
    /// Nothing to lay out there (no statement at the cursor).
    Nothing,
}

/// A formatter a language's module brings.
pub trait TextFormatter: Send + Sync + std::fmt::Debug {
    /// The languages it lays out (their ids).
    fn languages(&self) -> &'static [&'static str];
    /// `request`'s text laid out (quick: a large text is asked for off the UI thread).
    fn format(&self, request: &FormatRequest) -> Formatted;
}

/// The change replacing `range` of `text` by `replacement`, narrowed to what differs (the
/// same start and end left alone: the cursor and the marks there stay); `None` when nothing
/// does.
pub fn narrowed(text: &str, range: Range<usize>, replacement: &str) -> Option<TextEdit> {
    let old = text.get(range.clone())?;
    if old == replacement {
        return None;
    }
    let mut prefix = old.bytes().zip(replacement.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(prefix) || !replacement.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let (old_rest, new_rest) = (&old[prefix..], &replacement[prefix..]);
    let mut suffix =
        old_rest.bytes().rev().zip(new_rest.bytes().rev()).take_while(|(a, b)| a == b).count();
    while !old_rest.is_char_boundary(old_rest.len() - suffix)
        || !new_rest.is_char_boundary(new_rest.len() - suffix)
    {
        suffix -= 1;
    }
    Some(TextEdit {
        range: range.start + prefix..range.end - suffix,
        text: new_rest[..new_rest.len() - suffix].to_owned(),
    })
}

#[cfg(test)]
#[path = "../tests/unit/builtin.rs"]
mod tests;
