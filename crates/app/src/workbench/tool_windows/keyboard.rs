//! A view having the keyboard: the bottom one (its text, its selection, its field), or one
//! that takes the keyboard whole — the bottom window's (the terminal) or the active tab's (a
//! terminal opened in the editor, an agent's session).

use super::*;

impl Workbench {
    /// The active editor tab's view takes the keyboard whole, and has it: a view in the
    /// file's place (no text beside it), the editor focused.
    fn tab_takes_keyboard(&self) -> bool {
        self.focus == Focus::Editor
            && !self.keyboard_in_overlay()
            && self.tabs.get(self.active).and_then(|t| t.viewer.as_ref()).is_some_and(|v| {
                v.viewer.takes_keyboard() && v.kind == ui_workbench::ViewerKind::Replace
            })
    }

    /// The view taking the keyboard whole, where it has it: the bottom one, else the
    /// active tab's.
    fn keyboard_view(&mut self) -> Option<&mut Box<dyn ui_workbench::Viewer>> {
        if self.focus == Focus::Panel {
            return self.tools.panel_viewer();
        }
        if self.tab_takes_keyboard() {
            return self.tabs.get_mut(self.active)?.viewer.as_mut().map(|v| &mut v.viewer);
        }
        None
    }

    /// A view has the keyboard and takes it whole (a terminal, at the bottom or in a tab).
    pub fn view_takes_keyboard(&self) -> bool {
        let bottom = self.focus == Focus::Panel
            && !self.keyboard_in_overlay()
            && self
                .tools
                .window()
                .and_then(|w| w.group().active_view())
                .is_some_and(|v| v.viewer.takes_keyboard());
        bottom || self.tab_takes_keyboard()
    }

    /// A key press for the view taking the keyboard. `true` if it took it.
    pub fn view_key_press(&mut self, key: ui_workbench::KeyPress) -> bool {
        self.keyboard_view().is_some_and(|v| v.key_press(key))
    }

    /// ⌘ / Ctrl held over the bottom view: its link under `p` shown (`None`: released or
    /// elsewhere). `true` if it changed.
    pub(in crate::workbench) fn panel_link_hover(&mut self, p: Option<Point>) -> bool {
        self.tool_window_shown() && self.tools.panel_viewer().is_some_and(|v| v.link_hover(p))
    }

    /// The text selected in the view having the keyboard (the bottom one, or a tab's taking
    /// it whole).
    pub fn view_selection(&self) -> Option<String> {
        if self.tab_takes_keyboard() {
            return self.tabs.get(self.active)?.viewer.as_ref()?.viewer.selected_text();
        }
        if self.focus != Focus::Panel {
            return None;
        }
        self.tools
            .window()
            .and_then(|w| w.group().active_view())
            .and_then(|v| v.viewer.selected_text())
    }

    /// The bottom view has the keyboard in one of its text fields: the edit commands
    /// (Select All, Cut, Paste, Undo / Redo) are its.
    pub(in crate::workbench) fn panel_field(
        &mut self,
    ) -> Option<&mut Box<dyn ui_workbench::Viewer>> {
        if self.focus != Focus::Panel || !self.tool_window_shown() || self.keyboard_in_overlay() {
            return None;
        }
        self.tools.panel_viewer().filter(|v| v.wants_text() && !v.takes_keyboard())
    }

    /// The bottom view's text field has the keyboard (nothing else's text is copied).
    pub(in crate::workbench) fn panel_field_focused(&self) -> bool {
        self.focus == Focus::Panel
            && self
                .tools
                .window()
                .and_then(|w| w.group().active_view())
                .is_some_and(|v| v.viewer.wants_text() && !v.viewer.takes_keyboard())
    }

    pub fn clear_view_selection(&mut self) -> bool {
        self.keyboard_view().is_some_and(|v| v.clear_selection())
    }

    /// Pastes into the view having the keyboard. `true` if it took the text.
    pub(in crate::workbench) fn paste_into_view(&mut self, text: &str) -> bool {
        self.keyboard_view().is_some_and(|v| v.paste(text))
    }

    /// Typed text for the bottom view, or a tab's taking the keyboard whole. `None`: neither
    /// has the keyboard.
    pub fn type_into_view(&mut self, typed: &str) -> Option<bool> {
        if self.tab_takes_keyboard() {
            return self.keyboard_view().map(|v| v.wants_text() && v.text_input(typed));
        }
        if self.focus != Focus::Panel || !self.tool_window_shown() {
            return None;
        }
        let viewer = self.tools.panel_viewer()?;
        viewer.wants_text().then(|| viewer.text_input(typed))
    }
}
