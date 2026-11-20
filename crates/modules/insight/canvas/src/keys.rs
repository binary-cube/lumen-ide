//! What takes the canvas's keys, as its controls (see [`Canvas::with_controls`]): the search
//! while it's open, else the declaration's code being edited. The search: what's typed and ⌫
//! change the query (all of it once ⌘A chose it), ← → Home End only drop that choice, ↑ ↓
//! and Tab move among the results, ↵ asks for the chosen one, Esc closes it. The code: its
//! own keys (Esc ends the edit). What they ask of the canvas is written to the cells given;
//! the canvas does it after.

use code_intelligence::CatalogEntry;
use ui_components::ViewerKey;
use ui_components::control::Control;
use ui_components::tool_panel::KeyReach;

use crate::{edit, search};

/// The edit command choosing all of the query.
const SELECT_ALL: &str = "select_all";

/// The search while it's open, over the project's catalog.
#[derive(Debug)]
pub(crate) struct SearchPart<'a> {
    pub search: &'a mut Option<search::Search>,
    pub catalog: &'a [CatalogEntry],
    /// ↵: the result chosen (its place in the catalog, `None` with none), to jump to.
    pub chosen: &'a mut Option<Option<usize>>,
}

impl Control for SearchPart<'_> {
    fn focusable(&self) -> bool {
        self.search.is_some()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        let Some(search) = self.search.as_mut() else { return false };
        match key {
            ViewerKey::Escape => *self.search = None,
            ViewerKey::Backspace => search.backspace(self.catalog),
            ViewerKey::Left | ViewerKey::Right | ViewerKey::Home | ViewerKey::End => {
                search.all = false;
            }
            ViewerKey::Up => search.step(-1),
            ViewerKey::Down | ViewerKey::Tab => search.step(1),
            ViewerKey::Enter => {
                *self.chosen = Some(search.results.get(search.selected).copied());
            }
            _ => return false,
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some(search) = self.search.as_mut() else { return false };
        search.insert(typed, self.catalog);
        true
    }

    fn command(&mut self, id: &str) -> bool {
        let Some(search) = self.search.as_mut().filter(|_| id == SELECT_ALL) else {
            return false;
        };
        search.all = !search.query.is_empty();
        true
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
