//! HTML module: language, grammar, injections (JavaScript, CSS, JSON), folding on elements.

mod data;
mod deprecated;
mod hover;
mod intelligence;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("html");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.html"),
    name: "HTML",
    description: "HTML: colors, embedded JS / CSS / JSON, folding on elements",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-code", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct HtmlModule;

impl Module for HtmlModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    /// Its dictionary, read ahead of the first use.
    fn preload(&self) {
        crate::data::HTML.get();
    }

    fn release_idle(&self, idle: std::time::Duration) -> bool {
        crate::data::HTML.release_if_idle(idle)
    }

    fn loaded(&self) -> bool {
        crate::data::HTML.is_loaded()
    }

    fn memory(&self) -> Option<u64> {
        crate::data::HTML.peek(crate::data::Html::weight)
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(
            LanguageDescriptor::new("html", "HTML")
                .word_chars("-")
                .extensions(&["html", "htm", "xhtml", "vue"])
                .block_comment("<!--", "-->")
                .closing_tags(markup::unclosed_tag),
        );
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::HtmlIntelligence,
        ));
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.html",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub use intelligence::HtmlIntelligence;

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_html::LANGUAGE.into(),
        // Templates' directives (`v-if`, `:class`, `@click`, `hx-get`) first, then HTML's.
        highlights: format!(
            "{}\n{}",
            include_str!("../queries/directives.scm"),
            tree_sitter_html::HIGHLIGHTS_QUERY
        )
        .into(),
        injections: languages::shared_queries::HTML_INJECTIONS.into(),
        folds: languages::shared_queries::HTML_FOLDS.into(),
        indents: languages::shared_queries::HTML_INDENTS.into(),
        // Attribute values (e.g. `data-config='{...}'`) can contain JSON.
        string_content_nodes: &["attribute_value"],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
