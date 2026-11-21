//! The modal's text fields as its controls: one kind of part for every field the editor
//! has (made per driver), each knowing the field it is, so the view maps the place of the
//! one that answered back to its field. They're pressed where their field was drawn last
//! (and nothing is over it there); the focus moves among them by the view's own keys (Tab,
//! ↑ ↓, the filter's row), never the parts' order.
//!
//! The filter is the app's search field (its press, drag, editing keys, ⌘A / ⌘X / ⌘Z, ×
//! emptying it). The others' rules differ from a plain field's: a press only puts the caret (a second one
//! selects no word), a newline typed is dropped (never made a space), a cut with nothing
//! selected isn't used, and the password's text is never copied nor cut.

use std::borrow::Borrow;

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

use super::Hit;
use crate::sources::form::Field;

/// A field of the modal as a control: which one, its text (lent: `&mut` to be edited,
/// `&` to be read), and the window's hits (where the fields were drawn, what's over them).
pub(in crate::sources) struct FieldPart<'a, I> {
    pub field: Field,
    pub input: I,
    pub hits: &'a [(Rect, Hit)],
}

/// `p` is on `field` as drawn last: the topmost hit there is it.
fn on(hits: &[(Rect, Hit)], field: &Field, p: Point) -> bool {
    hits.iter()
        .rev()
        .find(|(r, _)| r.contains(p))
        .is_some_and(|(_, hit)| matches!(hit, Hit::Field(f) if f == field))
}

/// The list's filter as a control, the window's hits with it (what's over it).
pub(in crate::sources) struct FilterPart<'a> {
    pub search: &'a mut SearchField,
    pub hits: &'a [(Rect, Hit)],
}

impl Control for FilterPart<'_> {
    fn hit(&self, p: Point) -> bool {
        on(self.hits, &Field::Filter, p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        Control::press(self.search, p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(self.search, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.search, p)
    }

    /// The view moves the focus among its fields by its own keys.
    fn in_tab_order(&self) -> bool {
        false
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        Control::key(self.search, key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        Control::text(self.search, typed)
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.search)
    }

    fn command(&mut self, id: &str) -> bool {
        Control::command(self.search, id)
    }
}

impl<I: Borrow<TextInput>> FieldPart<'_, I> {
    /// `p` is on its field as drawn last: the topmost hit there is it.
    fn on(&self, p: Point) -> bool {
        on(self.hits, &self.field, p)
    }

    /// What it has selected; never the password's.
    fn selection(&self) -> Option<String> {
        if self.field == Field::Password {
            return None;
        }
        let selected = self.input.borrow().selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }
}

/// A field lent to be read (what it has selected, asked from the view's `&self`).
impl Control for FieldPart<'_, &TextInput> {
    fn hit(&self, p: Point) -> bool {
        self.on(p)
    }

    fn in_tab_order(&self) -> bool {
        false
    }

    fn focusable(&self) -> bool {
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.selection()
    }
}

/// A field lent to be edited.
impl Control for FieldPart<'_, &mut TextInput> {
    fn hit(&self, p: Point) -> bool {
        self.on(p)
    }

    /// The caret placed, whatever the series: a second click selects no word.
    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        self.input.click(p.x);
        Press::Held
    }

    /// The selection follows the pointer while it's over the field.
    fn drag(&mut self, p: Point) -> bool {
        if !self.input.contains(p) {
            return false;
        }
        self.input.extend_to(p.x);
        true
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    /// The view moves the focus among its fields by its own keys.
    fn in_tab_order(&self) -> bool {
        false
    }

    fn focusable(&self) -> bool {
        true
    }

    /// The editing keys, as any field's (the caret, the selection, what's deleted).
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        Control::key(&mut *self.input, key, reach)
    }

    /// The text typed, its control characters dropped (a newline too: never a space).
    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.selection()
    }

    /// Select all, undo, redo; a cut only of what's selected (nothing selected: not used),
    /// never the password's.
    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => {
                self.input.select_all();
                true
            }
            "undo" => self.input.undo(),
            "redo" => self.input.redo(),
            "cut" if self.field != Field::Password => {
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
