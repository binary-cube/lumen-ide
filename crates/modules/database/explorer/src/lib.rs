//! Data sources at work, as a module, and their data view: a file a driver opens by
//! itself (a SQLite database, a CSV file) shown as data — its tables, a grid of a
//! table's rows ([`view`], through [`grid_source`]), its structure, changed in the
//! structure's modals ([`schema`]); the data sources configured ([`catalog`]: the
//! project's and every project's) in the "Database" tool window ([`window`], over the
//! [`tree`] model), whose tables open in the same data view; their connections set in the
//! "Data Sources and Drivers" modal ([`sources`]), their passwords kept by
//! [`credentials`]. Each open source has a [`SessionWorker`]: its session,
//! its requests run one at a time on the shared pool `db`, the latest page of a table
//! winning, any request cancellable ([`worker`]). A project's workers are its
//! [`Sessions`], gone with it; an unused session is closed after a while
//! ([`settings::IDLE_CLOSE`]). A table is read and changed through a
//! [`SessionRepository`] (pages cached within a memory limit, the next one read ahead,
//! changes applied in one transaction — [`execute`]), whatever the driver.
//!
//! Lazy: until a source opens, a project holds an empty table, no thread, no connection.

pub mod cache;
pub mod catalog;
pub mod console;
pub mod credentials;
pub mod diagram;
pub mod edits;
pub mod engine_icon;
pub mod execute;
pub mod export;
pub mod grid_source;
pub mod identity;
pub mod layout;
pub mod pane_rows;
pub mod preview;
pub mod provider;
pub mod repository;
pub mod runtime;
pub mod schema;
pub mod sessions;
pub mod settings;
pub mod sources;
pub mod tree;
pub mod view;
pub mod window;
pub mod worker;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolPanel, ToolWindowProvider, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, MenuContribution, MenuPlace, MenuRow, ProjectService,
    ResourceReporter, ResourceUse, SettingsContribution, ViewAction, WorkbenchHost,
};

pub use provider::DataViewProvider;
pub use repository::SessionRepository;
pub use runtime::Runtime;
pub use sessions::{Sessions, SessionsService, sessions};
pub use worker::{Client, Reply, Request, Response, SessionWorker, Source, Ticket};

/// The module's id.
pub const MODULE_ID: &str = "database.explorer";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "Database explorer",
    description: "Data sources at work: their sessions in the background, tables read a page at a time and changed in transactions; SQLite and CSV files opened as data",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "database", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct DatabaseExplorerModule;

impl Module for DatabaseExplorerModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: the sessions' requests, and the watch closing the idle ones.
        ctx.contribute(workbench_model::ModuleThreads(&["db", "db-idle"]));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(SessionsService));
        ctx.contribute::<Arc<dyn ResourceReporter>>(Arc::new(SessionsResources));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(DataViewProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(preview::PreviewProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(schema::SchemaProvider));
        // The data sources' modal.
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(sources::SourcesProvider));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: sources::DATA_SOURCES,
                label: "Data Sources and Drivers…",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::TOOLS_DATA,
                    10,
                )),
                shortcuts: &[],
            },
            icon: Some("database"),
            handler: Arc::new(sources::SourcesCommand),
        });
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(settings::DataSettings));
        // The "Database" window: its button on the right, its panel made when first shown.
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(DatabaseTool));
        for (id, label, icon, shortcuts, handler) in window_commands() {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon,
                handler,
            });
        }
        // Its button's right-click menu (the same id).
        ctx.contribute(MenuContribution {
            id: window::TOOL,
            label_key: "datasources.title",
            icon: "database",
            places: &[MenuPlace::ToolStrip],
            rows: vec![
                MenuRow::Command(REFRESH_ALL, &[MenuPlace::ToolStrip]),
                MenuRow::Command(COLLAPSE, &[MenuPlace::ToolStrip]),
                MenuRow::Command(sources::DATA_SOURCES, &[MenuPlace::ToolStrip]),
            ],
        });
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: view::VIEW, action }),
            });
        }
        Ok(())
    }
}

/// The "Database" window, on the right.
#[derive(Debug)]
pub struct DatabaseTool;

impl ToolWindowProvider for DatabaseTool {
    fn id(&self) -> &'static str {
        window::TOOL
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        i18n::t("datasources.title")
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    /// After Structure, the Task Manager and the canvas.
    fn order(&self) -> u16 {
        40
    }

    /// Its group on the strip: the services the project talks to.
    fn group(&self) -> &'static str {
        "services"
    }

    fn toggle_command(&self) -> Option<&'static str> {
        Some(window::TOGGLE.0)
    }

    fn content(&self) -> ToolContent {
        ToolContent::Panel
    }

    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        Some(Box::new(window::DatabaseWindow::new()))
    }
}

/// Reads every source of the window again (its button's menu).
pub const REFRESH_ALL: CommandId = CommandId("database.refresh_all_sources");
/// Closes every node of the window (its button's menu).
pub const COLLAPSE: CommandId = CommandId("database.collapse_sources");

/// What a command asks the window ([`WorkbenchHost::tell_panel`]): one of its menu rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCommand(pub &'static str);

/// Shows / hides the window.
#[derive(Debug)]
struct Toggle;

impl CommandHandler for Toggle {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.toggle_tool(window::TOOL)
    }
}

/// Tells the window to run one of its rows (shown first).
#[derive(Debug)]
struct Tell(&'static str);

impl CommandHandler for Tell {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.tell_panel(window::TOOL, &WindowCommand(self.0), false)
    }
}

type WindowCommandSpec = (
    CommandId,
    &'static str,
    Option<&'static str>,
    &'static [&'static str],
    Arc<dyn CommandHandler>,
);

/// The window's commands: shown / hidden, read again (F5 while it has the keys), its
/// button's menu's rows, and the source's view it opens.
fn window_commands() -> Vec<WindowCommandSpec> {
    vec![
        (window::TOGGLE, "Database", Some("database"), &[], Arc::new(Toggle)),
        (
            window::REFRESH,
            "Refresh Data Sources",
            Some("refresh"),
            &["F5"],
            Arc::new(ViewAction { view: window::TOOL, action: "refresh" }),
        ),
        (REFRESH_ALL, "Refresh Data Sources", Some("refresh"), &[], Arc::new(Tell("refresh-all"))),
        (COLLAPSE, "Collapse All", Some("chevrons-collapse"), &[], Arc::new(Tell("collapse"))),
    ]
}

/// The data view's actions (remappable): the command, its label, its keys, what the view
/// is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 9] = [
    (CommandId("database.refresh"), "Refresh Data", &["F5"], "refresh"),
    (CommandId("database.transpose"), "Transpose Rows", &[], "transpose"),
    (CommandId("database.submit"), "Submit Data Changes", &["CmdOrCtrl+Enter"], "submit"),
    (CommandId("database.add_row"), "Add Row", &[], "grid:add-row"),
    (CommandId("database.clone_row"), "Clone Row", &[], "grid:clone-row"),
    (
        CommandId("database.delete_rows"),
        "Delete Rows",
        &["CmdOrCtrl+Backspace"],
        "grid:delete-rows",
    ),
    (CommandId("database.revert"), "Revert Data Changes", &[], "grid:revert-all"),
    (CommandId("database.export"), "Export Data…", &[], "grid:export:csv"),
    (CommandId("database.diagram"), "Relations Diagram (Data)", &[], "diagram"),
];

/// The open projects' sessions, for the Task Manager: one part per project with sources
/// opened (none otherwise).
#[derive(Debug, Default)]
pub struct SessionsResources;

impl ResourceReporter for SessionsResources {
    fn report(&self) -> Vec<ResourceUse> {
        sessions::all()
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut part = ResourceUse::new(i18n::t("tasks.database"), MODULE_ID);
                part.detail =
                    i18n::tr!("tasks.database_detail", open = s.connected(), sources = s.len());
                part.threads = vec!["db", "db-idle"];
                part.group = workbench_model::resources::SERVICES;
                part.project = Some(s.project());
                s.activities().report(&mut part);
                // Its idle sessions closed (opened again when next used): nothing is lost,
                // so not asked first.
                if s.connected() > 0 && !s.busy() {
                    part.actions.push(workbench_model::PartAction::new(
                        CLOSE_IDLE,
                        workbench_model::ActionKind::Disconnect,
                        workbench_model::ActionTarget::Part,
                    ));
                }
                part
            })
            .collect()
    }

    /// Its exports stopped, its idle sessions closed (on a thread of their own: closing
    /// may wait for the server).
    fn act(
        &self,
        part: &workbench_model::PartRef,
        action: &str,
        target: workbench_model::ActionTarget,
    ) -> workbench_model::ActOutcome {
        use workbench_model::{ActOutcome, ActionTarget};
        let Some(sessions) = part.project.and_then(sessions::sessions) else {
            return ActOutcome::Unsupported;
        };
        match (action, target) {
            (CLOSE_IDLE, ActionTarget::Part) => {
                kernel::work::run("db-idle", move || {
                    sessions.sweep(std::time::Instant::now(), std::time::Duration::ZERO);
                });
                ActOutcome::Done
            }
            (_, ActionTarget::Activity(_)) => sessions.activities().act(target),
            _ => ActOutcome::Unsupported,
        }
    }
}

/// The Task Manager's action closing a project's idle database sessions.
const CLOSE_IDLE: &str = "close-idle";

#[cfg(test)]
#[path = "../tests/unit/fake.rs"]
mod fake;

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
