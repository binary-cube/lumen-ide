//! Blade module (Laravel): Blade templates are HTML with `{{ }}`, `{!! !!}` and
//! `@directives`. The Blade grammar parses both (it extends HTML's); the PHP in echoes and
//! directive arguments is injected as PHP (the `php_only` grammar: PHP without `<?php`),
//! the CSS / JS in `<style>` / `<script>` as usual. Directive blocks (`@if … @endif`,
//! `@foreach`, `@section`…) can be collapsed.

mod assist;
mod variables;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub const LANGUAGE: LanguageId = LanguageId::new_static("blade");
/// PHP without the `<?php` tag: what Blade's echoes and directive arguments contain.
pub const PHP_ONLY: LanguageId = LanguageId::new_static("php_only");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.blade"),
    name: "Blade",
    description: "Blade: colors, embedded HTML / CSS / JS and PHP, folding on directives",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-code", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct BladeModule;

impl Module for BladeModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(
            LanguageDescriptor::new("blade", "Blade")
                .word_chars("-")
                .globs(&["*.blade.php"])
                .block_comment("{{--", "--}}")
                .closing_tags(markup::unclosed_tag)
                .icon("file-code"),
        );
        ctx.contribute(grammar());
        ctx.contribute(php_only_grammar());
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            assist::BladeAssist::default(),
        ));
        Ok(())
    }
}

/// HTML's highlights and injections first (the grammar extends HTML's), then Blade's own.
pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_blade::LANGUAGE.into(),
        highlights: format!(
            "{}\n{}",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            include_str!("../../../../../third_party/tree-sitter-blade/queries/highlights.scm")
        )
        .into(),
        injections: format!(
            "{}\n{}",
            languages::shared_queries::HTML_INJECTIONS,
            include_str!("../../../../../third_party/tree-sitter-blade/queries/injections.scm")
        )
        .into(),
        folds: format!(
            "{}\n{}",
            languages::shared_queries::HTML_FOLDS,
            include_str!("../queries/folds.scm")
        )
        .into(),
        // Blade's tree is HTML's: its elements indent as HTML's do.
        indents: languages::shared_queries::HTML_INDENTS.into(),
        string_content_nodes: &["attribute_value"],
        embedded_errors: true,
        injected_errors: false,
        own_errors: true,
    }
}

/// PHP without `<?php`, colored like PHP (its attributes and named arguments too).
pub fn php_only_grammar() -> Grammar {
    Grammar {
        language: PHP_ONLY,
        ts: tree_sitter_php::LANGUAGE_PHP_ONLY.into(),
        highlights: format!(
            "{}\n{}",
            languages::shared_queries::PHP_HIGHLIGHTS,
            tree_sitter_php::HIGHLIGHTS_QUERY
        )
        .into(),
        injections: "".into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &["string_content"],
        embedded_errors: false,
        injected_errors: true,
        own_errors: true,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
