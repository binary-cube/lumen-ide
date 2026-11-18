//! The Bash module: the language (shell scripts — `.sh`, `.bash`, `.zsh`, `.command`, the
//! shells' own startup files, a `sh` / `bash` / `zsh` / `dash` / `ksh` shebang), its grammar
//! (tree-sitter's Bash), highlighting, folding, indentation, and bash-language-server as its
//! language server, described as data (`data/language-servers.toml`).
//!
//! The built-in engine is syntax only (no completion or declarations of its own): the
//! server answers the rest. ShellCheck (GPL-3.0) is never bundled: the server runs it as a
//! separate program when it's found in PATH.

use code_intelligence::env::{EnvAccess, EnvReads};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("bash");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.bash"),
    name: "Bash",
    description: "Shell scripts: colors, folding, indentation, bash-language-server as their language server",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct BashModule;

impl Module for BashModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.bash",
            toml: include_str!("../data/language-servers.toml"),
        });
        // A script's `$X` / `${X}`: an environment variable or its own (not flagged when no
        // `.env` file defines it).
        ctx.contribute(
            EnvReads::new(&["bash"], &[EnvAccess::INTERPOLATION, EnvAccess::VARIABLE]).unflagged(),
        );
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("bash", "Bash")
        .extensions(&["sh", "bash", "zsh", "ksh", "command"])
        .file_names(&[
            ".bashrc",
            ".bash_profile",
            ".bash_login",
            ".bash_logout",
            ".bash_aliases",
            ".profile",
            ".zshrc",
            ".zshenv",
            ".zprofile",
            ".zlogin",
            ".zlogout",
            "PKGBUILD",
            "APKBUILD",
        ])
        .shebangs(&["sh", "bash", "zsh", "dash", "ksh"])
        .line_comment("#")
        .icon("terminal")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_bash::LANGUAGE.into(),
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
