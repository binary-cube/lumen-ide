//! YAML module: language, grammar, folding by section (with the number of keys / items), and
//! Red Hat's yaml-language-server as its language server, described as data
//! (`data/language-servers.toml`), with the schemas of the files most projects have
//! (GitHub's workflows, Docker Compose…) bundled and nothing ever downloaded.

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("yaml");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.yaml"),
    name: "YAML",
    description: "YAML: colors, folding on keys and lists, yaml-language-server as its language server",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-text", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct YamlModule;

impl Module for YamlModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(
            LanguageDescriptor::new("yaml", "YAML")
                .word_chars("-")
                .extensions(&["yaml", "yml"])
                .indent_after(&[':'])
                .file_names(&[".clang-format", ".clang-tidy"])
                .line_comment("#")
                .icon("file-text"),
        );
        ctx.contribute(grammar());
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.yaml",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_yaml::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
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
