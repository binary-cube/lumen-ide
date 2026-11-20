//! The field to write in, as one of the conversation's controls: a press puts the caret
//! (no drag selecting), the editing keys reach a character or a word and extend the
//! selection with ⇧, what's typed loses its control characters, ⌘A / undo / redo / cut (with
//! something selected) edit it; a paste keeps one line. ↵ and Esc aren't its own (the view sends, stops); nor is
//! Tab. It's hit where no button over it is.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

/// The field, where it was drawn, and the buttons drawn over it.
#[derive(Debug)]
pub(crate) struct ChatField<'a, H> {
    pub input: &'a mut TextInput,
    pub area: Rect,
    pub buttons: &'a [(Rect, H)],
}

impl<H> Control for ChatField<'_, H> {
    fn hit(&self, p: Point) -> bool {
        self.area.contains(p) && !self.buttons.iter().any(|(r, _)| r.contains(p))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.input.press(p.x, clicks);
        Press::Took
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter | ViewerKey::Escape => false,
            _ => reach.edit(self.input, key),
        }
    }

    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
        true
    }

    /// Pasted on one line: its breaks become spaces (other characters kept as pasted).
    fn paste(&mut self, text: &str) -> bool {
        self.input.insert(&text.replace(['\r', '\n'], " "));
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = self.input.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => {
                self.input.select_all();
                true
            }
            "undo" => self.input.undo(),
            "redo" => self.input.redo(),
            "cut" if !self.input.selected_text().is_empty() => {
                self.input.insert("");
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/field.rs"]
mod tests;
