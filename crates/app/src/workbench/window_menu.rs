//! A tool window's options (the "⋮" in its header, before its hide): what its module offers
//! there — the same menu its strip button's right click shows (a `MenuContribution` with
//! the window's id: its switches, its "Settings…") — then the app's own rows for every
//! window: close its tabs, half or all the height (at the bottom), hide. The app knows no
//! module's rows: it only puts them first.

use super::*;
use tool_windows::BottomSize;

/// The header button opening the menu (a side panel's; the bottom windows' is
/// [`tool_windows::ToolHit::More`]).
pub const WINDOW_MORE: &str = "window-more";

/// The prefix of the app's own rows: `window-menu:<action>:<window id>`.
pub const WINDOW_MENU: &str = "window-menu:";

/// The app's own rows' actions.
const CLOSE_ALL: &str = "close-all";
const HALF: &str = "half";
const FULL: &str = "full";
const HIDE: &str = "hide";

impl Workbench {
    /// The rows of window `id`'s "⋮": its module's, then the app's.
    pub(super) fn window_menu_entries(&mut self, id: &str) -> Vec<ui_workbench::MenuEntry> {
        use ui_workbench::MenuEntry;
        let mut entries = self.tool_menu_entries(id).unwrap_or_default();
        entries.push(MenuEntry::Separator);
        let row = |action: &str, key: &str, icon: &'static str, on: Option<bool>| MenuEntry::Item {
            id: format!("{WINDOW_MENU}{action}:{id}"),
            label: i18n::t(key),
            shortcut: None,
            icon: Some(icon),
            on,
            look: Default::default(),
        };
        let tool = self.tool(id);
        let views =
            matches!(tool.map(|t| t.content()), Some(ui_workbench::ToolContent::Views { .. }));
        if views && self.bottom_shows(id) && self.tools.window().is_some_and(|w| !w.is_empty()) {
            entries.push(row(CLOSE_ALL, "window_menu.close_all", "x", None));
        }
        if tool.is_some_and(|t| t.dock() == ui_workbench::Dock::Bottom) {
            let now = self.tools.bottom_size;
            let half =
                row(HALF, "tooltip.bottom_half", "view-split-down", Some(now == BottomSize::Half));
            entries.push(half);
            entries.push(row(
                FULL,
                "tooltip.bottom_full",
                "maximize-2",
                Some(now == BottomSize::Full),
            ));
        }
        entries.push(row(HIDE, "tooltip.bottom_hide", "minus", None));
        entries
    }

    /// Opens window `id`'s "⋮" under its button `at`.
    pub(super) fn open_window_menu(&mut self, id: &str, at: Rect) {
        let entries = self.window_menu_entries(id);
        self.open_context_menu(entries, Point::new(at.min_x(), at.max_y() + 4.0 * self.scale));
    }

    /// One of the app's rows of a window's "⋮" (its id, [`WINDOW_MENU`] taken off). `true`
    /// if something changed.
    pub fn window_menu(&mut self, row: &str) -> bool {
        let Some((action, id)) = row.split_once(':') else { return false };
        let side = self.tools.sides.iter().any(|s| s.id == id);
        match action {
            CLOSE_ALL => {
                let groups = self.tools.window().map_or(0, |w| w.groups.len());
                // From the last group: closing one's views may take the group away.
                for g in (0..groups).rev() {
                    self.close_tool_views(g, |_, v| !v.pinned);
                }
            }
            HALF => self.toggle_bottom_size(BottomSize::Half),
            FULL => self.toggle_bottom_size(BottomSize::Full),
            HIDE if side => self.close_side_panel(id),
            HIDE if self.bottom_shows(id) => self.set_bottom(None),
            _ => return false,
        }
        self.layout();
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/window_menu.rs"]
mod tests;
