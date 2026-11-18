//! SQL's intelligence without a database: the words of the file's dialect (keywords, built-in
//! functions inserted as calls), nothing offered in comments, strings and quoted names. What
//! a schema holds (tables, columns) comes from the database modules, for the documents they
//! know.

use std::borrow::Cow;
use std::path::Path;

use code_intelligence::{
    CompletionContext, ContextKind, FileSymbols, KeywordInsert, LanguageIntelligence, Naming,
    Target,
};
use languages::LanguageId;

use crate::dialect::{Dialect, STANDARD, VALUES};

#[derive(Debug, Default)]
pub struct SqlIntelligence;

/// What a name holds besides letters, digits and `_`.
const WORD_CHARS: &str = "$";

impl LanguageIntelligence for SqlIntelligence {
    fn language(&self) -> LanguageId {
        crate::LANGUAGE
    }

    /// Nothing declared to index (a schema's names are the database's).
    fn extensions(&self) -> &[&'static str] {
        &[]
    }

    /// Nothing resolved nor refactored: its names are the database's (the consoles' own
    /// rows come from the database modules).
    fn features(&self) -> &'static [code_intelligence::LanguageFeature] {
        &[]
    }

    fn extract(&self, _text: &str) -> FileSymbols {
        FileSymbols::default()
    }

    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        context(text, offset)
    }

    fn target(&self, _text: &str, _offset: usize) -> Option<Target> {
        None
    }

    /// A known function of the file's dialect (by its name): its signature, for a call's
    /// parameter help in any SQL file (a console's source answers first).
    fn call_signature(&self, path: &Path, name: &str) -> Option<String> {
        let flavor = Dialect::of_path(path).flavor();
        database::completion::builtin_signature(name, flavor).map(str::to_owned)
    }

    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }

    /// The standard's (where the file's dialect isn't known).
    fn keywords(&self) -> &[&'static str] {
        STANDARD
    }

    /// The keywords and functions of the file's dialect, by its name.
    fn keywords_for(&self, path: &Path) -> Cow<'_, [&'static str]> {
        Cow::Owned(crate::functions::get().of(Dialect::of_path(path)).to_vec())
    }

    /// A function as a call (`COUNT()`, the cursor between the parentheses), a value as
    /// written (`NULL`), any other keyword followed by a space.
    fn keyword_insert(&self, keyword: &str) -> KeywordInsert {
        if VALUES.contains(&keyword) {
            KeywordInsert::Exact
        } else if crate::functions::get().is_call(keyword) {
            KeywordInsert::Call
        } else {
            KeywordInsert::Spaced
        }
    }

    /// `alias.` (columns, 39j), a quoted name being written, a MySQL / SQLite backticked one,
    /// a string literal opened (an enum column's values, 48b).
    fn completion_triggers(&self) -> &'static str {
        ".\"`'"
    }

    fn word_chars(&self) -> &'static str {
        WORD_CHARS
    }

    /// Unquoted names ignore case.
    /// SQL shown without a parser (a console's statements in its log, a diff): the
    /// standard's words, `--` comments.
    fn code_style(&self) -> Option<code_intelligence::code_style::CodeStyle> {
        Some(code_intelligence::code_style::CodeStyle {
            extensions: &["sql", "ddl", "pgsql", "psql"],
            keywords: STANDARD,
            sigil: None,
            comments: &["--"],
        })
    }

    fn naming(&self) -> Naming {
        Naming { names_ignore_case: true, methods_ignore_case: true, ..Naming::PLAIN }
    }
}

/// The word being typed at `offset` and whether keywords fit there: not in a comment, a
/// string or a quoted name, not after `.` (a column: the schema's), not a parameter or a
/// number. Read from the current line (and a block comment still open before it), so it
/// holds for SQL embedded in another language (a PHP heredoc).
pub fn context(text: &str, offset: usize) -> CompletionContext {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || WORD_CHARS.contains(c);
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let prefix = &text[start..offset];
    let before = text[..start].chars().next_back();
    let none = |prefix: &str| CompletionContext {
        replace: start..offset,
        prefix: prefix.to_owned(),
        kind: ContextKind::None,
    };
    let quiet = prefix.is_empty()
        || prefix.starts_with(|c: char| c.is_ascii_digit() || c == '$')
        || matches!(before, Some('.' | ':' | '@' | '?'))
        || in_comment_or_string(text, start);
    if quiet {
        return none(prefix);
    }
    CompletionContext { replace: start..offset, prefix: prefix.to_owned(), kind: ContextKind::Name }
}

/// `offset` is in a comment, a string or a quoted name: the line's `--` comment or open
/// quote before it, or a `/* … */` opened before it and not closed.
pub fn in_comment_or_string(text: &str, offset: usize) -> bool {
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    // A block comment opened on an earlier line and not closed before `offset`.
    if let Some(open) = text[..line_start].rfind("/*")
        && !text[open + 2..offset].contains("*/")
    {
        return true;
    }
    let mut quote: Option<char> = None;
    let mut block = false;
    let mut chars = text[line_start..offset].chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, block, c) {
            (Some(q), _, c) if c == q => quote = None,
            (Some(_), _, _) => {}
            (None, true, '*') if chars.peek() == Some(&'/') => {
                chars.next();
                block = false;
            }
            (None, true, _) => {}
            (None, false, '-') if chars.peek() == Some(&'-') => return true,
            (None, false, '/') if chars.peek() == Some(&'*') => {
                chars.next();
                block = true;
            }
            (None, false, '\'' | '"' | '`') => quote = Some(c),
            _ => {}
        }
    }
    quote.is_some() || block
}

#[cfg(test)]
#[path = "../tests/unit/intelligence.rs"]
mod tests;
