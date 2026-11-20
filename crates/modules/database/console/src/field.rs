//! A modal's one text field as a form's control (a popover's filter, a rename's name): it
//! has the keys the whole time its view is open (the view gives it the focus), the editing
//! keys and the edit commands are its own, the view's (↵, esc, the list's arrows) stay the
//! view's. What it changed is said in `edited`, for the view to follow (a list filtered
//! again).

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

/// A text field as a form's control.
#[derive(Debug)]
pub(crate) struct KeyField<'a> {
    pub input: &'a mut TextInput,
    /// Where it was drawn last.
    pub area: Rect,
    /// A press is held and a drag selects (else a press only puts the caret).
    pub selects: bool,
    /// Set when its text changed (typed, deleted, undone, cut).
    pub edited: &'a mut bool,
}

impl Control for KeyField<'_> {
    fn area(&self) -> Option<Rect> {
        Some(self.area)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        self.input.click(p.x);
        if self.selects { Press::Held } else { Press::Took }
    }

    fn drag(&mut self, p: Point) -> bool {
        self.input.extend_to(p.x);
        true
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// The editing keys; ⌦ with nothing after the caret (and nothing selected) isn't used,
    /// so the view may give it a meaning of its own.
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let KeyReach { word, extend } = reach;
        match key {
            ViewerKey::Left => self.input.move_left(word, extend),
            ViewerKey::Right => self.input.move_right(word, extend),
            ViewerKey::Home => self.input.home(extend),
            ViewerKey::End => self.input.end(extend),
            ViewerKey::Backspace => {
                self.input.backspace(word);
                *self.edited = true;
            }
            ViewerKey::Delete => {
                let at_end = self.input.cursor() >= TextInput::text(self.input).len();
                if at_end && TextInput::selected_text(self.input).is_empty() {
                    return false;
                }
                self.input.delete();
                *self.edited = true;
            }
            _ => return false,
        }
        true
    }

    /// What's typed, without control characters.
    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
        *self.edited = true;
        true
    }

    fn selected_text(&self) -> Option<String> {
        Some(TextInput::selected_text(self.input).to_owned()).filter(|s| !s.is_empty())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => {
                self.input.select_all();
                true
            }
            "undo" | "redo" => {
                let done = if id == "redo" { self.input.redo() } else { self.input.undo() };
                *self.edited = true;
                done
            }
            "cut" => {
                let had = !TextInput::selected_text(self.input).is_empty();
                if had {
                    self.input.insert("");
                    *self.edited = true;
                }
                had
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/field.rs"]
mod tests;
