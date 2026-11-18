//! The highlighter of code outside the documents (a Markdown preview's fenced blocks):
//! the languages' grammars and queries, as the editor parses with, run on the caller's
//! thread (a view's background worker), never on the UI's.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use document::Document;
use syntax_ts::{Highlighter, parse_document};
use ui_workbench::snippets::SnippetHighlighter;

use crate::modules::LanguageSupport;

/// What highlights snippets with the modules' grammars.
pub struct Snippets {
    support: Arc<LanguageSupport>,
}

impl Snippets {
    /// Registers it as the app's snippet highlighter.
    pub fn register(support: Arc<LanguageSupport>) {
        ui_workbench::snippets::register(Arc::new(Self { support }));
    }
}

/// A PHP snippet without its opening tag would be read as HTML: one is put before it.
const PHP_OPEN: &str = "<?php\n";

impl SnippetHighlighter for Snippets {
    fn highlight(&self, extension: &str, code: &str) -> Option<Vec<(Range<usize>, Arc<str>)>> {
        let name = format!("snippet.{extension}");
        let language = self.support.languages.detect(Path::new(&name), code);
        if language == kernel::LanguageId::PLAIN_TEXT
            || !self.support.syntax.grammars().contains(&language)
        {
            return None;
        }
        let prefix = if language.as_str() == "php" && !code.contains("<?") { PHP_OPEN } else { "" };
        let text = format!("{prefix}{code}");
        let doc = Document::new(&text);
        let snapshot = doc.buffer().snapshot();
        let syntax = parse_document(&self.support.syntax, &language, &snapshot, None, &[], 0)?;
        let mut spans = Vec::new();
        Highlighter::new().highlight(&syntax, &snapshot, 0..text.len(), &mut spans);
        let shift = prefix.len();
        let mut names: Vec<Arc<str>> = Vec::new();
        Some(
            spans
                .into_iter()
                .filter(|s| s.range.end > shift)
                .filter_map(|s| {
                    let (_, capture) = self.support.syntax.scope(s.scope)?;
                    let name = match names.iter().find(|n| ***n == *capture) {
                        Some(n) => Arc::clone(n),
                        None => {
                            let n: Arc<str> = capture.into();
                            names.push(Arc::clone(&n));
                            n
                        }
                    };
                    Some((s.range.start.saturating_sub(shift)..s.range.end - shift, name))
                })
                .collect(),
        )
    }
}

#[cfg(test)]
#[path = "../tests/unit/snippets.rs"]
mod tests;
