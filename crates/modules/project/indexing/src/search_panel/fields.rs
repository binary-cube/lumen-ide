//! The fields beside the search's: the folder and the mask, the replacement, the options, the folders suggested, which one has the keyboard.

use super::*;
use ui_components::KeyReach;
use ui_components::control::Press;
use ui_components::picker::{PickerEvent, PickerItem};

impl ProjectSearch {
    /// The folder searched in: `folder` (relative to the project; empty: all of it), its row
    /// shown when there's one.
    pub fn set_scope(&mut self, folder: &str) {
        self.scope.set_text(folder);
        self.scope_open = !folder.is_empty();
        self.in_scope = false;
        self.clear_suggestions();
    }

    /// The folder searched in as the start of the paths in it (`src/Controller/`).
    pub fn scope_prefix(&self) -> Option<String> {
        let folder = self.scope.text().trim().trim_matches('/');
        (!folder.is_empty()).then(|| format!("{folder}/"))
    }

    /// The files' mask set (its row shown when there's one).
    pub fn set_mask(&mut self, mask: &str) {
        self.mask.set_text(mask);
        self.scope_open |= !mask.trim().is_empty();
    }

    /// A folder or a mask narrows the search.
    pub(super) fn narrowed(&self) -> bool {
        self.scope_prefix().is_some() || !self.mask.text().trim().is_empty()
    }

    /// The keyboard to the mask's field (`true`, when its row shows) or the search's.
    pub fn focus_mask(&mut self, on: bool) {
        self.focus_scope(false);
        self.in_replace = false;
        self.in_mask = on && self.scope_open;
    }

    #[cfg(test)]
    pub fn in_mask(&self) -> bool {
        self.in_mask
    }

    /// The keyboard is in the search's own field (not the folder's, the mask's or the
    /// replacement's).
    pub fn in_query(&self) -> bool {
        !self.in_scope && !self.in_mask && !self.in_replace
    }

    /// The folder's button: its row shows and takes the keyboard; pressed again with no
    /// folder or mask set, it hides.
    pub fn toggle_scope(&mut self) {
        if self.scope_open && !self.narrowed() {
            self.scope_open = false;
            self.focus_scope(false);
        } else {
            self.scope_open = true;
            self.focus_scope(true);
        }
    }

    /// The keyboard to the folder's field (`true`, when its row shows) or the search's.
    pub fn focus_scope(&mut self, on: bool) {
        self.in_scope = on && self.scope_open;
        if self.in_scope {
            self.in_replace = false;
            self.in_mask = false;
        } else {
            self.clear_suggestions();
        }
    }

    /// The keyboard to the replacement's field (`true`, when its row shows) or the search's.
    pub fn focus_replace(&mut self, on: bool) {
        self.in_replace = on && self.replace_open;
        if self.in_replace {
            self.focus_scope(false);
            self.in_mask = false;
        }
    }

    /// The replacement's row shown (taking the keyboard) or hidden.
    pub fn set_replace_open(&mut self, open: bool) {
        self.replace_open = open;
        self.focus_replace(open);
    }

    pub fn replace_open(&self) -> bool {
        self.replace_open
    }

    pub fn in_replace(&self) -> bool {
        self.in_replace
    }

    /// Replace All can be pressed: every occurrence found, and some.
    pub fn set_can_replace(&mut self, can: bool) {
        self.can_replace = can;
    }

    /// The next field (Tab): the search's, the replacement's, the folder's, the mask's, as
    /// they show.
    pub fn next_field(&mut self) {
        if self.in_mask {
            self.in_mask = false;
        } else if self.in_scope {
            self.focus_mask(true);
        } else if self.in_replace {
            self.in_replace = false;
            if self.scope_open {
                self.focus_scope(true);
            }
        } else if self.replace_open {
            self.focus_replace(true);
        } else {
            self.focus_scope(true);
        }
    }

    pub fn in_scope(&self) -> bool {
        self.in_scope
    }

    pub fn scope_open(&self) -> bool {
        self.scope_open
    }

    /// The field the keyboard types in.
    pub fn typing(&mut self) -> &mut TextInput {
        if self.in_scope {
            &mut self.scope
        } else if self.in_mask {
            &mut self.mask
        } else if self.in_replace {
            &mut self.replace
        } else {
            &mut self.input.input
        }
    }

    /// The folders the one typed goes on to (the typed one itself left out): the shared
    /// list opened under the folder's field as its suggestions (the field keeps the typing,
    /// the first one lit: ↵ takes it), closed when there are none.
    pub fn set_suggestions(&mut self, folders: Vec<String>) {
        let typed = self.scope.text().trim().trim_matches('/').to_owned();
        self.suggestions = folders.into_iter().filter(|f| *f != typed).collect();
        if self.suggestions.is_empty() {
            self.suggestion_list.close();
            return;
        }
        let items = self
            .suggestions
            .iter()
            .map(|folder| PickerItem {
                label: folder.clone(),
                icon: Some("folder"),
                ..PickerItem::default()
            })
            .collect();
        self.suggestion_list.open_suggestions(items, None, true);
    }

    #[cfg(test)]
    pub fn suggestion_at(&self, index: usize) -> Option<&str> {
        self.suggestions.get(index).map(String::as_str)
    }

    pub fn has_suggestions(&self) -> bool {
        self.in_scope && self.suggestion_list.is_open()
    }

    pub fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.suggestion_list.close();
    }

    /// Lights the suggestion `delta` rows away (as the list's arrows).
    pub fn move_suggestion(&mut self, delta: isize) {
        let key = if delta < 0 { ViewerKey::Up } else { ViewerKey::Down };
        self.suggestion_list.key(key, KeyReach::default());
    }

    /// The suggestion lit (`None`: the one at `index`) is taken: the folder's field holds it
    /// and a `/`, the caret after it, to go on into it.
    pub fn take_suggestion(&mut self, index: Option<usize>) -> bool {
        let Some(at) = index.or_else(|| self.suggestion_list.highlighted()) else {
            return false;
        };
        let Some(folder) = self.suggestions.get(at) else { return false };
        self.scope.set_text(&format!("{folder}/"));
        self.scope.end(false);
        true
    }

    /// `p` is on the folders suggested (their list, over the results).
    pub fn on_suggestions(&self, p: Point) -> bool {
        self.has_suggestions() && self.suggestion_list.contains(p)
    }

    /// A press on the folders suggested: its scrollbar's thumb held, or the folder chosen
    /// (the index taken).
    pub fn press_suggestions(&mut self, p: Point, clicks: u32) -> (Press, Option<usize>) {
        let press = Control::press(&mut self.suggestion_list, p, clicks);
        match self.suggestion_list.take_event() {
            Some(PickerEvent::Chosen(i)) => (press, Some(i)),
            _ => (press, None),
        }
    }

    /// The mouse moved to `p` (kept for the wheel): the suggestion under it lit. `true` if
    /// something changed.
    pub fn hover_suggestions(&mut self, p: Point) -> bool {
        self.pointer = p;
        self.has_suggestions() && self.suggestion_list.hover(p)
    }

    /// The wheel over the folders suggested (where the mouse was last): their rows scrolled.
    /// `None`: the mouse isn't over them.
    pub fn scroll_suggestions(&mut self, dy: f32) -> Option<bool> {
        let pointer = self.pointer;
        self.on_suggestions(pointer).then(|| self.suggestion_list.scroll(dy))
    }

    /// The suggestions' thumb dragged to `p`; `true` if their rows moved.
    pub fn drag_suggestions(&mut self, p: Point) -> bool {
        self.suggestion_list.dragging() && self.suggestion_list.drag(p)
    }

    /// The mouse let go: the suggestions' thumb too. `true` if it was held.
    pub fn release_suggestions(&mut self) -> bool {
        self.suggestion_list.release()
    }

    /// The cursor over the folders suggested (`None`: not over them).
    pub fn suggestions_cursor(&self, p: Point) -> Option<ViewerCursor> {
        if !self.on_suggestions(p) {
            return None;
        }
        self.suggestion_list.cursor_at(p)
    }

    /// When the suggestions' scrollbar next fades.
    pub fn suggestions_deadline(&self) -> Option<std::time::Instant> {
        self.has_suggestions().then(|| self.suggestion_list.next_frame()).flatten()
    }

    /// Where the folders suggested were drawn last, all of it and their rows (the tests).
    #[cfg(test)]
    pub(crate) fn suggestions_area(&self) -> Option<(Rect, Rect)> {
        self.has_suggestions()
            .then(|| {
                Control::area(&self.suggestion_list).map(|a| (a, self.suggestion_list.list_area()))
            })
            .flatten()
    }

    /// Where the folder's field was drawn last (the tests).
    #[cfg(test)]
    pub(crate) fn scope_field(&self) -> Rect {
        self.scope_field
    }

    /// Where the panel was drawn last (the tests).
    #[cfg(test)]
    pub(crate) fn panel_area(&self) -> Rect {
        self.panel
    }

    /// The caret where the field at `p` was clicked; `true` when it was the search's ×
    /// (the field emptied: it's searched again).
    pub fn place_caret(&mut self, p: Point) -> bool {
        if self.query_field.contains(p) {
            Control::press(&mut self.input, p, 1);
            // A press changes the text only by ×: told by both, once.
            return self.input.take_cleared() | self.input.take_edited();
        }
        if self.mask_field.contains(p) {
            self.mask.click(p.x);
        } else if self.scope_field.contains(p) {
            self.scope.click(p.x);
        } else if self.replace_field.contains(p) {
            self.replace.click(p.x);
        }
        false
    }

    /// One of the options turned on or off.
    pub fn toggle_option(&mut self, option: SearchOption) {
        let o = &mut self.options;
        match option {
            SearchOption::Case => o.case = !o.case,
            SearchOption::Word => o.word = !o.word,
            SearchOption::Regex => o.regex = !o.regex,
        }
    }

    /// What's typed isn't a pattern: why (`None`: it is).
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    #[cfg(test)]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_exact(&mut self, state: Option<ExactState>) {
        self.exact = state;
    }

    /// Lines found by an exact search, after those already listed (the selection stays).
    pub fn append_results(&mut self, results: Vec<Hit>) {
        self.results.extend(results);
    }

    /// The option button at `p` (its tooltip).
    pub fn option_at(&self, p: Point) -> Option<(Rect, SearchOption)> {
        self.option_buttons.iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// The replacement's button, for its tooltip.
    pub fn replace_button(&self) -> Rect {
        self.replace_button
    }

    /// The folder's button, for its tooltip.
    pub fn scope_button(&self) -> Rect {
        self.scope_button
    }
}
