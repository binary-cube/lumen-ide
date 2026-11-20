//! The diagram's filter as a form's control: its pill (but its ×) pressed puts the caret
//! (two clicks a word, three all of it) and a drag selects; with the focus it takes the
//! text and its editing keys, Esc empties it and then gives the keys back, ↵ gives them
//! back too. What it changed, and that it was left, are said back to the view.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

/// The filter, as a form's control.
#[derive(Debug)]
pub(super) struct FilterField<'a> {
    pub input: &'a mut TextInput,
    /// Its pill as drawn last.
    pub area: Rect,
    /// Its × (emptying it): not the field's.
    pub clear: Rect,
    /// Set when its text changed.
    pub edited: &'a mut bool,
    /// Set when it gives the keys back: `true` with ↵ (the first table found centered).
    pub left: &'a mut Option<bool>,
}

impl Control for FilterField<'_> {
    fn area(&self) -> Option<Rect> {
        Some(self.area)
    }

    fn hit(&self, p: Point) -> bool {
        self.area.contains(p) && !self.clear.contains(p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.input.press(p.x, clicks);
        Press::Held
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

    /// Tab isn't a move to another part (the view has no other field): left to its key.
    fn keeps_tab(&self) -> bool {
        true
    }

    /// Its keys move and delete a character at a time (no word, no selection by ⇧).
    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        let before = TextInput::text(self.input).to_owned();
        match key {
            ViewerKey::Backspace => self.input.backspace(false),
            ViewerKey::Delete => self.input.delete(),
            ViewerKey::Left => self.input.move_left(false, false),
            ViewerKey::Right => self.input.move_right(false, false),
            ViewerKey::Home => self.input.home(false),
            ViewerKey::End => self.input.end(false),
            ViewerKey::Escape if !before.is_empty() => self.input.set_text(""),
            ViewerKey::Escape => *self.left = Some(false),
            ViewerKey::Enter => *self.left = Some(true),
            _ => return false,
        }
        *self.edited |= TextInput::text(self.input) != before;
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.input.insert(typed);
        *self.edited = true;
        true
    }

    fn selected_text(&self) -> Option<String> {
        Some(TextInput::selected_text(self.input).to_owned()).filter(|s| !s.is_empty())
    }

    fn command(&mut self, id: &str) -> bool {
        let edited = match id {
            "select_all" => {
                self.input.select_all();
                return true;
            }
            "undo" => self.input.undo(),
            "redo" => self.input.redo(),
            "cut" => {
                self.input.insert("");
                true
            }
            _ => false,
        };
        *self.edited |= edited;
        edited
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view/filter.rs"]
mod tests;
