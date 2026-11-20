//! A preview's one-line field (the new name, the destination) as a form's control, with the
//! behavior these fields have: a press puts the caret there (no word nor line chosen, no
//! drag selecting), the keys move and erase one character at a time (never by word, never
//! extending a selection), what's typed loses its spaces (a name, a path) and `select_all`
//! chooses it whole. Tab isn't its own and doesn't move the focus (the page has no other
//! field). What reached it is told to the view by `edited`, which follows it (the plan, the
//! clashes).

use kernel::{Point, Rect};
use ui_components::ViewerKey;
use ui_components::control::{Control, Press};
use ui_components::tool_panel::KeyReach;
use ui_kit::TextInput;

/// The edit command choosing the whole field.
const SELECT_ALL: &str = "select_all";

/// The field, where it was drawn, and the flag the view follows.
#[derive(Debug)]
pub struct NameField<'a> {
    pub input: &'a mut TextInput,
    /// Where it was drawn last (`None`: not drawn yet).
    pub area: Option<Rect>,
    /// Set when a key or a text reached it.
    pub edited: &'a mut bool,
}

impl Control for NameField<'_> {
    fn area(&self) -> Option<Rect> {
        self.area
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        self.input.click(p.x);
        Press::Took
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        match key {
            ViewerKey::Backspace => self.input.backspace(false),
            ViewerKey::Delete => self.input.delete(),
            ViewerKey::Left => self.input.move_left(false, false),
            ViewerKey::Right => self.input.move_right(false, false),
            ViewerKey::Home => self.input.home(false),
            ViewerKey::End => self.input.end(false),
            _ => return false,
        }
        *self.edited = true;
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
        self.input.insert(&typed);
        *self.edited = true;
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id != SELECT_ALL {
            return false;
        }
        self.input.select_all();
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/name_field.rs"]
mod tests;
