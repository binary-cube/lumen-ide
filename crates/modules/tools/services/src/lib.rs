//! "Services" module: the window at the bottom showing what the
//! modules have running — a data source › its consoles' sessions › their results; later a
//! terminal's sessions, run configurations — as a tree whose nodes' tabs are their views
//! (the Output log, a result's grid).
//!
//! Generic: it never knows who contributes. A module registers a
//! [`ui_components::ServiceProvider`] (the app puts them in `ui_components::services`), and
//! the window asks each one for the project's services only once it's first shown
//! ([`panel::ServicesPanel`]); its button, title and toggle (⌘8 / Alt+8) are a tool window
//! like any, its panel made on first show.

mod paint;
pub mod panel;
#[cfg(any(test, feature = "snapshot"))]
pub mod sample;
pub mod tree;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolPanel, ToolWindowProvider};
use workbench_model::{
    CommandContribution, CommandHandler, MenuContribution, MenuPlace, MenuRow, ToggleTool,
    WorkbenchHost,
};

pub use panel::{ServicesPanel, TOOL};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("services"),
    name: "Services",
    description: "The Services window at the bottom: what the modules have running (a data source's consoles, their results), as a tree with each node's tabs",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "services", ..extensions::ModuleInfo::BUILT_IN },
};

/// Shows / hides the window (⌘8 / Alt+8).
pub const TOGGLE: CommandId = workbench_model::TOGGLE_SERVICES;
/// Opens every node of the window (its header, its button's menu).
pub const EXPAND_ALL: CommandId = CommandId("services.expand_all");
/// Folds every node of the window.
pub const COLLAPSE_ALL: CommandId = CommandId("services.collapse_all");

#[derive(Debug, Default)]
pub struct ServicesModule;

impl Module for ServicesModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ServicesTool));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: TOGGLE,
                label: "Services",
                // In View › Tool Windows: its tool window's row.
                menu: None,
                shortcuts: &["CmdOrCtrl+8", "Alt+8"],
            },
            icon: Some("services"),
            handler: Arc::new(ToggleTool(TOOL)),
        });
        for (id, label, icon, action) in [
            (EXPAND_ALL, "Expand All (Services)", "chevrons-expand", "expand"),
            (COLLAPSE_ALL, "Collapse All (Services)", "chevrons-collapse", "collapse"),
        ] {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon: Some(icon),
                handler: Arc::new(Tell(action)),
            });
        }
        // Its keys in the presets' keymaps (jetbrains: ⌘8 / Alt+8).
        ctx.contribute(command_model::bindings::PresetShortcuts {
            preset: "jetbrains",
            command: TOGGLE,
            shortcuts: &["CmdOrCtrl+8", "Alt+8"],
        });
        // Its button's right-click menu (the same id).
        ctx.contribute(MenuContribution {
            id: TOOL,
            label_key: "services.title",
            icon: "services",
            places: &[MenuPlace::ToolStrip],
            rows: vec![
                MenuRow::Command(EXPAND_ALL, &[MenuPlace::ToolStrip]),
                MenuRow::Command(COLLAPSE_ALL, &[MenuPlace::ToolStrip]),
            ],
        });
        Ok(())
    }
}

/// The window: a panel at the bottom, after the terminal, made when first shown.
#[derive(Debug)]
pub struct ServicesTool;

impl ToolWindowProvider for ServicesTool {
    fn id(&self) -> &'static str {
        TOOL
    }

    fn icon(&self) -> &'static str {
        "services"
    }

    fn title(&self) -> String {
        i18n::t("services.title")
    }

    fn dock(&self) -> Dock {
        Dock::Bottom
    }

    /// The first of the bottom group, the farthest from the strip's corner (from the corner
    /// up: Git, Remote, the local history, the terminal, then the services).
    fn order(&self) -> u16 {
        5
    }

    fn toggle_command(&self) -> Option<&'static str> {
        Some(TOGGLE.0)
    }

    fn content(&self) -> ToolContent {
        ToolContent::Panel
    }

    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        Some(Box::new(ServicesPanel::new()))
    }
}

/// What a command asks the window ([`WorkbenchHost::tell_panel`]): one of its commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelCommand(pub &'static str);

/// Tells the window to run one of its commands (shown first).
#[derive(Debug)]
struct Tell(&'static str);

impl CommandHandler for Tell {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.tell_panel(TOOL, &PanelCommand(self.0), false)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
