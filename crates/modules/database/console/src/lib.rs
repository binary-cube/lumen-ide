//! Query consoles, as a module (plan 39e): a data source's SQL written in a real `.sql`
//! file of the project's own folder (`consoles/<source>/<name>.sql`, listed in
//! `consoles/consoles.toml` — [`consoles`]), so the editor gives it everything it has
//! (undo, find, multiple cursors, saving, local history), with a bar above it
//! ([`bar::ConsoleBar`], through the generic [`ui_components::editor_toolbar`]):
//! ▶ Run, its options and Explain, the history, the parameters, `Tx: Auto`,
//! ■ Stop, Playground / Script, the settings, the Services window's toggle, and the database
//! and schema it runs on. Its tab is `console [source]`, with the driver's icon and the
//! source's color.
//!
//! Each console runs on a connection of its own ([`runner`]: its own transaction, never the
//! grid's), made through the explorer's [`database::console::ConsoleAccess`] (the same
//! password, startup script and read-only), on the pool `db-console`: the statement under
//! the cursor, the selection or the whole script ([`database::console::plan`]), marked in the
//! gutter while it runs and once done or failed. Nothing connects before the first run; the
//! connection closes with the console's tabs, or once idle (never with a transaction open).
//! A statement's parameters are given values first ([`params`]: written in by the dialect,
//! an error taken back to the user's text); a statement explained shows its profile in a tab
//! of the Services window or of the editor ([`profile::view`]: the tree with the time's heat,
//! the costliest branch, the problems, what can be optimized and the details).
//!
//! "Jump to Query Console…" (⇧⌘F10) opens a source's console from the "Database" window, a
//! data view or the palette ([`commands::jump`]): the source's consoles in a popover filtered
//! by name and text ([`jump`]), renamed or deleted there and in the Services window
//! ([`rename`]). Every statement run is kept in its source's history, in the project
//! ([`history`]), searched, inserted or run again from its window ([`history_view`]).
//! A console completes from its source's schema ([`assist`], on the index [`schema`] reads
//! in the background): the keywords of the place, the tables with their source, the columns
//! after `alias.` and in every clause, whole `JOIN … ON` conditions, an `INSERT`'s columns,
//! `SELECT *` expanded; goes to a table's data, a column's structure or an alias's, a CTE's
//! place in the text, shows their hover cards, and checks the text against the schema
//! ([`checks`]: an unknown table or column, an ambiguous one — only what's certain).
//! A result of one table's rows is edited in its tab as the data view's grid is, its changes
//! submitted through the console's connection and transaction ([`edit`]).
//!
//! A project's own `.sql` file gets the bar too ([`file_bar`]: "No data source — Attach…"):
//! attached to a source (kept in the project's `sql-files.toml`, following its file as a
//! console does), it runs, shows its results in the Services window, completes, resolves
//! and is checked from the source's schema and is laid out by its dialect, as a console;
//! detached, it's a plain SQL file again.

mod answer_slot;
pub mod assist;
pub mod bar;
pub mod checks;
pub mod commands;
pub mod confirm;
pub mod consoles;
pub mod edit;
mod field;
pub mod file_bar;
pub mod history;
pub mod history_view;
pub mod jump;
pub mod layout;
pub mod lent_rows;
pub mod listing;
pub mod params;
pub mod profile;
pub mod provider;
pub mod rename;
pub mod result_panes;
pub mod result_source;
pub mod result_view;
pub mod results;
pub mod results_tab;
pub mod runner;
pub mod runtime;
pub mod schema;
pub mod services;
pub mod settings;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec, When};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::editor_toolbar::EditorToolbarProvider;
use workbench_model::{
    CommandContribution, ProjectService, ResourceReporter, ResourceUse, SettingsContribution,
};

pub use consoles::{Consoles, ConsolesService};

/// The module's id.
pub const MODULE_ID: &str = "database.console";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "Query consoles",
    description: "A data source's SQL in a console file with its own bar and connection: the statement under the cursor, the selection or the script run, stopped, explained",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "console", ..extensions::ModuleInfo::BUILT_IN },
};

/// Runs the statement under the cursor, or the selection (⌘↵ in a console).
pub const RUN: CommandId = CommandId("database.console.run");
/// Runs the whole console as a script (⇧⌘↵).
pub const RUN_SCRIPT: CommandId = CommandId("database.console.run_script");
/// Shows the console's session (its Output) in the Services window.
pub const OPEN_IN_SERVICES: CommandId = CommandId("database.console.open_in_services");
/// Stops what the console runs (⌘F2).
pub const STOP: CommandId = CommandId("database.console.stop");
/// The plan of the statement under the cursor; as it runs.
pub const EXPLAIN: CommandId = CommandId("database.console.explain");
pub const EXPLAIN_ANALYZE: CommandId = CommandId("database.console.explain_analyze");
/// The values of the console's parameters, edited (the bar's `(p)`).
pub const PARAMS: CommandId = CommandId("database.console.params");
/// The window of the statements run on the console's source (the bar's 🕘, ⌥⌘E).
pub const HISTORY: CommandId = CommandId("database.console.history");
/// Commits / rolls back the console's transaction.
pub const COMMIT: CommandId = CommandId("database.console.commit");
pub const ROLLBACK: CommandId = CommandId("database.console.rollback");
/// "Jump to Query Console…" (⇧⌘F10).
pub const JUMP: CommandId = CommandId(database::console::JUMP_COMMAND);
/// "New Query Console".
pub const NEW: CommandId = CommandId(database::console::NEW_COMMAND);
/// "Attach Data Source…" / "Detach Data Source": a SQL file of the project run on a source
/// (39n), or no longer.
pub const ATTACH: CommandId = CommandId("database.console.attach");
pub const DETACH: CommandId = CommandId("database.console.detach");
/// "Expand Column List": a select list's `*` replaced by its columns (⌥↵ in a console).
pub const EXPAND_COLUMNS: CommandId = CommandId("database.console.expand_columns");

#[derive(Debug, Default)]
pub struct DatabaseConsoleModule;

type Spec = (
    CommandId,
    &'static str,
    &'static [&'static str],
    Option<&'static str>,
    Arc<dyn workbench_model::CommandHandler>,
);

fn commands() -> Vec<Spec> {
    use commands::{OnConsole, OnSqlFile, Run};
    let everywhere = When::Everywhere;
    vec![
        (
            RUN,
            "Execute",
            &["CmdOrCtrl+Enter"],
            Some("play"),
            Arc::new(OnSqlFile(|h| commands::run(h, false))),
        ),
        (
            RUN_SCRIPT,
            "Execute Script",
            &["CmdOrCtrl+Shift+Enter"],
            Some("play"),
            Arc::new(OnSqlFile(|h| commands::run(h, true))),
        ),
        (
            STOP,
            "Stop Console Statement",
            &["CmdOrCtrl+F2"],
            Some("stop"),
            Arc::new(OnConsole(commands::stop)),
        ),
        (
            EXPLAIN,
            "Explain Plan",
            &[],
            Some("plan"),
            Arc::new(OnConsole(|h| commands::explain(h, false))),
        ),
        (
            EXPLAIN_ANALYZE,
            "Explain Analyze",
            &[],
            Some("plan"),
            Arc::new(OnConsole(|h| commands::explain(h, true))),
        ),
        (
            PARAMS,
            "Edit Parameters…",
            &[],
            Some("parameter"),
            Arc::new(OnConsole(commands::edit_params)),
        ),
        (
            HISTORY,
            "Query History…",
            &["CmdOrCtrl+Alt+E"],
            Some("clock"),
            Arc::new(OnConsole(commands::history)),
        ),
        (
            COMMIT,
            "Commit Console Transaction",
            &[],
            Some("check"),
            Arc::new(OnConsole(|h| commands::finish(h, true))),
        ),
        (
            ROLLBACK,
            "Roll Back Console Transaction",
            &[],
            Some("undo"),
            Arc::new(OnConsole(|h| commands::finish(h, false))),
        ),
        (
            JUMP,
            "Jump to Query Console…",
            &["CmdOrCtrl+Shift+F10"],
            Some("console"),
            Arc::new(Run(|h| commands::jump(h, false), everywhere)),
        ),
        (
            NEW,
            "New Query Console",
            &[],
            Some("console"),
            Arc::new(Run(|h| commands::jump(h, true), everywhere)),
        ),
        (ATTACH, "Attach Data Source…", &[], Some("link"), Arc::new(commands::AttachCommand)),
        (DETACH, "Detach Data Source", &[], Some("x"), Arc::new(commands::DetachCommand)),
        (
            EXPAND_COLUMNS,
            "Expand Column List",
            &["Alt+Enter"],
            None,
            Arc::new(OnConsole(crate::assist::expand_star)),
        ),
        (
            assist::GOTO,
            "Go to Table or Column",
            &[],
            None,
            Arc::new(OnConsole(crate::assist::goto)),
        ),
        (
            OPEN_IN_SERVICES,
            "Open in Services",
            &[],
            Some("services"),
            Arc::new(OnConsole(commands::open_in_services)),
        ),
    ]
}

impl Module for DatabaseConsoleModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: the consoles' statements, and the watch closing the idle ones.
        ctx.contribute(workbench_model::ModuleThreads(&[
            "db-console",
            "db-console-idle",
            "db-schema-index",
        ]));
        // Its completion from the source's schema (the console's documents).
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            assist::ConsoleAssist::default(),
        ));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(ConsolesService));
        ctx.contribute::<Arc<dyn ResourceReporter>>(Arc::new(ConsolesResources));
        // A console laid out ("Reformat Code") by its source's dialect.
        database::console::tell_flavors(Arc::new(assist::flavor_of));
        ctx.contribute::<Arc<dyn EditorToolbarProvider>>(Arc::new(provider::ConsoleBarProvider));
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(settings::ConsoleSettings));
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(settings::ProfileSettings));
        // Its confirmation and its parameters (modals of its own).
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            confirm::ConfirmProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            results_tab::ResultsProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(params::ParamsProvider));
        // Its history, its consoles' popover and their renaming (views of its own).
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            history_view::HistoryProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(jump::JumpProvider));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(rename::RenameProvider));
        // A profile in an editor tab of its own (`view-profile`), and its actions.
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            profile::ProfileViewProvider,
        ));
        for command in profile::commands() {
            ctx.contribute(command);
        }
        // Its sessions and their results in the Services window.
        ctx.contribute::<Arc<dyn ui_components::ServiceProvider>>(Arc::new(
            services::ConsoleServicesProvider,
        ));
        for (id, label, shortcuts, icon, handler) in commands() {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon,
                handler,
            });
        }
        for item in menu_items() {
            ctx.contribute(item);
        }
        // A result's editing keys, where its view has them (its tab in the Services window).
        for (id, label, shortcuts, action) in edit::ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(workbench_model::ViewAction { view: edit::VIEW, action }),
            });
        }
        // Their keys in the presets' keymaps (jetbrains: ⌘↵, ⇧⌘↵, ⌘F2, ⇧⌘F10, ⌘⌥E, ⌥↵;
        // vscode: Ctrl+Enter).
        for (preset, command, shortcuts) in [
            ("jetbrains", RUN, &["CmdOrCtrl+Enter"][..]),
            ("jetbrains", RUN_SCRIPT, &["CmdOrCtrl+Shift+Enter"][..]),
            ("jetbrains", STOP, &["CmdOrCtrl+F2"][..]),
            ("jetbrains", JUMP, &["CmdOrCtrl+Shift+F10"][..]),
            ("jetbrains", HISTORY, &["CmdOrCtrl+Alt+E"][..]),
            ("jetbrains", EXPAND_COLUMNS, &["Alt+Enter"][..]),
            ("vscode", RUN, &["CmdOrCtrl+Enter"][..]),
        ] {
            ctx.contribute(command_model::bindings::PresetShortcuts { preset, command, shortcuts });
        }
        Ok(())
    }
}

/// A console's own rows in the editor's right-click menu, first: Execute,
/// Execute Script, Explain Plan, Attach / Detach Data Source (a SQL file of the project),
/// then Jump to Query Console and Open in Services among the navigation's. Each only where
/// its command is enabled (Execute also on a SQL file attached to nothing: it asks for a
/// source first); Go to Declaration and Jump, while Explain is (a console, an attached file).
pub fn menu_items() -> Vec<workbench_model::ContextMenuItem> {
    use workbench_model::{ContextMenuItem as Item, MenuPlace, MenuSection, MenuWhen};
    const EDITOR: &[MenuPlace] = &[MenuPlace::Editor];
    vec![
        Item::new(RUN, EDITOR, MenuSection::Run, 10),
        Item::new(RUN_SCRIPT, EDITOR, MenuSection::Run, 20),
        Item::new(EXPLAIN, EDITOR, MenuSection::Run, 30),
        Item::new(ATTACH, EDITOR, MenuSection::Run, 40),
        Item::new(DETACH, EDITOR, MenuSection::Run, 50),
        // Its names resolve through its source's schema (a table, a column, an alias).
        Item::new(workbench_model::GOTO_DECLARATION, EDITOR, MenuSection::Navigate, 10)
            .when(MenuWhen::With(EXPLAIN)),
        Item::new(JUMP, EDITOR, MenuSection::Navigate, 40).when(MenuWhen::With(EXPLAIN)),
        Item::new(OPEN_IN_SERVICES, EDITOR, MenuSection::Navigate, 50),
    ]
}

/// The open projects' consoles, for the Task Manager: one part per project with console
/// connections or Output kept (none otherwise).
#[derive(Debug, Default)]
pub struct ConsolesResources;

impl ResourceReporter for ConsolesResources {
    fn report(&self) -> Vec<ResourceUse> {
        consoles::all()
            .iter()
            .filter(|c| !c.runners().is_empty())
            .map(|c| {
                let runners = c.runners();
                let mut part = ResourceUse::new(i18n::t("tasks.consoles"), MODULE_ID);
                part.detail = i18n::tr!(
                    "tasks.consoles_detail",
                    open = c.connected(),
                    consoles = runners.len()
                );
                part.memory = Some(runners.iter().map(|r| r.memory() as u64).sum());
                part.threads = vec!["db-console", "db-console-idle"];
                part.group = workbench_model::resources::SERVICES;
                part.project = Some(c.project());
                // Its queries, running and the last ended (each with its Stop).
                runner::QUERIES.report(&mut part);
                part
            })
            .collect()
    }

    /// Its queries' "Stop" (the console's own).
    fn act(
        &self,
        _part: &workbench_model::PartRef,
        _action: &str,
        target: workbench_model::ActionTarget,
    ) -> workbench_model::ActOutcome {
        runner::QUERIES.act(target)
    }
}

#[cfg(test)]
#[path = "../tests/unit/fake.rs"]
mod fake;

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
