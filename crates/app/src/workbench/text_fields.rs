//! The text field that has the keyboard: whose it is (a window over the editor, the empty
//! screen, the find bar), its cursor keys and a selection set from outside.

use super::editing::move_cursor;
use super::*;

impl Workbench {
    /// A modal or a window over the editor with a text field has the keyboard: what's
    /// behind it shows no caret (and gets no keys).
    pub(super) fn keyboard_in_overlay(&self) -> bool {
        self.modal_open()
            || self.active_overlay().is_some_and(|o| {
                let slot = o.slot();
                slot.has_input(self) || slot.input(self).is_some()
            })
    }

    /// Who has the keyboard's text field that the app edits: the window over the editor on
    /// top when it has one (none when it hasn't, or its view edits its own: nothing behind
    /// it is edited), else the app's screens (the empty screen's search, the find bar).
    pub(super) fn input_owner(&self) -> Option<InputOwner> {
        // Down the open windows over the editor: the first with a field has it; a popover
        // without one (a context menu) lets it through to what's under it, a modal never.
        if self.active_overlay().is_some() {
            for o in Overlay::ALL {
                let slot = o.slot();
                if slot.floats(self) || !slot.is_open(self) {
                    continue;
                }
                if slot.input(self).is_some() {
                    return Some(InputOwner::Overlay(o));
                }
                if slot.kind(self) == OverlayKind::Modal {
                    return None;
                }
            }
            return None;
        }
        if self.home_visible() && self.focus == Focus::Editor {
            return Some(InputOwner::Home);
        }
        let find =
            self.focus == Focus::Find && self.tabs.get(self.active).is_some_and(|t| t.find_open);
        find.then_some(InputOwner::Find)
    }

    /// The side panel having the keyboard, while it takes text (its field: the project
    /// panel's search, a new entry's name).
    pub(super) fn side_typing(&self) -> Option<&SidePanel> {
        let Focus::Side(dock) = self.focus else { return None };
        if self.keyboard_in_overlay() {
            return None;
        }
        self.tools.side_on(dock).filter(|s| s.panel.wants_text())
    }

    /// The text field that has the keyboard (see `input_owner`).
    pub(super) fn active_input(&self) -> Option<&ui_kit::TextInput> {
        Some(match self.input_owner()? {
            InputOwner::Overlay(o) => o.slot().input(self)?,
            InputOwner::Home => &self.home.picker.query.input,
            InputOwner::Find => self.tabs.get(self.active)?.find_bar.focused_input_ref(),
        })
    }

    /// A cursor key for the text field having the keyboard. `true` if it moved in it.
    pub(super) fn move_in_field(&mut self, key: &EditKey) -> bool {
        let EditKey::Move { motion, extend } = *key else { return false };
        self.active_input_mut().is_some_and(|input| move_cursor(input, motion, extend))
    }

    /// The text field with the keyboard's selection set to `range` (bytes: a screen reader
    /// moved its caret) — only if it's the field the reader meant, its text `meant` (the
    /// node's value); `false`: no field has the keyboard, or another one has it (a find
    /// bar's while the reader was in a setting's), or the node wasn't a field's whole text
    /// (a code editor tells only its lines shown).
    pub fn select_in_field(&mut self, range: std::ops::Range<usize>, meant: &str) -> bool {
        let Some(input) = self.active_input_mut() else { return false };
        let text = input.text();
        if text != meant {
            return false;
        }
        let fit = |b: usize| text.floor_char_boundary(b.min(text.len()));
        let range = fit(range.start)..fit(range.end);
        input.select(range);
        true
    }

    /// The code editor with the keyboard: the selection set from a screen reader's places,
    /// each (the line among those it told, a character in it); `false`: the editor hasn't
    /// the keyboard (a field has it), or the line isn't one it told.
    pub fn select_in_editor(&mut self, anchor: (usize, usize), head: (usize, usize)) -> bool {
        if self.focus != Focus::Editor || self.active_input_mut().is_some() {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let buffer = tab.doc.buffer();
        let offset = |(told, character): (usize, usize)| {
            let line = tab.view.told_line(told)? as usize;
            let text = buffer.line_text(line);
            let inside = text.char_indices().nth(character).map_or(text.len(), |(at, _)| at);
            Some(buffer.line_start(line) + inside)
        };
        let (Some(anchor), Some(head)) = (offset(anchor), offset(head)) else { return false };
        let selection = document::Selection { anchor, head, goal: None };
        tab.doc.set_selections(document::Selections::single(selection));
        true
    }

    /// A text field of the window's has the keyboard (a screen reader's value goes in it).
    pub fn text_field_focused(&mut self) -> bool {
        self.active_input_mut().is_some()
    }

    pub(super) fn active_input_mut(&mut self) -> Option<&mut ui_kit::TextInput> {
        Some(match self.input_owner()? {
            InputOwner::Overlay(o) => o.slot().input_mut(self)?,
            InputOwner::Home => &mut self.home.picker.query.input,
            InputOwner::Find => self.tabs.get_mut(self.active)?.find_bar.focused_input(),
        })
    }
}

/// Who has the keyboard's text field (see `Workbench::input_owner`): a window over the
/// editor (its slot says which field), or one of the app's screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputOwner {
    Overlay(Overlay),
    Home,
    Find,
}
