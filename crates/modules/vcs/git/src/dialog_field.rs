//! A git dialog's one-line field as a form's control (a branch's or a tag's name, a stash's
//! or a tag's message): the pointer puts the caret and selects as in any field; with the
//! focus it takes the text typed without its control characters, the editing keys (↵ and
//! Esc are the dialog's own, Tab moves the focus) and the edit commands, a cut only of what's
//! selected.

use kernel::Point;
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

/// A dialog's field: its input, used through [`Control`].
#[derive(Debug)]
pub(crate) struct DialogField<'a> {
    pub(crate) input: &'a mut TextInput,
}

impl Control for DialogField<'_> {
    fn hit(&self, p: Point) -> bool {
        Control::hit(&*self.input, p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        Control::press(&mut *self.input, p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut *self.input, p)
    }

    fn release(&mut self) -> bool {
        Control::release(&mut *self.input)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.input, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let input = &mut *self.input;
        match key {
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            _ => return false,
        }
        true
    }

    /// What's typed, its control characters dropped (a line break with them).
    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
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
            "cut" => {
                let had = !self.input.selected_text().is_empty();
                if had {
                    self.input.insert("");
                }
                had
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/dialog_field.rs"]
mod tests;
