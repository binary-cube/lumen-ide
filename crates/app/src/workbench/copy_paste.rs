//! Copy, cut, paste and Select All where the keyboard is (a field, a view, the document),
//! and the text being composed by the input method.

use super::*;

impl Workbench {
    /// The text to copy: from the focused text field, or from the document.
    /// A collapsed region reached is copied whole (`Tab::folded`).
    pub fn copy(&mut self) -> Option<String> {
        match self.selected_outside_document() {
            Some(selected) => selected,
            None => Some(self.tabs.get_mut(self.active)?.folded(true, |doc| commands::copy(doc))),
        }
    }

    /// What's selected where the keyboard is (a field, the terminal, a view, the document);
    /// nothing selected: `None`, never the line a copy would take.
    pub fn selected_text(&self) -> Option<String> {
        let selected = match self.selected_outside_document() {
            Some(selected) => selected,
            None => {
                let doc = &self.tabs.get(self.active)?.doc;
                let selection = doc.selections().primary();
                (!selection.is_empty()).then(|| commands::copy(doc))
            }
        };
        selected.filter(|s| !s.is_empty())
    }

    /// `Some`: what a field or a view having the keyboard holds selected (maybe nothing).
    /// `None`: it's the document's.
    fn selected_outside_document(&self) -> Option<Option<String>> {
        // A window over everything answering the clipboard itself (a module's view): what
        // its field (or its view) has selected, never the document behind it.
        if self.input_owner().is_none()
            && let Some(selected) = self.active_overlay().and_then(|o| o.slot().selected_text(self))
        {
            return Some(selected);
        }
        if let Some(selected) = self.view_selection() {
            return Some(Some(selected).filter(|s| !s.is_empty()));
        }
        // The bottom view's field with nothing selected: nothing (not the editor's text).
        if self.panel_field_focused() {
            return Some(None);
        }
        // The bottom view has the keyboard (the terminal) and nothing selected in it:
        // nothing copied (not what's selected elsewhere).
        if self.view_takes_keyboard() && !self.overlay_open() {
            return Some(None);
        }
        if let Some(side) = self.side_typing() {
            return Some(side.panel.selected_text());
        }
        if self.input_owner().is_some() {
            let selected = self.active_input().map(|input| input.selected_text().to_owned());
            return Some(selected.filter(|s| !s.is_empty()));
        }
        let tab = self.tabs.get(self.active)?;
        // Beside the text, a preview clicked last: its selected text (nothing if none).
        if self.focus == Focus::Editor
            && let Some(v) = tab.viewer.as_ref().filter(|v| v.selects())
        {
            return Some(v.viewer.selected_text());
        }
        // A view having the keys (a grid): what's selected in it.
        if let Some(v) = &tab.viewer
            && (!v.editor_visible() || (self.focus == Focus::Editor && v.has_keys()))
        {
            let selected = v.viewer.selected_text();
            // Beside the text, nothing selected in it copies nothing (not the text's).
            if selected.is_some() || v.editor_visible() {
                return Some(selected);
            }
        }
        None
    }

    /// An edit command for the window on top's own field or view: `None` when it takes none.
    pub(super) fn overlay_edit(&mut self, id: &str) -> Option<bool> {
        self.active_overlay()?.slot().edit_command(self, id)
    }

    pub fn cut(&mut self, text: &mut TextSystem) -> Option<String> {
        // A window over everything answering the clipboard itself: its field's selection,
        // never the document's.
        if self.input_owner().is_none()
            && let Some(selected) = self.active_overlay().and_then(|o| o.slot().selected_text(self))
        {
            self.overlay_edit("cut");
            return selected;
        }
        if self.input_owner().is_some() || self.side_typing().is_some() {
            let copied = self.copy();
            // Inserting empty text deletes the selection in the field (and refreshes what
            // depends on it, e.g. a filter).
            self.type_text(text, "");
            return copied;
        }
        // The bottom view's field: its selection cut.
        if let Some(v) = self.panel_field() {
            let selected = v.selected_text();
            v.command("cut");
            return selected;
        }
        // A view having the keys: its field's text cut; cells only copied, but in a view
        // standing in for its file (a diagram's shapes): copied, then cut.
        if let Some(v) = self.keyed_viewer() {
            if !v.viewer.wants_text() {
                let selected = v.viewer.selected_text();
                if !v.editor_visible() && selected.is_some() {
                    v.viewer.command("cut");
                }
                return selected;
            }
            let selected = v.viewer.selected_text();
            v.viewer.command("cut");
            return selected;
        }
        let mut copied = None;
        self.edit(text, |doc| copied = Some(commands::cut(doc)));
        copied
    }

    pub fn paste(&mut self, text: &mut TextSystem, clip: &str) -> bool {
        if self.view_takes_keyboard() {
            return self.paste_into_view(clip);
        }
        // The bottom view's field: its first line, typed in (all of it in a field of several
        // lines: a commit's message).
        if let Some(v) = self.panel_field() {
            let line =
                if v.field_takes_lines() { clip } else { clip.lines().next().unwrap_or_default() };
            return v.paste(line) || v.text_input(line);
        }
        if self.input_owner().is_some() || self.side_typing().is_some() {
            return self.type_text(text, clip.lines().next().unwrap_or_default());
        }
        // A window over everything whose view takes what's pasted.
        if let Some(o) = self.active_overlay()
            && let Some(used) = o.slot().paste(self, text, clip.lines().next().unwrap_or_default())
        {
            return used;
        }
        // A viewer with its own field (the commit message, the canvas search).
        if let Some(v) = self.keyed_viewer() {
            // Beside the text, a view without a field takes nothing pasted.
            if v.viewer.wants_text() || !v.editor_visible() {
                return v.viewer.wants_text() && v.viewer.text_input(clip);
            }
        }
        self.edit(text, |doc| commands::paste(doc, clip))
    }

    /// ⌘A: the whole text of the focused field, a viewer's field, or the document.
    pub fn select_all(&mut self, text: &mut TextSystem) -> bool {
        if let Some(input) = self.active_input_mut() {
            input.select_all();
            return true;
        }
        if let Some(used) = self.overlay_edit("select_all") {
            return used;
        }
        // The bottom view's text field (the Git log's filters).
        if let Some(v) = self.panel_field() {
            return v.command("select_all");
        }
        // A viewer's text field (e.g. the canvas search, the theme name).
        if self.focus == Focus::Editor
            && let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && v.has_keys()
            && v.viewer.wants_text()
        {
            return v.viewer.command("select_all");
        }
        // Beside the text, a preview clicked last: all its text.
        if let Some(v) = self.selecting_viewer() {
            v.viewer.select_all();
            return true;
        }
        // A view selecting what it shows (a grid's rows).
        if self.focus == Focus::Editor
            && let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut())
            && v.has_keys()
        {
            // Beside the text, the view's ⌘A stays in it.
            if v.viewer.select_all() || v.editor_visible() {
                return true;
            }
        }
        self.edit(text, commands::select_all)
    }

    pub fn set_preedit(&mut self, preedit: Option<String>) -> bool {
        match self.tabs.get_mut(self.active) {
            // Locked: nothing composed in it either (what's committed is refused).
            Some(tab) if self.focus == Focus::Editor && (preedit.is_none() || !tab.locked()) => {
                tab.view.set_preedit(preedit);
                true
            }
            _ => false,
        }
    }
}
