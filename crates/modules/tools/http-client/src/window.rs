//! The HTTP client's button (plan 77i, 77l): no window of its own — it opens the HTTP tab
//! ([`crate::collection_view`]: every collection's tree, searched, beside the visual editor
//! of the request chosen), one per project, brought forward when it's open.

use i18n::t;
use ui_components::tool_panel::ToolPanel;
use ui_components::{Dock, ToolContent, ToolWindowProvider};
use workbench_model::{CommandHandler, WorkbenchHost};

/// Its button's id.
pub const TOOL: &str = "http";
pub use crate::request_tree::THREAD;

/// [`TOGGLE`]: the HTTP tab opened with the project's whole tree (open: brought forward).
#[derive(Debug)]
pub struct Open;

impl CommandHandler for Open {
    fn requires(&self) -> &'static [command_model::Requires] {
        &[command_model::Requires::Project]
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if host.focus_view(crate::collection_view::EXTENSION) {
            return true;
        }
        let data = crate::collection_view::CollectionData {
            queries: host.queries(),
            project: host.project_id(),
            collection: String::new(),
            select: None,
        };
        host.open_view(&t("http.window_title"), crate::collection_view::EXTENSION, &data)
    }
}

/// Its button, on the right strip.
#[derive(Debug)]
pub struct HttpTool;

impl ToolWindowProvider for HttpTool {
    fn id(&self) -> &'static str {
        TOOL
    }

    fn icon(&self) -> &'static str {
        "globe"
    }

    fn title(&self) -> String {
        t("http.window_title")
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    fn order(&self) -> u16 {
        35
    }

    /// Its group on the strip: the services the project talks to.
    fn group(&self) -> &'static str {
        "services"
    }

    fn requires(&self) -> &'static [command_model::Requires] {
        &[command_model::Requires::Project]
    }

    fn toggle_command(&self) -> Option<&'static str> {
        Some(TOGGLE.0)
    }

    // No window of its own: the button opens the HTTP tab (lit while it's the active one).
    fn content(&self) -> ToolContent {
        ToolContent::Command { command: TOGGLE.0, tab: Some(crate::collection_view::EXTENSION) }
    }

    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        None
    }
}

/// Opens (or brings forward) the HTTP tab.
pub const TOGGLE: command_model::CommandId = command_model::CommandId("view.toggle_http");

#[cfg(test)]
#[path = "../tests/unit/window.rs"]
mod tests;
