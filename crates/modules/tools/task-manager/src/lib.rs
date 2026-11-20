//! The Task Manager as a module: a tab of the editor (one per window — the command, its
//! button on the right strip and the status bar's CPU / memory / GPU bring it to the front
//! in whichever group it is, or open it) showing what each part of the app — its own and
//! the modules', through their `ResourceReporter`s — costs in CPU, memory, GPU and
//! network, gathered by one collector per window ([`collector::Hub`]) while a view of it
//! is shown.

pub mod collector;
pub mod confirm;
#[cfg(any(test, feature = "snapshot"))]
pub mod demo;
pub mod layout;
pub mod model;
pub mod settings;
mod view;
pub mod widget;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{
    Dock, ToolContent, ToolWindow, ToolWindowProvider, Viewer, ViewerKind, ViewerProvider,
};
use workbench_model::{CommandContribution, CommandHandler, ViewAction, WorkbenchHost};

pub use collector::{Frame, Hub, HubInfo, Line, Rates, Sampler, Tick};
pub use model::Column;
pub use view::{Popover, Preferences, TaskManager, preferences, set_preferences};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tasks.manager"),
    name: "Task Manager",
    description: "What each part of the app costs (CPU, memory, GPU), sortable and filterable, with the last minute in small graphs",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "cpu", ..extensions::ModuleInfo::BUILT_IN },
};

/// Opens the Task Manager (or brings its tab to the front).
pub const OPEN: CommandId = workbench_model::SHOW_RESOURCES;
/// Its button on the right strip.
pub const TOOL: &str = "tasks";
/// The extension of its tab's virtual path.
pub const EXTENSION: &str = "view-task-manager";
/// Its viewer (where its actions' keys work).
const VIEW: &str = "task-manager";
/// What a view chose that the settings keep (its columns, its detail's share, how often it
/// looks and how far back), saved: run by the views.
pub const SAVE: CommandId = CommandId("tasks.save");

/// Its view's actions (remappable): the command, its label, its keys, what the view is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 14] = [
    (CommandId("tasks.pause"), "Pause / Resume (Task Manager)", &[], "pause"),
    (CommandId("tasks.find"), "Filter (Task Manager)", &["CmdOrCtrl+F"], "find"),
    (CommandId("tasks.columns"), "Choose Columns… (Task Manager)", &[], "columns"),
    (CommandId("tasks.sort_name"), "Sort by Name (Task Manager)", &[], "sort:name"),
    (CommandId("tasks.sort_cpu"), "Sort by CPU (Task Manager)", &[], "sort:cpu"),
    (CommandId("tasks.sort_memory"), "Sort by Memory (Task Manager)", &[], "sort:memory"),
    (CommandId("tasks.sort_network"), "Sort by Network (Task Manager)", &[], "sort:network"),
    (CommandId("tasks.sort_disk"), "Sort by Disk (Task Manager)", &[], "sort:disk"),
    (CommandId("tasks.sort_gpu"), "Sort by GPU (Task Manager)", &[], "sort:gpu"),
    (CommandId("tasks.sort_threads"), "Sort by Threads (Task Manager)", &[], "sort:threads"),
    (CommandId("tasks.detail"), "Show in Detail (Task Manager)", &[], "detail"),
    (CommandId("tasks.copy_row"), "Copy Row (Task Manager)", &[], "copy-row"),
    (CommandId("tasks.copy_pid"), "Copy PID (Task Manager)", &[], "copy-pid"),
    // The keys' row's program, operation or part stopped (asked first when work is lost).
    (CommandId("tasks.stop"), "Stop (Task Manager)", &[], "stop"),
];

#[derive(Debug, Default)]
pub struct TaskManagerModule;

impl Module for TaskManagerModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        // The question before an action that loses work: a modal of its own.
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(confirm::ConfirmProvider));
        // Its tab and breadcrumb show it.
        ctx.contribute(languages::FileIconRule::new(
            languages::FileMatcher::Extension(EXTENSION),
            "cpu",
            "config",
        ));
        // Its button at the right strip's bottom (the window's bottom right corner): the tab
        // opens (lit while it's the active one).
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: TOOL,
            icon: "cpu",
            title_key: "tasks.title",
            dock: Dock::Right,
            order: 20,
            strip_end: true,
            group: "",
            toggle: Some(OPEN.0),
            content: ToolContent::Command { command: OPEN.0, tab: Some(EXTENSION) },
        }));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(Itself));
        // The app's resources on calm screens (Relax mode's scene): its cards on glass.
        ctx.contribute::<Arc<dyn ui_components::screen_widgets::ScreenWidget>>(Arc::new(
            widget::ResourcesWidget,
        ));
        // How often it looks and how far its graphs reach back.
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::TaskSettings,
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: OPEN,
                label: "Task Manager",
                // In View › Tool Windows: its tool window's row.
                menu: None,
                shortcuts: &[],
            },
            icon: Some("cpu"),
            handler: Arc::new(Open),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SAVE,
                label: "Save Task Manager Choices",
                menu: None,
                shortcuts: &[],
            },
            icon: None,
            handler: Arc::new(Save),
        });
        // Its actions: in the palette, remappable, their keys only in its tab.
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: VIEW, action }),
            });
        }
        Ok(())
    }
}

/// The Task Manager counts itself too (its collector's thread).
#[derive(Debug)]
struct Itself;

impl workbench_model::ResourceReporter for Itself {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        let mut part =
            workbench_model::ResourceUse::new(i18n::t("tasks.title"), MANIFEST.id.as_str());
        part.threads = vec!["task-manager"];
        vec![part]
    }
}

/// Its tab's view.
#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(TaskManager::default()))
    }
}

/// [`SAVE`]: the views' preferences written to the settings.
struct Save;

impl CommandHandler for Save {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(user) = host.user_settings() else { return false };
        settings::save_preferences(user, &view::preferences());
        true
    }
}

/// Brings the window's Task Manager to the front, in whichever group it is; else opens it
/// with the window's resources.
struct Open;

impl CommandHandler for Open {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.resources().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if host.focus_view(EXTENSION) {
            return true;
        }
        let Some(source) = host.resources() else { return false };
        host.open_view(&i18n::t("tasks.title"), EXTENSION, &workbench_model::ResourceData(source))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
