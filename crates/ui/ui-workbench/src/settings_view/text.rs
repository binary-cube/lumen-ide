//! The settings' text: the search field and the shortcuts' one (which has the keys), a
//! block's field being written, a shortcut recorded, the results walked.

use super::*;

impl SettingsView {
    pub fn has_query(&self) -> bool {
        !self.query.text().trim().is_empty()
    }

    /// The search field with the keys: the shortcuts' while it has them, else the settings'
    /// (typing anywhere in the settings searches them).
    fn keys_field(&mut self) -> usize {
        let field = self.field_with_keys();
        self.with_controls(|c, parts| c.focus(parts, Some(field)));
        field
    }

    /// The search field the keys reach, without moving the focus: the shortcuts' while it
    /// has the focus and its page shows, else the settings'.
    fn field_with_keys(&self) -> usize {
        match self.controls.focused() {
            Some(FILTER) if self.on_keymap() => FILTER,
            _ => QUERY,
        }
    }

    /// The keymap's page shows (its shortcuts' search with it).
    pub(super) fn on_keymap(&self) -> bool {
        !self.has_query() && self.active() == KEYMAP
    }

    pub fn type_text(&mut self, typed: &str) {
        if let Some(editing) = &mut self.editing {
            editing.input.insert(typed);
            return;
        }
        let field = self.keys_field();
        self.with_controls(|c, parts| c.text(parts, typed));
        if field == QUERY {
            self.selected = 0;
        }
    }

    pub fn backspace(&mut self, word: bool) {
        if let Some(editing) = &mut self.editing {
            editing.input.backspace(word);
            return;
        }
        let field = self.keys_field();
        let reach = ui_components::KeyReach { word, extend: false };
        self.with_controls(|c, parts| c.key(parts, ui_components::ViewerKey::Backspace, reach));
        if field == QUERY {
            self.selected = 0;
        }
    }

    /// The field with the keys (for clipboard / select-all routing): a block's text field
    /// being written, else the search field with the focus.
    pub fn query_input(&self) -> &ui_kit::TextInput {
        if let Some(editing) = &self.editing {
            return &editing.input;
        }
        match self.field_with_keys() {
            FILTER => &self.shortcuts.filter.input,
            _ => &self.query.input,
        }
    }

    /// The same field as `query_input`, to edit: asking never moves the focus from one
    /// field to another (with none focused, the settings' search takes it).
    pub fn query_input_mut(&mut self) -> &mut ui_kit::TextInput {
        if self.editing.is_some() {
            return match &mut self.editing {
                Some(editing) => &mut editing.input,
                None => &mut self.query.input,
            };
        }
        let field = self.field_with_keys();
        if self.controls.focused().is_none() {
            self.with_controls(|c, parts| c.focus(parts, Some(field)));
        }
        match field {
            FILTER => &mut self.shortcuts.filter.input,
            _ => &mut self.query.input,
        }
    }

    /// A block's text field is being written.
    pub fn editing_text(&self) -> bool {
        self.editing.is_some()
    }

    /// Enter (or a click elsewhere): the text written, to save.
    pub fn commit_text(&mut self) -> Option<SettingsAction> {
        let Editing { block, key, input } = self.editing.take()?;
        Some(SettingsAction::SetText { block, key, value: input.text().trim().to_owned() })
    }

    /// Esc: the change given up (a field being written, a shortcut being recorded or its
    /// conflict, the shortcuts' search). `false` if there was none.
    pub fn cancel_text(&mut self) -> bool {
        if self.editing.take().is_some() || self.shortcuts.cancel() {
            return true;
        }
        // The shortcuts' search left (its caret gone; typing searches the settings again).
        let leaving = self.controls.focused() == Some(FILTER);
        if leaving {
            self.with_controls(|c, parts| c.focus(parts, None));
        }
        leaving
    }

    /// Settings → Shortcuts: `command` opened, recording its next keys.
    pub fn open_shortcut(&mut self, command: &str) {
        self.shortcuts.open_recording(command);
    }

    /// The next keys pressed are a shortcut being recorded (they don't act).
    pub fn recording_shortcut(&self) -> bool {
        self.shortcuts.recording()
    }

    /// Keys pressed while recording (as written, as shown): the change, unless others
    /// have them (then it asks).
    pub fn record_shortcut(&mut self, spec: &str, shown: &str) -> Option<SettingsAction> {
        self.shortcuts.record(spec, shown).map(SettingsAction::Shortcuts)
    }

    /// The shortcuts' search or a recording has the keys (as a field being written does).
    pub fn shortcuts_have_keys(&self) -> bool {
        self.controls.focused() == Some(FILTER) || self.shortcuts.recording()
    }

    pub fn clear_query(&mut self) {
        self.query.set_text("");
        self.results.clear();
        self.searched = None;
    }

    /// ↑ / ↓ through the results.
    pub fn move_result(&mut self, delta: isize) -> bool {
        if self.results.is_empty() {
            return false;
        }
        self.selected = ui_kit::step_index(self.selected, delta, self.results.len());
        true
    }

    /// Enter: opens the selected result.
    pub fn confirm(&mut self, model: &SettingsModel) -> bool {
        let Some(&entry) = self.results.get(self.selected) else { return false };
        self.open_entry(model, entry);
        true
    }
}
