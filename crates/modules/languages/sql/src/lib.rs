//! SQL module: the language (`.sql`, `.ddl`, `.pgsql`, `.psql`), its grammar (DerekStride's
//! permissive tree-sitter-sql, vendored), highlighting, folding, indentation, and the words
//! of each dialect (keywords; SQLite's built-in functions from `data/sqlite.functions`),
//! and "Reformat Code" (`format`: the domain's formatter by the document's dialect, with
//! the options of `settings`). It runs nothing against a database: running SQL and the
//! schema's names belong to the database modules (a console's dialect is told by theirs,
//! through the domain). Heredocs in PHP (`<<<SQL`) light up through PHP's own injection.

mod dialect;
pub mod format;
mod functions;
mod intelligence;
pub mod settings;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use languages::{LanguageDescriptor, LanguageId};
use syntax_ts::Grammar;

pub use dialect::Dialect;
pub use intelligence::SqlIntelligence;

pub const LANGUAGE: LanguageId = LanguageId::new_static("sql");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.sql"),
    name: "SQL",
    description: "SQL: colors, folding, keywords and functions per dialect, formatting",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct SqlModule;

impl Module for SqlModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn release_idle(&self, idle: std::time::Duration) -> bool {
        functions::WORDS.release_if_idle(idle)
    }

    fn loaded(&self) -> bool {
        functions::WORDS.is_loaded()
    }

    fn memory(&self) -> Option<u64> {
        functions::WORDS.peek(functions::Words::weight)
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(descriptor());
        ctx.contribute(grammar());
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            SqlIntelligence,
        ));
        ctx.contribute(comment_item());
        // "Reformat Code": the file through Code quality's command, the statement here.
        ctx.contribute::<Arc<dyn code_quality::TextFormatter>>(Arc::new(format::SqlFormatter));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::FormatSettings,
        ));
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: format::FORMAT_STATEMENT,
                label: "Reformat Statement",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::CODE_FORMAT,
                    20,
                )),
                shortcuts: &[],
            },
            icon: Some("braces"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::Language("sql")],
                Arc::new(format::FormatStatement),
            ),
        });
        for item in format_items() {
            ctx.contribute(item);
        }
        Ok(())
    }
}

/// "Reformat Code" and "Reformat Statement" in a SQL file's right-click menu (a console's
/// too), after its line comment.
pub fn format_items() -> Vec<workbench_model::ContextMenuItem> {
    use workbench_model::{ContextMenuItem, MenuPlace, MenuSection, MenuWhen};
    [(workbench_model::REFORMAT_FILE, 20), (format::FORMAT_STATEMENT, 30)]
        .into_iter()
        .map(|(command, order)| {
            ContextMenuItem::new(command, &[MenuPlace::Editor], MenuSection::Code, order)
                .when(MenuWhen::Language("sql"))
        })
        .collect()
}

/// "Comment with Line Comment" (`--`) in a SQL file's right-click menu.
pub fn comment_item() -> workbench_model::ContextMenuItem {
    use workbench_model::{ContextMenuItem, MenuPlace, MenuSection, MenuWhen};
    ContextMenuItem::new(workbench_model::COMMENT_LINE, &[MenuPlace::Editor], MenuSection::Code, 10)
        .when(MenuWhen::Language("sql"))
}

pub fn descriptor() -> LanguageDescriptor {
    LanguageDescriptor::new("sql", "SQL")
        // PostgreSQL's names may hold `$` (`a$b`); `$1` is a parameter.
        .word_chars("$")
        // `x.sqlite.sql` is a `.sql` file (its dialect is told by `Dialect::of_path`).
        .extensions(&["sql", "ddl", "pgsql", "psql"])
        .quotes(&['\'', '"', '`'])
        .line_comment("--")
        .block_comment("/*", "*/")
        .icon("database")
}

pub fn grammar() -> Grammar {
    Grammar {
        language: LANGUAGE,
        ts: tree_sitter_sql::LANGUAGE.into(),
        highlights: include_str!("../queries/highlights.scm").into(),
        // A function's `$$` body is read as SQL by the grammar itself (the same tree): no
        // injection needed — and the engine never injects a language into itself.
        injections: "".into(),
        folds: include_str!("../queries/folds.scm").into(),
        indents: include_str!("../queries/indents.scm").into(),
        string_content_nodes: &[],
        // A permissive grammar for many dialects: its errors would be false alarms on valid
        // SQL (the server's errors, after running, are the real ones).
        embedded_errors: false,
        // Nothing is injected into SQL (see above).
        injected_errors: false,
        own_errors: false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
