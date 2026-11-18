//! PHP module: language, grammar, injections (HTML, heredocs, JSON), folding.

mod callables;
mod checks;
mod comment_places;
mod docblock;
mod doctree;
mod hover;
mod infer;
mod intelligence;
mod interpreter;
mod level_check;
mod locals;
mod namespaces;
mod parameters;
mod phpdoc;
mod quality;
mod requirements;
mod signature;
mod stubs;
mod template_vars;
mod types;
mod typing;
mod unused;
mod version;

use std::sync::{Arc, OnceLock};

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::tree_sitter::Query;
use syntax_ts::{Grammar, Injection, InjectionContext, InjectionProvider, InjectionSource};

pub const LANGUAGE: LanguageId = LanguageId::new_static("php");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.php"),
    name: "PHP",
    description: "PHP: colors, embedded HTML / JS / CSS / JSON, heredocs, folding",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct PhpModule;

impl Module for PhpModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    /// Its stubs, read ahead of the first use.
    fn preload(&self) {
        crate::stubs::STUBS.get();
    }

    fn release_idle(&self, idle: std::time::Duration) -> bool {
        let stubs = crate::stubs::STUBS.release_if_idle(idle);
        // The documents' trees go with the stubs (nothing asked of PHP for a while).
        let trees = stubs && crate::doctree::release();
        stubs || trees
    }

    fn loaded(&self) -> bool {
        crate::stubs::STUBS.is_loaded()
    }

    fn memory(&self) -> Option<u64> {
        // The stubs, and the documents' texts kept with their trees.
        crate::stubs::STUBS.peek(crate::stubs::Stubs::weight).map(|w| w + crate::doctree::weight())
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Composer's libraries: in `vendor`, or where `composer.json` puts them.
        ctx.contribute(kernel::ProjectDirs {
            dependencies: &["vendor"],
            found_by: &[kernel::DependencyFinder {
                manifest: "composer.json",
                folders: composer_vendor_dir,
            }],
            ..Default::default()
        });
        // Asking the PHP found for its version: its threads count on this module's row.
        ctx.contribute(workbench_model::ModuleThreads(&["php-input", "php-output", DETECT_THREAD]));
        ctx.contribute(
            LanguageDescriptor::new("php", "PHP")
                .word_chars("$")
                // Its HTML (outside `<?php`).
                .closing_tags(markup::unclosed_tag)
                .extensions(&[
                    "php", "phtml", "inc", "php3", "php4", "php5", "php7", "php8", "phps",
                ])
                .file_names(&[".php-cs-fixer.php", ".php-cs-fixer.dist.php"])
                .shebangs(&["php"])
                .line_comment("//")
                .block_comment("/*", "*/"),
        );
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn InjectionProvider>>(Arc::new(JsonFunctionArgument::default()));
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            intelligence::PhpIntelligence::default(),
        ));
        // PHPDoc's tags, types and names colored as code.
        ctx.contribute::<Arc<dyn syntax_ts::DocComments>>(Arc::new(comment_places::DocParts));
        // The classes named by `@var` in Twig's and Blade's comments.
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            template_vars::TemplateVars,
        ));
        // Its formatters and checkers (PHP CS Fixer, Pint, PHP_CodeSniffer, PHPStan).
        for tool in quality::tools() {
            ctx.contribute::<Arc<dyn code_quality::QualityTool>>(tool);
        }
        // Its parameters, for "Change Signature".
        ctx.contribute::<Arc<dyn refactor::signature::SignatureSyntax + Send + Sync>>(Arc::new(
            signature::PhpParams,
        ));
        ctx.contribute::<Arc<dyn refactor::moving::NamespaceRules>>(Arc::new(
            namespaces::PhpNamespaces,
        ));
        // Its language server (phpactor), as data (started, if chosen, by the language
        // servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.php",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub use intelligence::PhpIntelligence;
pub use namespaces::PhpNamespaces;

/// Where Composer installs the libraries, if `composer.json` says (`config.vendor-dir`,
/// relative to the project): its `vendor` otherwise, the module's fixed name.
pub fn composer_vendor_dir(composer: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(composer) else { return Vec::new() };
    json.pointer("/config/vendor-dir")
        .and_then(serde_json::Value::as_str)
        .map(|dir| dir.trim().trim_start_matches("./").trim_end_matches('/').replace('\\', "/"))
        .filter(|dir| {
            !dir.is_empty() && dir != "vendor" && !dir.starts_with('/') && !dir.contains("..")
        })
        .into_iter()
        .collect()
}

/// The thread finding the installed PHP (its version) at startup.
const DETECT_THREAD: &str = "php-detect";

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_php::LANGUAGE_PHP.into(),
        // Attributes and named arguments first, then the stock query.
        highlights: format!(
            "{}\n{}",
            languages::shared_queries::PHP_HIGHLIGHTS,
            tree_sitter_php::HIGHLIGHTS_QUERY
        )
        .into(),
        injections: include_str!("../queries/injections.scm").into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &["string_content"],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

/// First argument of functions that take JSON (`json_decode`, `json_validate`).
#[derive(Debug, Default)]
pub struct JsonFunctionArgument {
    query: OnceLock<Option<Query>>,
}

const JSON_FUNCTIONS: &str = r#"
(function_call_expression
  function: (name) @_function
  arguments: (arguments . (argument [
    (string (string_content) @content)
    (encapsed_string (string_content) @content)
  ]))
  (#match? @_function "^json_(decode|validate)$"))
"#;

impl InjectionProvider for JsonFunctionArgument {
    fn id(&self) -> &str {
        "php.json_functions"
    }

    fn hosts(&self) -> &[LanguageId] {
        static HOSTS: [LanguageId; 1] = [LANGUAGE];
        &HOSTS
    }

    fn detect(&self, ctx: &InjectionContext<'_>, out: &mut Vec<Injection>) {
        let Some(query) = self.query.get_or_init(|| Query::new(ctx.language, JSON_FUNCTIONS).ok())
        else {
            return;
        };
        let Some(content) = query.capture_index_for_name("content") else { return };
        ctx.for_each_match(query, |m| {
            let ranges: Vec<_> = m
                .captures()
                .iter()
                .filter(|c| c.index == content)
                .map(|c| c.node.byte_range())
                .collect();
            if !ranges.is_empty() {
                out.push(Injection {
                    ranges,
                    language: LanguageId::new_static("json"),
                    source: InjectionSource::Rule,
                });
            }
        });
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
