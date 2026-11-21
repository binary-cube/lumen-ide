//! The filter field as one of the view's parts: the app's search field
//! ([`SearchField`]: a press puts the caret, a drag selects, × empties it), lent with the
//! view's rules. With the focus it takes the text typed and pasted (on one line, the field's
//! own way), its editing keys (← → Home End, Backspace, Delete, by word and extending) and
//! the edit commands (⌘A, ⌘X, ⌘Z, ⌘⇧Z); what it has selected is the copy's. It is focused
//! by the view's own ring (the toolbar's stop it is) or a press on it, never by Tab among
//! the parts; it is hit only while drawn this frame with no popover over it, and never on
//! the view's own × closing a narrow page's empty filter.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_components::{KeyReach, ViewerCursor, ViewerKey};

/// `typed` has something to type (not only control characters: a lone ↵ isn't text).
pub(super) fn types(typed: &str) -> bool {
    typed.chars().any(|c| !c.is_control())
}

/// The filter's field, lent to the view's parts.
#[derive(Debug)]
pub(super) struct FilterField<'a> {
    pub field: &'a mut SearchField,
    /// Drawn this frame, nothing over it: a press on it is its own.
    pub shown: bool,
    /// The view's own × on it (a narrow page's filter opened empty): never the field's.
    pub close: Option<Rect>,
}

impl Control for FilterField<'_> {
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.field).filter(|_| self.shown)
    }

    fn hit(&self, p: Point) -> bool {
        self.shown && self.field.hit(p) && !self.close.is_some_and(|r| r.contains(p))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        Control::press(&mut *self.field, p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut *self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(&mut *self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// The view's ring stops at it, in the order it draws its controls.
    fn in_tab_order(&self) -> bool {
        false
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        Control::key(&mut *self.field, key, reach)
    }

    /// On one line, the field's own way (its breaks as spaces); nothing to type isn't taken.
    fn text(&mut self, typed: &str) -> bool {
        types(typed) && Control::text(&mut *self.field, typed)
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field)
    }

    fn command(&mut self, id: &str) -> bool {
        Control::command(&mut *self.field, id)
    }
}
