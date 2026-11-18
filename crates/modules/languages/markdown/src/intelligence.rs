//! Markdown's intelligence: its headings for the Structure panel and the breadcrumb
//! ([`crate::outline`]). Nothing else: no names to resolve or rename, no completion, not
//! indexed (an open file's own headings are what's shown).

use code_intelligence::{
    CompletionContext, ContextKind, FileSymbols, LanguageIntelligence, Target,
};
use languages::LanguageId;

/// A file larger than this isn't looked through (no document anyone reads by its outline).
const MAX_TEXT: usize = 8 << 20;

#[derive(Debug, Default)]
pub struct MarkdownIntelligence;

impl LanguageIntelligence for MarkdownIntelligence {
    fn language(&self) -> LanguageId {
        crate::LANGUAGE
    }

    /// No menu rows: headings aren't names to follow or rename.
    fn features(&self) -> &'static [code_intelligence::LanguageFeature] {
        &[]
    }

    fn extensions(&self) -> &[&'static str] {
        &[]
    }

    fn extract(&self, text: &str) -> FileSymbols {
        if text.len() > MAX_TEXT {
            return FileSymbols::default();
        }
        crate::outline::symbols(text)
    }

    fn context(&self, _text: &str, offset: usize) -> CompletionContext {
        CompletionContext {
            replace: offset..offset,
            prefix: String::new(),
            kind: ContextKind::None,
        }
    }

    fn target(&self, _text: &str, _offset: usize) -> Option<Target> {
        None
    }

    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }

    fn keywords(&self) -> &[&'static str] {
        &[]
    }
}
