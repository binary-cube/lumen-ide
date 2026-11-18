//! Its button: on the right strip's bottom group, right above Focus mode's (order 5; Relax
//! mode's is 10), lit while it's on (kept awake, or watching for work running); a click
//! runs [`crate::TOGGLE`], a right click shows its menu (the module's, by the same id).

use ui_components::{Dock, ToolContent, ToolWindowProvider};

/// Its id among the tool windows.
pub const TOOL: &str = "keep-awake";

#[derive(Debug)]
pub struct KeepAwakeButton;

impl ToolWindowProvider for KeepAwakeButton {
    fn id(&self) -> &'static str {
        TOOL
    }

    fn icon(&self) -> &'static str {
        crate::ICON
    }

    fn title(&self) -> String {
        i18n::t("keep_awake.title")
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    fn order(&self) -> u16 {
        3
    }

    fn strip_end(&self) -> bool {
        true
    }

    fn toggle_command(&self) -> Option<&'static str> {
        Some(crate::TOGGLE.0)
    }

    fn content(&self) -> ToolContent {
        ToolContent::Command { command: crate::TOGGLE.0, tab: None }
    }

    fn active(&self) -> bool {
        crate::settings::config().on
    }
}
