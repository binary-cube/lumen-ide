//! The editor area taking the window (the tab bar's button, its commands): the side windows
//! hidden (all the width), the bottom one (all the height), or both; what was hidden kept, and
//! shown again as it was when it goes back — whatever was opened meanwhile stays.

use super::*;

/// What the editor area hid to take the window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Hidden {
    /// The side panels it hid (their ids).
    sides: Vec<&'static str>,
    /// The bottom window it hid: a side panel docked at the bottom, or a tool window.
    bottom: Option<(String, bool)>,
}

impl Workbench {
    /// The editor area all the width (`width`), all the height (`height`), or both — or,
    /// taking the window already, back as it was. `true`: something changed.
    pub fn toggle_editor_full(&mut self, width: bool, height: bool) -> bool {
        if let Some(hidden) = self.editor_full.take() {
            for id in hidden.sides {
                if !self.tools.sides.iter().any(|s| s.id == id) {
                    self.toggle_side_panel(id);
                }
            }
            if let Some((id, side)) = hidden.bottom
                && self.tools.bottom.is_none()
            {
                if side {
                    self.toggle_side_panel(&id);
                } else {
                    self.set_bottom(Some(&id));
                }
            }
            self.layout();
            return true;
        }
        let mut hidden = Hidden::default();
        if width {
            hidden.sides = self
                .tools
                .sides
                .iter()
                .filter(|s| matches!(s.dock, ui_workbench::Dock::Left | ui_workbench::Dock::Right))
                .map(|s| s.id)
                .collect();
            for id in &hidden.sides {
                self.toggle_side_panel(id);
            }
        }
        if height && let Some(id) = self.tools.bottom.clone() {
            let side = self.tools.sides.iter().any(|s| s.id == id);
            if side {
                self.toggle_side_panel(&id);
            } else {
                self.set_bottom(None);
            }
            hidden.bottom = Some((id, side));
        }
        if hidden == Hidden::default() {
            return false;
        }
        self.editor_full = Some(hidden);
        self.layout();
        true
    }

    /// The editor area takes the window now.
    pub fn editor_full(&self) -> bool {
        self.editor_full.is_some()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/editor_full.rs"]
mod tests;
