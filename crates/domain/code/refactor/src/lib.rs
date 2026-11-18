//! Refactoring bounded context: a refactoring is a **plan** — the places it changes in the
//! project's files (each with its line, how sure it is, and whether it's chosen) and the
//! files it renames — shown before anything is written, then **applied** as text edits.
//!
//! Nothing here knows a language: what a symbol is and where it's used come through the
//! ports of [`application`] (the app answers them with each language's own rules).

pub mod application;
pub mod applying;
pub mod events;
pub mod moving;
pub mod signature;

use std::ops::Range;
use std::path::PathBuf;

/// What the occurrences of a name are made of, for finding them as whole words: letters,
/// digits, `_`, the language's `extra` characters and its `sigil` (part of a name before it:
/// `$total` in PHP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameStyle {
    pub extra: &'static str,
    pub sigil: Option<char>,
}

impl NameStyle {
    /// Code identifiers.
    pub const CODE: Self = Self { extra: "", sigil: None };
    /// Also `-` (CSS classes, ids, custom properties).
    pub const CSS: Self = Self { extra: "-", sigil: None };

    /// The same, with the language's sigil.
    pub const fn with_sigil(self, sigil: Option<char>) -> Self {
        Self { sigil, ..self }
    }

    fn is_name_char(self, c: char) -> bool {
        c.is_alphanumeric() || c == '_' || self.extra.contains(c) || Some(c) == self.sigil
    }
}

/// Where `name` appears in `text` as a whole word (byte ranges).
pub fn find_word(text: &str, name: &str, style: NameStyle) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if name.is_empty() {
        return found;
    }
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let start = from + i;
        let end = start + name.len();
        from = end;
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        // `$name` is the name of a PHP variable `name`: the sigil belongs to it, so it's
        // allowed before a name that starts without it, not before another one.
        let sigil_before =
            |c: char| Some(c) == style.sigil && !name.starts_with(|n: char| Some(n) == style.sigil);
        let before_ok = before.is_none_or(|c| !style.is_name_char(c) || sigil_before(c));
        let after_ok = after.is_none_or(|c| !style.is_name_char(c));
        if before_ok && after_ok {
            found.push(start..end);
        }
    }
    found
}

/// How sure a found place is to be the renamed symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certainty {
    /// It resolves to the symbol: chosen by default.
    Certain,
    /// It couldn't be resolved (e.g. a method called on a value of unknown type): shown,
    /// left for the user to choose.
    Uncertain,
}

/// A place in a file the refactoring changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The bytes replaced.
    pub range: Range<usize>,
    /// What they were when the plan was made (checked before applying: the text may have
    /// changed since).
    pub original: String,
    /// What replaces them (for a rename, the new name: filled in when it's known).
    pub replacement: String,
    pub certainty: Certainty,
    /// Applied when the plan is.
    pub chosen: bool,
    /// The line (0-based) and its text, for the preview.
    pub line: u32,
    pub line_text: String,
    /// The range within `line_text`.
    pub in_line: Range<usize>,
}

/// A file the refactoring touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    /// Absolute.
    pub path: PathBuf,
    pub occurrences: Vec<Occurrence>,
}

/// A file the refactoring renames (e.g. a PHP class's file, PSR-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRename {
    pub from: PathBuf,
    pub to: PathBuf,
    pub chosen: bool,
}

/// A refactoring before it's applied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub files: Vec<FilePlan>,
    pub renames: Vec<FileRename>,
}

impl Plan {
    /// How many places are chosen, and in how many files.
    pub fn chosen(&self) -> (usize, usize) {
        let mut places = 0;
        let mut files = 0;
        for file in &self.files {
            let n = file.occurrences.iter().filter(|o| o.chosen).count();
            places += n;
            files += usize::from(n > 0);
        }
        (places, files)
    }

    /// Sets every occurrence's replacement (a rename's new name, as typed); a renamed file
    /// follows the new name (`User.php` → `Customer.php`).
    pub fn set_replacement(&mut self, text: &str) {
        for file in &mut self.files {
            for occurrence in &mut file.occurrences {
                occurrence.replacement = text.to_owned();
            }
        }
        for rename in &mut self.renames {
            let name = match rename.from.extension() {
                Some(ext) => format!("{text}.{}", ext.to_string_lossy()),
                None => text.to_owned(),
            };
            rename.to = rename.from.with_file_name(name);
        }
    }

    /// Chooses / leaves out every occurrence of file `file`.
    pub fn choose_file(&mut self, file: usize, on: bool) {
        if let Some(file) = self.files.get_mut(file) {
            for occurrence in &mut file.occurrences {
                occurrence.chosen = on;
            }
        }
    }

    /// Chooses / leaves out every occurrence.
    pub fn choose_all(&mut self, on: bool) {
        for i in 0..self.files.len() {
            self.choose_file(i, on);
        }
        for rename in &mut self.renames {
            rename.chosen = on;
        }
    }

    /// The edits to make, per file: the chosen occurrences, in order.
    pub fn edits(&self) -> Vec<FileEdit> {
        self.files
            .iter()
            .filter_map(|file| {
                let mut edits: Vec<TextEdit> = file
                    .occurrences
                    .iter()
                    .filter(|o| o.chosen)
                    .map(|o| TextEdit {
                        range: o.range.clone(),
                        original: o.original.clone(),
                        text: o.replacement.clone(),
                    })
                    .collect();
                edits.sort_by_key(|e| e.range.start);
                edits.dedup_by(|a, b| a.range == b.range);
                (!edits.is_empty()).then(|| FileEdit { path: file.path.clone(), edits })
            })
            .collect()
    }
}

/// What a language server asks to change (a fix, a formatting, its own edit) as a plan,
/// every place chosen: applied like any refactoring — open files edited (one undo step
/// each), closed ones written, files renamed — and undone at once.
pub fn plan_of(change: &code_intelligence::servers::WorkspaceChange) -> Plan {
    Plan {
        files: change
            .files
            .iter()
            .filter(|f| !f.edits.is_empty())
            .map(|f| FilePlan {
                path: f.path.clone(),
                occurrences: f
                    .edits
                    .iter()
                    .map(|e| Occurrence {
                        range: e.range.clone(),
                        original: e.original.clone(),
                        replacement: e.text.clone(),
                        certainty: Certainty::Certain,
                        chosen: true,
                        line: e.line,
                        line_text: String::new(),
                        in_line: 0..0,
                    })
                    .collect(),
            })
            .collect(),
        renames: change
            .renames
            .iter()
            .map(|(from, to)| FileRename { from: from.clone(), to: to.clone(), chosen: true })
            .collect(),
    }
}

/// A replacement in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    /// The text expected at `range` (empty: not checked).
    pub original: String,
    pub text: String,
}

/// The edits of one file (sorted, not overlapping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub path: PathBuf,
    pub edits: Vec<TextEdit>,
}

/// Why edits can't be applied to a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// An edit falls outside the text or inside a character, two overlap, or the text at
    /// an edit isn't what it was (the text changed since the plan was made).
    Stale,
}

/// `text` with `edits` (sorted by start) applied.
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> Result<String, EditError> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        let ok = edit.range.start >= at
            && edit.range.end <= text.len()
            && edit.range.start <= edit.range.end
            && text.is_char_boundary(edit.range.start)
            && text.is_char_boundary(edit.range.end)
            && (edit.original.is_empty() || text[edit.range.clone()] == edit.original);
        if !ok {
            return Err(EditError::Stale);
        }
        out.push_str(&text[at..edit.range.start]);
        out.push_str(&edit.text);
        at = edit.range.end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

/// The line (0-based), the line's text and the range within it, of `range` in `text`.
pub fn line_of(text: &str, range: &Range<usize>) -> (u32, String, Range<usize>) {
    let start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[range.start..].find('\n').map_or(text.len(), |i| range.start + i);
    let line = kernel::text::line_of(text, range.start);
    let line_text = text[start..end].trim_end_matches('\r').to_owned();
    let in_line = range.start - start..(range.end - start).min(line_text.len());
    (line, line_text, in_line)
}

/// The identifier `name` is valid for `style` (a rename's new name).
pub fn valid_name(name: &str, style: NameStyle) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| {
        c.is_alphabetic() || c == '_' || Some(c) == style.sigil || style.extra.contains(c)
    });
    first_ok && name.chars().all(|c| style.is_name_char(c))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
