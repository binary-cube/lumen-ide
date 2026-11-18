//! The `.http` language's intelligence: its requests and variables for the Structure panel
//! and the breadcrumb. Completion is the module's assist ([`crate::assist`]: it knows the
//! environments).

use code_intelligence::{
    CompletionContext, ContextKind, FileSymbols, LanguageIntelligence, Target,
};
use languages::LanguageId;

/// A file larger than this isn't looked through (not one anyone writes by hand).
const MAX_TEXT: usize = 4 << 20;

#[derive(Debug, Default)]
pub struct HttpIntelligence;

impl LanguageIntelligence for HttpIntelligence {
    fn language(&self) -> LanguageId {
        crate::LANGUAGE
    }

    /// Not indexed: an open file's own requests are what's shown.
    fn extensions(&self) -> &[&'static str] {
        &[]
    }

    fn extract(&self, text: &str) -> FileSymbols {
        if text.len() > MAX_TEXT {
            return FileSymbols::default();
        }
        crate::outline::symbols(text)
    }

    /// The assist completes (it has the environments).
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

    fn completion_triggers(&self) -> &'static str {
        "{@:"
    }
}
