//! Which menu rows apply now: the window's state (`MenuContext`: a project, a repository,
//! the active tab…) against what each command needs (the app's `commands::requires`, a
//! module's handler's `requires`, then its `enabled`). A row that doesn't apply is greyed
//! out — in the native menu bar and the drawer alike — and its keys do nothing. Made only
//! when the state changes (`EditorApp::sync_menu_states`), never per frame.

use command_model::{MenuContext, Requires};
use platform_api::MenuItem;

use super::*;

/// Whether a menu row applies now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Enabled,
    /// Greyed out, with what it lacks (`None`: its module said no).
    Disabled(Option<Requires>),
}

impl Workbench {
    /// The window's state the commands' requirements are checked against (cheap: no text
    /// read).
    pub fn menu_context(&self) -> MenuContext {
        let tab = self.tabs.get(self.active);
        let open = |id: &DocumentId| self.tabs.iter().any(|t| t.doc.id() == *id);
        MenuContext {
            project: self.project.is_some(),
            repository: self.data.git.is_some(),
            tabs: self.tabs.len(),
            text_file: tab.is_some_and(|t| t.viewer.as_ref().is_none_or(|v| v.editor_visible())),
            file_on_disk: tab.and_then(|t| t.doc.path()).is_some_and(|p| !disk::is_virtual(p)),
            language: tab.map(|t| t.syntax.language().as_str().to_owned()),
            modified: tab.is_some_and(Tab::is_dirty),
            closed_tab: self.can_reopen(),
            back: self.back.iter().any(open),
            forward: self.forward.iter().any(open),
            lockable: tab.is_some_and(Tab::lockable),
            keeps_on_top: self.chrome.can_keep_on_top,
        }
    }

    /// Whether menu row `id` (a command, a tool window's) applies in `context`.
    pub fn row_state(&self, context: &MenuContext, id: &str) -> RowState {
        // A tool window without a toggle of its own: while its button shows.
        if let Some(tool) = id.strip_prefix(crate::commands::TOOL_WINDOW_PREFIX) {
            // The provider's own requirements (a project, a repository).
            if let Some(missing) = self.tool(tool).and_then(|t| context.missing(t.requires())) {
                return RowState::Disabled(Some(missing));
            }
            let command = self.tool(tool).and_then(|t| match t.content() {
                ui_workbench::ToolContent::Views { command, .. }
                | ui_workbench::ToolContent::Command { command, .. } => Some(command),
                ui_workbench::ToolContent::Panel => None,
            });
            return match command {
                Some(command) => self.row_state(context, command),
                None => RowState::Enabled,
            };
        }
        let Some(command) = crate::commands::by_id(id) else { return RowState::Enabled };
        if command == crate::commands::CLEAR_RECENT && crate::commands::recent_menu_is_empty() {
            return RowState::Disabled(None);
        }
        if let Some(missing) = context.missing(crate::commands::requires(command)) {
            return RowState::Disabled(Some(missing));
        }
        let support = &self.support;
        let handler = support.commands.iter().find(|c| c.spec.id == command).map(|c| &c.handler);
        let handler = handler.or_else(|| {
            support.implementations.iter().find(|c| c.id == command).map(|c| &c.handler)
        });
        match handler {
            Some(handler) => match context.missing(handler.requires()) {
                Some(missing) => RowState::Disabled(Some(missing)),
                None if !handler.enabled(self) => RowState::Disabled(None),
                None => RowState::Enabled,
            },
            // A workbench command a module implements, without that module.
            None if workbench_model::IMPLEMENTED.contains(&command) => RowState::Disabled(None),
            None => RowState::Enabled,
        }
    }

    /// Command `id` applies now (its keys, a menu's row run): the state read fresh.
    pub fn command_applies(&self, id: command_model::CommandId) -> bool {
        self.row_state(&self.menu_context(), id.0) == RowState::Enabled
    }

    /// The menu bar's rows that don't apply in `context`, with what they lack.
    pub fn disabled_rows(&self, context: &MenuContext) -> Vec<(String, Option<Requires>)> {
        fn walk(
            wb: &Workbench,
            context: &MenuContext,
            items: &[MenuItem],
            out: &mut Vec<(String, Option<Requires>)>,
        ) {
            for item in items {
                match item {
                    MenuItem::Command { id, .. } => {
                        if let RowState::Disabled(missing) = wb.row_state(context, id) {
                            out.push((id.clone(), missing));
                        }
                    }
                    MenuItem::Submenu { items, .. } => walk(wb, context, items, out),
                    MenuItem::Separator | MenuItem::Native { .. } => {}
                }
            }
        }
        let mut out = Vec::new();
        for menu in crate::commands::menus() {
            walk(self, context, &menu.items, &mut out);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/menu_states.rs"]
mod tests;
