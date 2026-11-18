//! The templates' files as a language of their own (`extractor`, `*.extractor`): read by the
//! Twig grammar (already in the tree: its `{{ … }}` tags are ours — the fields, the filters,
//! the sections' tags colored apart from the text around them, `{# … #}` a comment), completed by the template language's own suggestions (the fields after
//! `{{`, the sections after `#` / `^` / `/`, the filters after `|`: [`TemplateAssist`]); the
//! errors are the template language's (said where they are when the file is read), never the
//! grammar's.

use code_intelligence::{
    AssistContext, CompletionContext, CompletionItem, CompletionKind, ContextKind, FileSymbols,
    FrameworkCompletion, LanguageIntelligence, Target,
};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

/// The language's id.
pub const LANGUAGE: LanguageId = LanguageId::new_static("extractor");

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("extractor", "Data Extractor Template")
        .extensions(&[extractors::template::EXTENSION])
        .block_comment("{#", "#}")
        .icon("table")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_twig::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: "".into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: false,
        injected_errors: false,
        own_errors: false,
    }
}

/// The language's intelligence: nothing indexed or resolved; completion opened by the
/// template's own characters (its suggestions are [`TemplateAssist`]'s).
#[derive(Debug, Default)]
pub struct TemplateIntelligence;

impl LanguageIntelligence for TemplateIntelligence {
    fn language(&self) -> LanguageId {
        LANGUAGE
    }

    fn features(&self) -> &'static [code_intelligence::LanguageFeature] {
        &[]
    }

    fn extensions(&self) -> &[&'static str] {
        &[]
    }

    fn extract(&self, _text: &str) -> FileSymbols {
        FileSymbols::default()
    }

    /// Nothing of the language's own (the template's suggestions come from the assist).
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

    /// `{{`, a section's `#` / `^` / `/`, a filter's `|`, a field's `.`.
    fn completion_triggers(&self) -> &'static str {
        "{#^/|."
    }
}

/// The template's suggestions at the cursor (see the module's doc).
#[derive(Debug, Default)]
pub struct TemplateAssist;

impl code_intelligence::DocumentAssist for TemplateAssist {
    fn id(&self) -> &'static str {
        "extractor-templates"
    }

    fn languages(&self) -> &'static [&'static str] {
        &["extractor"]
    }

    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        let (replace, suggestions) = extractors::template::complete(at.text, at.offset)?;
        let items = suggestions
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let kind = match s.kind {
                    extractors::template::SuggestionKind::Field => CompletionKind::Property,
                    extractors::template::SuggestionKind::Section => CompletionKind::Keyword,
                    extractors::template::SuggestionKind::Filter => CompletionKind::Function,
                };
                let mut item = CompletionItem::word(&s.label, kind, Some(&s.detail));
                item.insert = s.insert;
                item.score = 1000 - i as i32;
                item
            })
            .collect();
        Some(FrameworkCompletion { replace, items, merge: false })
    }
}

#[cfg(test)]
#[path = "../tests/unit/language.rs"]
mod tests;
