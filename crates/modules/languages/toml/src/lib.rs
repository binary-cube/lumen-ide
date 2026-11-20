//! The TOML module: the language (`.toml`, and the files written in it without its
//! extension: `Cargo.lock`, `uv.lock`, `poetry.lock`, `Pipfile`), its grammar
//! (tree-sitter-grammars' TOML), highlighting, folding, indentation, and taplo as its
//! language server, described as data (`data/language-servers.toml`), with its schemas
//! bundled (Cargo.toml, pyproject.toml…).
//!
//! The built-in engine is syntax only (no completion or declarations of its own): the
//! server answers the rest.

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("toml");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.toml"),
    name: "TOML",
    description: "TOML: colors, folding, indentation, taplo as its language server",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "settings", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct TomlModule;

impl Module for TomlModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.toml",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("toml", "TOML")
        .word_chars("-")
        .extensions(&["toml"])
        // Files written in TOML without its extension. (Not Cargo's old `.cargo/config`: a
        // file named `config` is just as often Git's or SSH's.)
        .file_names(&["Cargo.lock", "uv.lock", "poetry.lock", "Pipfile"])
        .line_comment("#")
        .icon("settings")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_toml_ng::LANGUAGE.into(),
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
