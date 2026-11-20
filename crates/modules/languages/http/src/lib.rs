//! The `.http` module (plan 77c): HTTP request files in the syntax shared by HTTP
//! clients (`.http`, `.rest`), their grammar (rest-nvim's tree-sitter-http,
//! vendored), the colors — the request line, the headers, the variables, the comments'
//! directives, a body from a file — with a request's body in its own language (JSON, XML)
//! and its scripts in JavaScript, folded per request; its requests and variables in the
//! Structure panel ([`outline`]); completion ([`assist`]: methods, headers and their values,
//! the comments' directives, the variables — the file's, its environments', the dynamic
//! ones). What a request is is read by the domain's `http-requests`, not by the tree.

pub mod assist;
pub mod complete;
pub mod hover;
pub mod intelligence;
pub mod outline;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

/// The language's id.
pub const ID: &str = "http";
pub const LANGUAGE: LanguageId = LanguageId::new_static(ID);
/// The language's name, as the user reads it.
pub const NAME: &str = "HTTP Request";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.http"),
    name: "HTTP requests (language)",
    description: ".http files: colors (the body in its own language, the scripts in JavaScript), folding per request, structure, completion",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "globe", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct HttpModule;

impl Module for HttpModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::HttpIntelligence,
        ));
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            assist::Completion::default(),
        ));
        ctx.contribute(workbench_model::ModuleThreads(&[assist::THREAD]));
        Ok(())
    }
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new(ID, NAME).extensions(&["http", "rest"]).line_comment("#").icon("globe")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_http::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        injections: include_str!("../queries/injections.scm").into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        embedded_errors: false,
        injected_errors: false,
        // A grammar over two clients' dialects: no false alarms in the user's own files.
        own_errors: false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
