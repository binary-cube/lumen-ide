//! Local history module: keeps a version of every project file on each save — and the
//! version on disk just before it (the first state, edits made outside) — in the project's
//! data folder (outside the project), for 30 days. "Local History › Show History" (the
//! editor's and the tree's right-click menus) lists a file's versions in its window at the
//! bottom, a tab for each file (as Git's and Remote's): one compared with the current text
//! in the editor above, or brought back (unsaved, one undo step). Its fixed tab, "Current
//! file", follows the active file while it shows ([`follow`]). Works without git.
//!
//! Loaded in pieces: the service only queues saves to its thread (which ends with the
//! project); the list is read when its tab opens and let go when it closes.

mod follow;
mod service;
pub mod settings;
mod view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use ui_components::{
    Dock, ToolContent, ToolWindow, ToolWindowProvider, Viewer, ViewerKind, ViewerProvider,
};
use workbench_model::{
    CommandContribution, CommandHandler, MenuContribution, MenuPlace, MenuRow, ProjectService,
    ViewAction, WorkbenchHost,
};

pub use service::HistoryService;
pub use view::{EXTENSION, FollowData, HistoryData, HistoryView};

/// The module's id (the settings show its options while it works).
pub const MODULE_ID: &str = "history.local";

/// What its views read (a version compared, the file's hash): the Task Manager's disk
/// column, on the module's row.
pub(crate) static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "Local History",
    description: "A version of every file kept on each save, compared or brought back — no git needed",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "clock", ..extensions::ModuleInfo::BUILT_IN },
};

pub const SHOW_HISTORY: CommandId = CommandId("history.show");
/// The window's fixed tab, "Current file" (its button: always there).
pub const CURRENT: CommandId = CommandId("history.current");
/// The fixed tab's key in the window (the same in every language).
const CURRENT_KEY: &str = "current-file";

/// Its window at the bottom: the files' histories as its tabs.
pub const TOOL: &str = "local-history";

/// The versions' folder inside the project's data folder.
pub const FOLDER: &str = "local-history";

#[derive(Debug, Default)]
pub struct LocalHistoryModule;

impl Module for LocalHistoryModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["history-usage"]));
        ctx.contribute(workbench_model::ModuleCounters { disk: Some(&DISK), traffic: None });
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(HistoryService::default()));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(service::Reporter));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::HistorySettings,
        ));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        // Its window at the bottom, with Git's and Remote's: its fixed tab follows the active
        // file; a file's history asked from a menu, a tab of its own. Its button is always
        // there (with no file: the tab says so).
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: TOOL,
            icon: "clock",
            title_key: "tooltip.local_history",
            dock: Dock::Bottom,
            order: 15,
            strip_end: false,
            group: "",
            toggle: None,
            content: ToolContent::Views { command: CURRENT.0, new: None },
        }));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: CURRENT,
                label: "Local History (Current File)",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("clock"),
            handler: Arc::new(ShowCurrent),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SHOW_HISTORY,
                label: "Show Local History",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::FILE_HISTORY,
                    10,
                )),
                shortcuts: &[],
            },
            icon: Some("clock"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::FileOnDisk],
                Arc::new(ShowHistory),
            ),
        });
        ctx.contribute(MenuContribution {
            id: "local-history",
            label_key: "history.menu",
            icon: "clock",
            places: &[MenuPlace::Editor, MenuPlace::TreeFile],
            rows: vec![MenuRow::Command(SHOW_HISTORY, &[MenuPlace::Editor, MenuPlace::TreeFile])],
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RELOAD,
                label: "Reload (Local History)",
                menu: None,
                shortcuts: &["r"],
            },
            icon: None,
            handler: Arc::new(ViewAction { view: MODULE_ID, action: "reload" }),
        });
        Ok(())
    }
}

/// Reads the history's versions again (remappable; its key in the view, `r`).
pub const RELOAD: CommandId = CommandId("history.reload");

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(HistoryView::default()))
    }
}

/// The window's fixed tab: the active file's versions, followed (always offered).
struct ShowCurrent;

impl CommandHandler for ShowCurrent {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let store = host
            .project_storage()
            .map(|storage| Arc::new(history_fs::DiskHistory::new(storage.join(FOLDER))));
        let data = FollowData {
            project: host.project_id(),
            root: host.project_root().unwrap_or_default(),
            store,
        };
        host.show_in_panel_keyed(TOOL, CURRENT_KEY, &t("history.current_tab"), EXTENSION, &data)
    }
}

/// Lists the active file's (or the menu's file's) versions.
struct ShowHistory;

impl CommandHandler for ShowHistory {
    /// Only for the project's files (one opened from elsewhere has no history here).
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        let (Some(file), Some(root)) = (host.active_file(), host.project_root()) else {
            return false;
        };
        version_control::relative_path(&root, &file.path).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(file), Some(root), Some(storage)) =
            (host.active_file(), host.project_root(), host.project_storage())
        else {
            return false;
        };
        let Some(path) = version_control::relative_path(&root, &file.path) else {
            host.notify(workbench_model::Notification::info(t("history.outside_project")));
            return false;
        };
        let store = Arc::new(history_fs::DiskHistory::new(storage.join(FOLDER)));
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let data = HistoryData { store, path, file: file.path };
        host.show_in_panel(TOOL, &tr!("history.tab", name = name.as_str()), EXTENSION, &data)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
