//! Its button: on the right strip's bottom group, right above Relax mode's (order 10; the
//! Task Manager's is 20), lit while it's on; a click runs [`crate::TOGGLE`].

use ui_components::{Dock, ToolContent, ToolWindowProvider};

/// Its id among the tool windows.
pub const TOOL: &str = "focus";

#[derive(Debug)]
pub struct FocusButton;

impl ToolWindowProvider for FocusButton {
    fn id(&self) -> &'static str {
        TOOL
    }

    fn icon(&self) -> &'static str {
        crate::ICON
    }

    fn title(&self) -> String {
        i18n::t("focus.title")
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    fn order(&self) -> u16 {
        5
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
        crate::state::is_on()
    }
}

#[cfg(test)]
#[path = "../tests/unit/button.rs"]
mod tests;
