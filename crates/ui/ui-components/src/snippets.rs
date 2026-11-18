//! Highlighting for code outside the editor's documents (a Markdown preview's fenced
//! blocks): the same parser and queries as the editor, behind a port the app fills at
//! startup (it holds the grammars). A view asks from a background thread and colors the
//! captures with its palette ([`crate::SyntaxColors::capture`]).

use std::ops::Range;
use std::sync::{Arc, RwLock};

/// What highlights a piece of code.
pub trait SnippetHighlighter: Send + Sync {
    /// The captures of `code` written in the language of files with `extension` (`rs`,
    /// `php`), in the order they apply (later ones over earlier ones): byte ranges and the
    /// capture's name (`keyword`, `string.special`). `None`: no grammar for it.
    fn highlight(&self, extension: &str, code: &str) -> Option<Vec<(Range<usize>, Arc<str>)>>;
}

static HIGHLIGHTER: RwLock<Option<Arc<dyn SnippetHighlighter>>> = RwLock::new(None);

/// Registers the highlighter (the app, at startup).
pub fn register(highlighter: Arc<dyn SnippetHighlighter>) {
    if let Ok(mut h) = HIGHLIGHTER.write() {
        *h = Some(highlighter);
    }
}

/// The highlighter, if the app registered one.
pub fn highlighter() -> Option<Arc<dyn SnippetHighlighter>> {
    HIGHLIGHTER.read().ok().and_then(|h| h.clone())
}
