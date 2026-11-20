//! The Lua module: the language (`.lua`, `.luacheckrc`, a `lua` shebang), its grammar
//! (tree-sitter-grammars' Lua), highlighting, folding, indentation, and lua-language-server
//! as its language server, described as data (`data/language-servers.toml`).
//!
//! The built-in engine is syntax only (no completion or declarations of its own): the
//! server answers the rest.

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("lua");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.lua"),
    name: "Lua",
    description: "Lua: colors, folding, indentation, lua-language-server as its language server",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "moon", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct LuaModule;

impl Module for LuaModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // LuaRocks' local packages (`luarocks --tree lua_modules`).
        ctx.contribute(kernel::ProjectDirs {
            dependencies: &["lua_modules"],
            ..Default::default()
        });
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.lua",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("lua", "Lua")
        .extensions(&["lua"])
        .file_names(&[".luacheckrc"])
        .shebangs(&["lua", "luajit"])
        .line_comment("--")
        .block_comment("--[[", "]]")
        .icon("moon")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_lua::LANGUAGE.into(),
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
