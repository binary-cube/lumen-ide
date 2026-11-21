//! What takes the canvas's keys, as its controls (see [`Canvas::with_controls`]): the search
//! while it's open, else the declaration's code being edited. The search is the app's search
//! field (a press puts the caret, a drag selects, the editing keys, ⌘A / ⌘X / ⌘Z, × empties
//! it), what changes its text finding again; ↑ ↓ and Tab move among the results, ↵ asks for
//! the chosen one, Esc closes it. The code: its own keys (Esc ends the edit). What they ask
//! of the canvas is written to the cells given; the canvas does it after.

use code_intelligence::CatalogEntry;
use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::tool_panel::KeyReach;
use ui_components::{ViewerCursor, ViewerKey};

use crate::{edit, search};

/// The edit command choosing all of the query (it changes nothing found).
const SELECT_ALL: &str = "select_all";

/// The search while it's open, over the project's catalog.
#[derive(Debug)]
pub(crate) struct SearchPart<'a> {
    pub search: &'a mut Option<search::Search>,
    pub catalog: &'a [CatalogEntry],
    /// ↵: the result chosen (its place in the catalog, `None` with none), to jump to.
    pub chosen: &'a mut Option<Option<usize>>,
}

impl SearchPart<'_> {
    /// `edit` on the open search's field; used, and `changes` its text, found again.
    fn edited(
        &mut self,
        changes: bool,
        edit: impl FnOnce(&mut ui_components::search_field::SearchField) -> bool,
    ) -> bool {
        let Some(search) = self.search.as_mut() else { return false };
        let used = edit(&mut search.field);
        if used && changes {
            search.refresh(self.catalog);
        }
        used
    }
}

impl Control for SearchPart<'_> {
    fn area(&self) -> Option<Rect> {
        self.search.as_ref().and_then(|s| Control::area(&s.field))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let Some(search) = self.search.as_mut() else { return Press::Ignored };
        let press = Control::press(&mut search.field, p, clicks);
        // × emptied it: nothing found.
        if search.field.take_cleared() {
            search.refresh(self.catalog);
        }
        press
    }

    fn drag(&mut self, p: Point) -> bool {
        self.search.as_mut().is_some_and(|s| Control::drag(&mut s.field, p))
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.search.as_mut().is_some_and(|s| Control::hover(&mut s.field, p))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.search.as_ref().and_then(|s| Control::cursor(&s.field, p))
    }

    fn focusable(&self) -> bool {
        self.search.is_some()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(search) = self.search.as_mut() else { return false };
        match key {
            ViewerKey::Escape => *self.search = None,
            ViewerKey::Up => search.step(-1),
            ViewerKey::Down | ViewerKey::Tab => search.step(1),
            ViewerKey::Enter => {
                *self.chosen = Some(search.results.get(search.selected).copied());
            }
            _ => {
                let erases = matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
                return self.edited(erases, |f| Control::key(f, key, reach));
            }
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.edited(true, |f| Control::text(f, typed))
    }

    fn selected_text(&self) -> Option<String> {
        self.search.as_ref().and_then(|s| Control::selected_text(&s.field))
    }

    fn command(&mut self, id: &str) -> bool {
        self.edited(id != SELECT_ALL, |f| Control::command(f, id))
    }
}

/// The declaration's code being edited.
#[derive(Debug)]
pub(crate) struct EditPart<'a> {
    pub editing: &'a mut Option<edit::CodeEdit>,
    /// Its text or caret changed: the cards are laid out again.
    pub relaid: &'a mut bool,
    /// Esc: the edit ends (what changed goes to the app).
    pub finished: &'a mut bool,
}

impl Control for EditPart<'_> {
    fn focusable(&self) -> bool {
        self.editing.is_some()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        let Some(edit) = self.editing.as_mut() else { return false };
        if key == ViewerKey::Escape {
            *self.finished = true;
            return true;
        }
        *self.relaid = true;
        edit.key(key)
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some(edit) = self.editing.as_mut() else { return false };
        edit.insert(typed);
        *self.relaid = true;
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/keys.rs"]
mod tests;
