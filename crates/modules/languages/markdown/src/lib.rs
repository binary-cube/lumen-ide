//! The Markdown module: the language (`.md`, `.markdown`…, a `README` without an
//! extension), its grammars (tree-sitter-grammars' Markdown: the blocks — headings, lists,
//! quotes, code blocks, tables — and, injected into each block's text, the inline one —
//! emphasis, code spans, links), its colors in the theme's own names, folding by section
//! (a heading to the next of its level or higher), code blocks, quotes and HTML.
//!
//! A fenced code block is drawn in the language its info string names (` ```php `), the
//! front matter as YAML (or TOML), the HTML as HTML. The preview is the Markdown viewer's
//! (a module of its own): this one is the text's. Its headings are its structure (the
//! Structure panel, the breadcrumb).

mod intelligence;
pub mod outline;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("markdown");
/// The text inside the blocks (a language of its own, injected: no file is written in it).
pub const INLINE: LanguageId = LanguageId::new_static("markdown_inline");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.markdown"),
    name: "Markdown",
    description: "Markdown: colors, folding by section, code blocks in their own language, headings as its structure",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "markdown", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct MarkdownModule;

impl Module for MarkdownModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        ctx.contribute(inline_grammar());
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::MarkdownIntelligence,
        ));
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("markdown", "Markdown")
        .icon_sample(4, "README.md")
        .word_chars("-")
        .extensions(&["md", "markdown", "mdown", "mkd", "mdx"])
        .file_names(&["README", "CHANGELOG", "CONTRIBUTING"])
        .icon("file-text")
}

/// The blocks.
pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_md::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: include_str!("../queries/injections.scm").into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: "".into(),
        string_content_nodes: &[],
        // Markdown has no errors: whatever is written is text.
        embedded_errors: false,
        injected_errors: false,
        own_errors: false,
    }
}

/// The text inside the blocks.
pub fn inline_grammar() -> Grammar {
    Grammar {
        language: INLINE,
        ts: tree_sitter_md::INLINE_LANGUAGE.into(),
        highlights: include_str!("../queries/inline-highlights.scm").into(),
        injections: include_str!("../queries/inline-injections.scm").into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: false,
        injected_errors: false,
        own_errors: false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
