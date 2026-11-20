//! SCSS module: language, grammar (colors from the official grammar), folding.

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("scss");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.scss"),
    name: "SCSS",
    description: "SCSS: colors, folding on rules and comments",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-style", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct ScssModule;

impl Module for ScssModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(
            LanguageDescriptor::new("scss", "SCSS")
                .word_chars("-$")
                .extensions(&["scss"])
                .line_comment("//")
                .block_comment("/*", "*/")
                .icon("file-style"),
        );
        ctx.contribute(grammar());
        Ok(())
    }
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_scss::language(),
        highlights: tree_sitter_scss::HIGHLIGHTS_QUERY.into(),
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
