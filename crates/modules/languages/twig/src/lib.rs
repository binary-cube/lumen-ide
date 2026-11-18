//! Twig module: Twig templates (Symfony) are HTML with `{{ }}`, `{% %}`, `{# #}` directives.
//!
//! The Twig grammar parses the directives; the rest of the file is injected as HTML (a single
//! document), so CSS, JS and JSON in it get colors and folding through their modules.
//! Blocks (`{% block %}…{% endblock %}`, `if`, `for`, `macro`…) can be collapsed.

mod assist;
mod variables;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{IconSvg, LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("twig");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.twig"),
    name: "Twig",
    description: "Twig: colors, embedded HTML / CSS / JS, folding on blocks",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct TwigModule;

impl Module for TwigModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(IconSvg { name: "file-twig", svg: include_str!("../svg/file-twig.svg") });
        ctx.contribute(
            LanguageDescriptor::new("twig", "Twig")
                .word_chars("-")
                .extensions(&["twig"])
                .block_comment("{#", "#}")
                .closing_tags(markup::unclosed_tag)
                .icon("file-twig"),
        );
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            assist::TwigAssist::default(),
        ));
        Ok(())
    }
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_twig::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: include_str!("../queries/injections.scm").into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
