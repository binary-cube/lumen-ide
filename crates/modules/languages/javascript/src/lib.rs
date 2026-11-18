//! The JavaScript family's module: JavaScript, TypeScript and TSX — languages, grammars,
//! injections (including `JSON.parse('...')`), folding, and one intelligence for all three
//! (TypeScript's grammar extends JavaScript's: the same nodes, plus types). One module,
//! not two: modules don't depend on each other, and TypeScript would repeat it all.

use std::sync::{Arc, OnceLock};

mod comment_places;
mod data;
mod definition;
mod deprecated;
mod hover;
mod intelligence;
mod quality;
mod signature;
mod unused;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::tree_sitter::Query;
use syntax_ts::{Grammar, Injection, InjectionContext, InjectionProvider, InjectionSource};

pub const LANGUAGE: LanguageId = LanguageId::new_static("javascript");
pub const TYPESCRIPT: LanguageId = LanguageId::new_static("typescript");
pub const TSX: LanguageId = LanguageId::new_static("tsx");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.javascript"),
    name: "JavaScript / TypeScript",
    description: "JavaScript, TypeScript and TSX: colors, folding, declarations, JSON in JSON.parse",
    depends_on: &[],
};

/// `process.env.X`, `process.env['X']`, `import.meta.env.X` (Vite's): the variables the
/// runtime or the bundler provide itself are never flagged undefined.
const ENV_READS: code_intelligence::env::EnvReads = {
    use code_intelligence::env::{EnvAccess, EnvReads};
    EnvReads::new(
        &["javascript", "typescript", "tsx"],
        &[
            EnvAccess::member("process.env"),
            EnvAccess::index("process.env"),
            EnvAccess::member("import.meta.env"),
            EnvAccess::index("import.meta.env"),
        ],
    )
    .providing(&["NODE_ENV", "MODE", "DEV", "PROD", "SSR", "BASE_URL"])
};

#[derive(Debug, Default)]
pub struct JavaScriptModule;

impl Module for JavaScriptModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    /// Its dictionary, read ahead of the first use.
    fn preload(&self) {
        crate::data::JS.get();
    }

    fn release_idle(&self, idle: std::time::Duration) -> bool {
        crate::data::JS.release_if_idle(idle)
    }

    fn loaded(&self) -> bool {
        crate::data::JS.is_loaded()
    }

    fn memory(&self) -> Option<u64> {
        crate::data::JS.peek(crate::data::Js::weight)
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // npm's packages (never listed: very many files) and the bundlers' output.
        ctx.contribute(kernel::ProjectDirs {
            // npm's, and the older managers' (Bower's, jspm's).
            dependencies: &["node_modules", "bower_components", "jspm_packages"],
            unlisted: &["node_modules"],
            generated: &["dist", "build"],
            ..Default::default()
        });
        // Its formatter and checker (Prettier — for CSS, JSON, YAML… too — and ESLint).
        for tool in quality::tools() {
            ctx.contribute::<std::sync::Arc<dyn code_quality::QualityTool>>(tool);
        }
        // How its code reads environment variables (followed to the `.env` files).
        ctx.contribute(ENV_READS);
        // Its parameters, for "Change Signature".
        ctx.contribute::<std::sync::Arc<dyn refactor::signature::SignatureSyntax + Send + Sync>>(
            std::sync::Arc::new(signature::JsParams),
        );
        ctx.contribute(
            LanguageDescriptor::new("javascript", "JavaScript")
                .word_chars("$")
                .quotes(&['"', '\'', '`'])
                .extensions(&["js", "mjs", "cjs", "jsx"])
                // JSX.
                .closing_tags(markup::unclosed_tag)
                .shebangs(&["node", "deno", "bun"])
                .line_comment("//")
                .block_comment("/*", "*/"),
        );
        ctx.contribute(
            LanguageDescriptor::new("typescript", "TypeScript")
                .word_chars("$")
                .quotes(&['"', '\'', '`'])
                .extensions(&["ts", "mts", "cts"])
                .shebangs(&["ts-node", "tsx", "deno", "bun"])
                .line_comment("//")
                .block_comment("/*", "*/"),
        );
        ctx.contribute(
            LanguageDescriptor::new("tsx", "TSX")
                .word_chars("$")
                .quotes(&['"', '\'', '`'])
                .extensions(&["tsx"])
                .closing_tags(markup::unclosed_tag)
                .line_comment("//")
                .block_comment("/*", "*/"),
        );
        ctx.contribute(grammar());
        ctx.contribute(typescript_grammar());
        ctx.contribute(tsx_grammar());
        for dialect in [Dialect::JavaScript, Dialect::TypeScript, Dialect::Tsx] {
            ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
                intelligence::JsIntelligence::new(dialect),
            ));
        }
        ctx.contribute::<Arc<dyn InjectionProvider>>(Arc::new(JsonParseArgument::default()));
        // JSDoc's / TSDoc's tags, types and names colored as code.
        for language in [LANGUAGE, TYPESCRIPT, TSX] {
            ctx.contribute::<Arc<dyn syntax_ts::DocComments>>(Arc::new(comment_places::DocParts(
                language,
            )));
        }
        // Its language server, as data (started, if chosen, by the language servers' module).
        ctx.contribute(code_intelligence::servers::ServerData {
            module: "lang.javascript",
            toml: include_str!("../data/language-servers.toml"),
        });
        Ok(())
    }
}

pub use intelligence::{Dialect, JsIntelligence};

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_javascript::LANGUAGE.into(),
        highlights: tree_sitter_javascript::HIGHLIGHT_QUERY.into(),
        injections: tree_sitter_javascript::INJECTIONS_QUERY.into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &["string_fragment"],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

/// TypeScript: JavaScript's highlights, then TypeScript's own (types, `interface`, …).
pub fn typescript_grammar() -> Grammar {
    Grammar {
        language: TYPESCRIPT,
        ts: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        highlights: format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        )
        .into(),
        injections: tree_sitter_javascript::INJECTIONS_QUERY.into(),
        folds: include_str!("../queries/folds-typescript.scm").into(),
        indents: include_str!("../queries/indents-typescript.scm").into(),
        string_content_nodes: &["string_fragment"],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

/// TSX: TypeScript's, plus JSX's highlights and folding.
pub fn tsx_grammar() -> Grammar {
    Grammar {
        language: TSX,
        ts: tree_sitter_typescript::LANGUAGE_TSX.into(),
        highlights: format!(
            "{}\n{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        )
        .into(),
        injections: tree_sitter_javascript::INJECTIONS_QUERY.into(),
        folds: include_str!("../queries/folds-tsx.scm").into(),
        indents: include_str!("../queries/indents-tsx.scm").into(),
        string_content_nodes: &["string_fragment"],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    }
}

/// `JSON.parse('...')`: the argument is JSON, even if it does not look like valid JSON yet.
#[derive(Debug, Default)]
pub struct JsonParseArgument {
    /// Its query, for each host grammar (JavaScript, TypeScript, TSX: each its own).
    queries: [OnceLock<Option<Query>>; 3],
}

const JSON_PARSE: &str = r#"
(call_expression
  function: (member_expression
    object: (identifier) @_object
    property: (property_identifier) @_method)
  arguments: (arguments . (string (string_fragment) @content))
  (#eq? @_object "JSON")
  (#eq? @_method "parse"))
"#;

impl InjectionProvider for JsonParseArgument {
    fn id(&self) -> &str {
        "javascript.json_parse"
    }

    fn hosts(&self) -> &[LanguageId] {
        static HOSTS: [LanguageId; 3] = [LANGUAGE, TYPESCRIPT, TSX];
        &HOSTS
    }

    fn detect(&self, ctx: &InjectionContext<'_>, out: &mut Vec<Injection>) {
        let slot = match ctx.host {
            h if *h == TYPESCRIPT => &self.queries[1],
            h if *h == TSX => &self.queries[2],
            _ => &self.queries[0],
        };
        let Some(query) = slot.get_or_init(|| Query::new(ctx.language, JSON_PARSE).ok()) else {
            return;
        };
        let Some(content) = query.capture_index_for_name("content") else { return };
        ctx.for_each_match(query, |m| {
            for c in m.captures().iter().filter(|c| c.index == content) {
                out.push(Injection {
                    ranges: vec![c.node.byte_range()],
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
