//! The diagram's filter as a form's control: the app's search field (its press, drag,
//! editing keys, ⌘A / ⌘X / ⌘Z, × emptying it), with the view's own two keys before it: Esc
//! empties it and then gives the keys back, ↵ gives them back too. What it changed, and
//! that it was left, are said back to the view.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_components::{KeyReach, ViewerCursor, ViewerKey};

/// The filter, as a form's control.
#[derive(Debug)]
pub(super) struct FilterField<'a> {
    pub field: &'a mut SearchField,
    /// It was drawn by the last paint (only what's drawn can be pressed).
    pub shown: bool,
    /// Set when its text changed.
    pub edited: &'a mut bool,
    /// Set when it gives the keys back: `true` with ↵ (the first table found centered).
    pub left: &'a mut Option<bool>,
}

impl FilterField<'_> {
    /// Runs `edit`, noting whether the text changed.
    fn editing(&mut self, edit: impl FnOnce(&mut SearchField) -> bool) -> bool {
        let used = edit(&mut *self.field);
        *self.edited |= self.field.take_edited();
        used
    }
}

impl Control for FilterField<'_> {
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.field).filter(|_| self.shown)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let press = Control::press(self.field, p, clicks);
        // Its × emptied it: what's shown follows.
        *self.edited |= self.field.take_cleared() | self.field.take_edited();
        press
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(self.field, p.filter(|_| self.shown))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Tab isn't a move to another part (the view has no other field): left to its key.
    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Escape if !SearchField::text(self.field).is_empty() => {
                self.field.set_text("");
                *self.edited = true;
            }
            ViewerKey::Escape => *self.left = Some(false),
            ViewerKey::Enter => *self.left = Some(true),
            _ => return self.editing(|f| Control::key(f, key, reach)),
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.editing(|f| Control::text(f, typed))
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field)
    }

    fn command(&mut self, id: &str) -> bool {
        self.editing(|f| Control::command(f, id))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view/filter.rs"]
mod tests;
