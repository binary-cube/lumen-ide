//! CSS module: language, grammar (colors from the official grammar), folding.

mod comment_places;
mod data;
mod deprecated;
mod hover;
mod intelligence;
mod quality;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("css");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.css"),
    name: "CSS",
    description: "CSS: colors, folding on rules and comments",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-style", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct CssModule;

impl Module for CssModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    /// Its dictionary, read ahead of the first use.
    fn preload(&self) {
        crate::data::CSS.get();
    }

    fn release_idle(&self, idle: std::time::Duration) -> bool {
        crate::data::CSS.release_if_idle(idle)
    }

    fn loaded(&self) -> bool {
        crate::data::CSS.is_loaded()
    }

    fn memory(&self) -> Option<u64> {
        crate::data::CSS.peek(crate::data::Css::weight)
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(
            LanguageDescriptor::new("css", "CSS")
                .icon_sample(2, "style.css")
                .word_chars("-")
                .extensions(&["css"])
                .block_comment("/*", "*/")
                .icon("file-style"),
        );
        ctx.contribute(grammar());
        // CSS and SCSS share the same intelligence (selectors, variables, properties); SCSS
        // adds `$variables` and mixins.
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::CssIntelligence::css(),
        ));
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::CssIntelligence::scss(),
        ));
        // SassDoc's tags, types and names colored as code (SCSS's grammar is its module's:
        // the reader is matched by language).
        ctx.contribute::<Arc<dyn syntax_ts::DocComments>>(Arc::new(comment_places::SassDocParts));
        // Its checker (Stylelint; formatting is Prettier's, from the JavaScript module).
        for tool in quality::tools() {
            ctx.contribute::<Arc<dyn code_quality::QualityTool>>(tool);
        }
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.css",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub use intelligence::CssIntelligence;

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_css::LANGUAGE.into(),
        highlights: tree_sitter_css::HIGHLIGHTS_QUERY.into(),
        injections: "".into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
