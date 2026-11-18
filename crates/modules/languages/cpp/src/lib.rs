//! The C / C++ module: both languages (C: `.c`; C++: `.cpp`, `.cc`, `.cxx`, `.c++`, the
//! headers `.h`, `.hpp`, `.hh`, `.hxx`, `.h++`, `.ipp`, `.tpp`, `.inl`), their grammars
//! (tree-sitter's C and C++), highlighting, folding, indentation, and clangd as their
//! language server, described as data (`data/language-servers.toml`). One module, not two:
//! C++'s grammar extends C's, and the two share a server.
//!
//! `.h` is read as C++: a header may be included from either language, and the C++ grammar
//! reads C headers well where the C grammar can't read C++ ones (classes, namespaces,
//! templates). clangd decides how it compiles a header itself (from the compilation
//! database: the flags of a file including it), whatever the editor calls it.
//!
//! The built-in engine is syntax only (no completion or declarations of its own): the
//! server answers the rest.

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const C: LanguageId = LanguageId::new_static("c");
pub const CPP: LanguageId = LanguageId::new_static("cpp");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.cpp"),
    name: "C / C++",
    description: "C and C++: colors, folding, indentation, clangd as their language server",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct CppModule;

impl Module for CppModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // CMake's and CLion's build folders, clangd's index: not searched as text.
        ctx.contribute(kernel::ProjectDirs {
            dependencies: &[],
            unlisted: &[],
            generated: &["build", "cmake-build-debug", "cmake-build-release", ".cache/clangd"],
            ..Default::default()
        });
        ctx.contribute(c_descriptor());
        ctx.contribute(cpp_descriptor());
        ctx.contribute(c_grammar());
        ctx.contribute(cpp_grammar());
        // Their language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.cpp",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub fn c_descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("c", "C")
        .extensions(&["c"])
        .line_comment("//")
        .block_comment("/*", "*/")
        .icon("file-code")
}

pub fn cpp_descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("cpp", "C++")
        .extensions(&[
            "cpp", "cc", "cxx", "c++", "h", "hpp", "hh", "hxx", "h++", "ipp", "tpp", "inl",
        ])
        .line_comment("//")
        .block_comment("/*", "*/")
        .icon("file-code")
}

pub fn c_grammar() -> Grammar {
    Grammar {
        language: C,
        ts: tree_sitter_c::LANGUAGE.into(),
        // Its own patterns, then what C and C++ share (only nodes both grammars have): on
        // the same node the first pattern wins.
        highlights: concat!(
            include_str!("../queries/highlights-c.scm"),
            "\n",
            include_str!("../queries/highlights-common.scm")
        )
        .into(),
        injections: "".into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        // Macros the grammar can't expand make it see errors in valid code: the server's
        // diagnostics are the real ones.
        embedded_errors: false,
        injected_errors: false,
        own_errors: false,
    }
}

pub fn cpp_grammar() -> Grammar {
    Grammar {
        language: CPP,
        ts: tree_sitter_cpp::LANGUAGE.into(),
        highlights: concat!(
            include_str!("../queries/highlights-cpp.scm"),
            "\n",
            include_str!("../queries/highlights-common.scm")
        )
        .into(),
        injections: "".into(),
        folds: concat!(
            include_str!("../queries/folds.scm"),
            "\n",
            include_str!("../queries/folds-cpp.scm")
        )
        .into(),
        indents: concat!(
            include_str!("../queries/indents.scm"),
            "\n",
            include_str!("../queries/indents-cpp.scm")
        )
        .into(),
        string_content_nodes: &[],
        embedded_errors: false,
        injected_errors: false,
        own_errors: false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
