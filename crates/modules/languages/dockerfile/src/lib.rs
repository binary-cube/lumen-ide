//! The Dockerfile module: the language (`Dockerfile`, `Containerfile`, `Dockerfile.*`,
//! `*.dockerfile`), its grammar (WharfLab's maintained Containerfile grammar), highlighting
//! (the shell commands of `RUN` / `CMD` / `ENTRYPOINT` as Bash, when that module is there),
//! folding, and dockerfile-language-server as its language server, described as data
//! (`data/language-servers.toml`).
//!
//! Compose files are YAML: the YAML module's server checks them with the bundled compose
//! schema.

use code_intelligence::env::{EnvAccess, EnvReads};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("dockerfile");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.dockerfile"),
    name: "Dockerfile",
    description: "Dockerfile: colors, the shell commands as Bash, folding, dockerfile-language-server as its language server",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "docker", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct DockerfileModule;

impl Module for DockerfileModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.dockerfile",
            toml: include_str!("../data/language-servers.toml"),
        });
        // Docker Compose's interpolation (`${TAG:-latest}`, `$PORT`), from its `.env` file
        // or the shell: not flagged when no `.env` file defines it.
        ctx.contribute(
            EnvReads::new(&["yaml"], &[EnvAccess::INTERPOLATION, EnvAccess::VARIABLE])
                .in_files(&[
                    "compose*.yaml",
                    "compose*.yml",
                    "docker-compose*.yaml",
                    "docker-compose*.yml",
                ])
                .unflagged(),
        );
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("dockerfile", "Dockerfile")
        .file_names(&["Dockerfile", "Containerfile"])
        // On the name lowercased: `dockerfile`, `Dockerfile.dev`, `api.dockerfile`.
        .globs(&["dockerfile", "containerfile", "dockerfile.*", "containerfile.*", "*.dockerfile"])
        .extensions(&["dockerfile", "containerfile"])
        .line_comment("#")
        .icon("docker")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_containerfile::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: include_str!("../queries/injections.scm").into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: false,
        own_errors: true,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
