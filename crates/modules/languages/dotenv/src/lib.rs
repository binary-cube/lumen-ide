//! The `.env` module: the language (`.env`, `.env.*` — `.env.local`, `.env.prod`,
//! `.env.example`… —, `*.env`), its grammar (pnx's tree-sitter-dotenv, vendored),
//! highlighting (keys, values by what they look like, the variables they interpolate,
//! comments, `export`), the secret-looking values hidden until hovered (a setting), its
//! sections and keys in the Structure panel and the breadcrumb, folded (`N keys`), a key
//! defined twice flagged; and its keys followed through the project's code — Go to
//! Definition (a list in the order the files are loaded), the hover card, Find Usages,
//! Rename, a variable read but defined nowhere, a key read nowhere (a setting).
//!
//! What other modules know comes through [`code_intelligence::env`]: the frameworks' section
//! markers ([`code_intelligence::env::EnvSections`]) and load orders
//! ([`code_intelligence::env::EnvFileOrder`]), the ways each language reads a variable
//! ([`EnvReads`]; PHP's own are contributed here, keyed by its id). The module knows no
//! framework.
//!
//! The grammar colors the file; the module's own reading of it ([`syntax`]) is what
//! everything else stands on.

pub mod catalog;
pub mod checks;
pub mod conceal;
pub mod intelligence;
pub mod keys;
pub mod navigation;
pub mod order;
pub mod outline;
pub mod reads;
pub mod rename;
pub mod sections;
pub mod service;
pub mod settings;
pub mod syntax;
pub mod used;
pub mod values;

use std::sync::Arc;

use code_intelligence::env::{EnvAccess, EnvReads};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

/// The language's id.
pub const ID: &str = "dotenv";
pub const LANGUAGE: LanguageId = LanguageId::new_static(ID);
/// The language's name, as the user reads it.
pub const NAME: &str = ".env";
/// The threads its background work runs on (reading the `.env` files, the checks).
pub const THREAD: &str = "dotenv";

/// How PHP itself reads a variable: `getenv('X')`, `$_ENV['X']`, `$_SERVER['X']`.
const PHP_READS: &[EnvAccess] =
    &[EnvAccess::call("getenv"), EnvAccess::index("$_ENV"), EnvAccess::index("$_SERVER")];

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.dotenv"),
    name: ".env",
    description: ".env files: colors, secrets hidden until hovered, sections, folding; keys followed through the code: definitions, hover, usages, rename, checks",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct DotenvModule;

impl Module for DotenvModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::DotenvIntelligence,
        ));
        ctx.contribute::<Arc<dyn syntax_ts::FoldProvider>>(Arc::new(intelligence::SectionFolds));
        // Its keys followed through the code: definitions, hover, usages, rename, checks.
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            navigation::Navigation,
        ));
        ctx.contribute::<Arc<dyn workbench_model::CursorSymbols>>(Arc::new(rename::Keys));
        ctx.contribute::<Arc<dyn workbench_model::ProjectService>>(Arc::new(
            service::EnvService::default(),
        ));
        ctx.contribute(workbench_model::ModuleThreads(&[THREAD]));
        // PHP's own ways of reading a variable (its module needn't know `.env` files).
        ctx.contribute(EnvReads::new(&["php"], PHP_READS));
        ctx.contribute::<Arc<dyn workbench_model::Concealment>>(Arc::new(conceal::Secrets));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::DotenvSettings,
        ));
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new(ID, NAME)
        .file_names(&[".env"])
        // `.env.local`, `.env.prod.local`, `.env.example`… (not `.env.local.php`: another
        // language's extension wins over an open glob).
        .globs(&[".env.*"])
        // `docker.env`, `app.env`.
        .extensions(&["env"])
        .line_comment("#")
        .icon("key")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_dotenv::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: "".into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        embedded_errors: false,
        injected_errors: false,
        // A permissive grammar over the loaders' dialects (Node's `KEY=a b`): no false
        // alarms in the user's own files.
        own_errors: false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/fixture.rs"]
mod fixture;
