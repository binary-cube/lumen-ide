//! How code is colored where there's no parser (code shown outside the editor: the canvas,
//! a comparison, a hover card): each language's words to color as keywords, its sigil and
//! its line comments, registered once the languages are known, looked up by a file's
//! extension. A file of a language that registered nothing is colored plainly (strings,
//! numbers, calls).

use std::sync::RwLock;

/// A language's coloring of code shown without a parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CodeStyle {
    /// Its files' extensions.
    pub extensions: &'static [&'static str],
    /// Its keywords and built-in types (lowercase).
    pub keywords: &'static [&'static str],
    /// What variables are written after (`$` in PHP).
    pub sigil: Option<char>,
    /// What starts a comment line (`//`, `#`, `/*`, `*`).
    pub comments: &'static [&'static str],
}

impl CodeStyle {
    /// `word` is one of its keywords (whatever its case).
    pub fn is_keyword(&self, word: &str) -> bool {
        self.keywords.iter().any(|k| k.eq_ignore_ascii_case(word))
    }
}

static STYLES: RwLock<Vec<CodeStyle>> = RwLock::new(Vec::new());

/// Registers a language's style (at startup, from its module).
pub fn register(style: CodeStyle) {
    let mut styles = STYLES.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    styles.retain(|s| s.extensions != style.extensions);
    styles.push(style);
}

/// The style of the files with `extension` (plain if no language registered one).
pub fn for_extension(extension: &str) -> CodeStyle {
    let styles = STYLES.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    styles
        .iter()
        .find(|s| s.extensions.iter().any(|e| e.eq_ignore_ascii_case(extension)))
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/unit/code_style.rs"]
mod tests;
