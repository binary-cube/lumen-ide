//! The project panel, as a module: the project's tree (folders read when opened, the
//! modules' marks, a new entry typed in place, dragging) and the search for files in it,
//! with the tree's operations (new, rename, cut / copy / paste, delete, copy path, reveal) as its
//! commands on the action's targets. Its views are built on `ui-components`, like any
//! module's.

mod away;
pub mod file_search;
pub mod operations;
pub mod panel;
mod path_search;
pub mod tree_view;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolPanel, ToolWindowProvider};
use workbench_model::{CommandContribution, ContextMenuItem, MenuPlace, MenuSection};

pub use file_search::{FileSearch, FileSearchStyle};
pub use operations::Operations;
pub use panel::ProjectPanel;
pub use tree_view::{PendingEntry, ProjectTreeStyle, ProjectTreeView, TreeAction};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("project-tree"),
    name: "Project",
    description: "The project panel: the tree of the project's files, their search and operations",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "folder", ..extensions::ModuleInfo::BUILT_IN },
};

/// Its window, on the left.
pub const TOOL: &str = workbench_model::PROJECT_PANEL;
/// Shows / hides it (⌘1): the app's command.
pub const TOGGLE: CommandId = CommandId(panel::TOGGLE);

#[derive(Debug, Default)]
pub struct ProjectTreeModule;

/// The project window: its panel made when first shown.
#[derive(Debug)]
struct ProjectTool;

impl ToolWindowProvider for ProjectTool {
    fn id(&self) -> &'static str {
        TOOL
    }
    fn icon(&self) -> &'static str {
        "folder-tool"
    }
    fn title(&self) -> String {
        i18n::t("tooltip.project")
    }
    fn dock(&self) -> Dock {
        Dock::Left
    }
    fn toggle_command(&self) -> Option<&'static str> {
        Some(TOGGLE.0)
    }
    fn content(&self) -> ToolContent {
        ToolContent::Panel
    }
    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        Some(Box::new(ProjectPanel::new()))
    }
}

/// The tree's right-click menu rows (on a file or a folder).
const MENU: &[ContextMenuItem] = {
    use operations::*;
    const ENTRY: &[MenuPlace] = &[MenuPlace::TreeFile, MenuPlace::TreeFolder];
    use ContextMenuItem as Item;
    use MenuSection::{Clipboard, Danger, New, Path};
    &[
        Item::new(NEW_FILE, ENTRY, New, 10),
        Item::new(NEW_FOLDER, ENTRY, New, 20),
        // The editor's clipboard keys act on the tree's entries when it has the keyboard.
        Item::new(CUT, ENTRY, Clipboard, 10).shortcut("CmdOrCtrl+X"),
        Item::new(COPY, ENTRY, Clipboard, 20).shortcut("CmdOrCtrl+C"),
        Item::new(PASTE, ENTRY, Clipboard, 30).shortcut("CmdOrCtrl+V"),
        // Beside Move… (the refactoring module's, 40): where an entry is and what it's called.
        Item::new(RENAME, ENTRY, Clipboard, 35),
        Item::new(COPY_PATH, ENTRY, Path, 10),
        Item::new(REVEAL, ENTRY, Path, 30).label(REVEAL_LABEL),
        // As in the system's file manager: ⌘⌫ (Finder), Delete (Explorer).
        Item::new(DELETE, ENTRY, Danger, 10).shortcut(DELETE_KEY),
    ]
};

/// "Reveal" named after the system's file manager.
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "command.reveal_finder"
} else if cfg!(target_os = "windows") {
    "command.reveal_explorer"
} else {
    "command.reveal_folder"
};

/// The key that deletes the tree's selection (shown in its menu).
const DELETE_KEY: &str = if cfg!(target_os = "macos") { "CmdOrCtrl+Backspace" } else { "Delete" };

impl Module for ProjectTreeModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        use operations::*;
        // Shown / hidden by the app's own command (⌘1: without a project, one is opened).
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ProjectTool));
        ctx.contribute(workbench_model::ModuleThreads(&[path_search::THREAD]));
        let shared = Arc::new(Operations::default());
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(
            operations::Reporter,
        ));
        ctx.contribute::<Arc<dyn workbench_model::ProjectService>>(Arc::new(Service(Arc::clone(
            &shared,
        ))));
        let commands: [(CommandId, &'static str, &'static str); 9] = [
            (NEW_FILE, "New File", "file"),
            (NEW_FOLDER, "New Folder", "folder"),
            (CUT, "Cut", "scissors"),
            (COPY, "Copy", "copy"),
            (PASTE, "Paste", "clipboard"),
            (DELETE, "Delete…", "trash"),
            (COPY_PATH, "Copy Path", "copy"),
            (REVEAL, "Reveal in File Manager", "folder-open"),
            (CANCEL_PASTE, "Stop Pasting", "copy"),
        ];
        for (id, label, icon) in commands {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon: Some(icon),
                handler: Arc::new(Command { id, operations: Arc::clone(&shared) }),
            });
        }
        // Rename: ⇧F6 while the tree has the keys (the editor's ⇧F6 renames the
        // symbol), F2 in the presets whose file rename is F2.
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RENAME,
                label: "Rename…",
                menu: None,
                shortcuts: &["Shift+F6"],
            },
            icon: Some("file-pen"),
            handler: Arc::new(Command { id: RENAME, operations: Arc::clone(&shared) }),
        });
        for preset in ["vscode", "zed"] {
            ctx.contribute(command_model::bindings::PresetShortcuts {
                preset,
                command: RENAME,
                shortcuts: &["F2"],
            });
        }
        for item in MENU {
            ctx.contribute(*item);
        }
        Ok(())
    }
}
