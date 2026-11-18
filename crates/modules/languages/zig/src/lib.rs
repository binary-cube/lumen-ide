//! The Zig module: the language (`.zig`, and `.zon`, Zig's object notation), its grammar
//! (tree-sitter-grammars' Zig), highlighting, folding, indentation, and ZLS as its language
//! server, described as data (`data/language-servers.toml`).
//!
//! The built-in engine is syntax only (no completion or declarations of its own): the
//! server answers the rest.

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("zig");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.zig"),
    name: "Zig",
    description: "Zig: colors, folding, indentation, ZLS as its language server",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct ZigModule;

impl Module for ZigModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // The build's cache (very many files) and its output.
        ctx.contribute(kernel::ProjectDirs {
            dependencies: &[],
            unlisted: &[".zig-cache", "zig-cache"],
            generated: &[".zig-cache", "zig-cache", "zig-out"],
            ..Default::default()
        });
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.zig",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("zig", "Zig").extensions(&["zig", "zon"]).line_comment("//").icon("zap")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_zig::LANGUAGE.into(),
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
