//! The `.env` language's intelligence: its sections and keys for the Structure panel and
//! the breadcrumb, a key defined twice flagged, its keys' navigation and renaming offered in
//! its menus; and its sections' folding regions. The
//! frameworks' section rules are read from the registry the app fills
//! ([`code_intelligence::env::sections`]).

use code_intelligence::{
    Checked, CompletionContext, ContextKind, FileSymbols, LanguageIntelligence, Problem, Target,
    Types,
};
use languages::{FoldRange, LanguageId};

use crate::syntax::EnvFile;
use crate::{checks, outline};

/// A file larger than this isn't looked through (not a `.env` file anyone writes by hand).
const MAX_TEXT: usize = 1 << 20;

#[derive(Debug, Default)]
pub struct DotenvIntelligence;

impl LanguageIntelligence for DotenvIntelligence {
    fn language(&self) -> LanguageId {
        crate::LANGUAGE
    }

    /// Its keys' definitions, usages and renaming (the module's own: [`crate::rename`]).
    fn features(&self) -> &'static [code_intelligence::LanguageFeature] {
        code_intelligence::LanguageFeature::NAMES
    }

    /// Not indexed: an open file's own keys are what's shown.
    fn extensions(&self) -> &[&'static str] {
        &[]
    }

    fn extract(&self, text: &str) -> FileSymbols {
        if text.len() > MAX_TEXT {
            return FileSymbols::default();
        }
        outline::symbols(text, &code_intelligence::env::sections())
    }

    /// No completion of its own.
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
        &["export"]
    }

    /// A key defined again in the same file.
    fn check(&self, text: &str, _types: &Types<'_>) -> Checked {
        if text.len() > MAX_TEXT {
            return Checked::default();
        }
        let problems = checks::duplicates(text, &EnvFile::parse(text))
            .into_iter()
            .map(|range| Problem {
                name: text[range.clone()].to_owned(),
                range,
                kind: "env_duplicate_key",
                arguments: None,
            })
            .collect();
        Checked { problems, ..Checked::default() }
    }
}

/// The sections' folding regions (`N keys`), beside the grammar's.
#[derive(Debug, Default)]
pub struct SectionFolds;

impl syntax_ts::FoldProvider for SectionFolds {
    fn language(&self) -> LanguageId {
        crate::LANGUAGE
    }

    fn folds(&self, text: &str) -> Vec<FoldRange> {
        if text.len() > MAX_TEXT {
            return Vec::new();
        }
        outline::folds(text, &code_intelligence::env::sections())
    }
}

#[cfg(test)]
#[path = "../tests/unit/intelligence.rs"]
mod tests;
