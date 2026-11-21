//! The paths' search above the list as a form's control: the app's search field (a press
//! puts the caret, a drag selects, × empties it with the keys kept; the editing keys, the
//! text typed, ⌘A / ⌘X / ⌘Z), with the view's own rules before it: Esc empties it, then
//! gives the keys back, and whatever changes the text tells the list to start from its top.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_components::{KeyReach, ViewerCursor, ViewerKey};

pub(super) struct SearchPart<'a> {
    pub field: &'a mut SearchField,
    /// What's searched for changed: the list from its top.
    pub edited: &'a mut bool,
    /// Esc on an empty search: the keys go back.
    pub left: &'a mut bool,
}

impl SearchPart<'_> {
    /// `edit` done, `edited` told when it changed the text.
    fn editing(&mut self, edit: impl FnOnce(&mut SearchField) -> bool) -> bool {
        let done = edit(self.field);
        *self.edited |= self.field.take_edited();
        done
    }
}

impl Control for SearchPart<'_> {
    fn area(&self) -> Option<Rect> {
        Control::area(self.field)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let pressed = Control::press(self.field, p, clicks);
        *self.edited |= self.field.take_cleared() | self.field.take_edited();
        pressed
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if key == ViewerKey::Escape {
            if SearchField::text(self.field).is_empty() {
                *self.left = true;
            } else {
                self.field.set_text("");
                *self.edited = true;
            }
            return true;
        }
        self.editing(|field| Control::key(field, key, reach))
    }

    fn text(&mut self, typed: &str) -> bool {
        self.editing(|field| Control::text(field, typed))
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(self.field)
    }

    fn command(&mut self, id: &str) -> bool {
        self.editing(|field| Control::command(field, id))
    }
}
