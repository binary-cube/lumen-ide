//! The wizard's text fields as a form's controls: a press puts the caret (one click, no
//! selection dragged), and the one with the focus takes what's typed (its control
//! characters left out), its moving and erasing keys and Select All — while it's in the
//! step shown. The focus is the wizard's to move (its own keys walk the step's fields, a
//! step given focuses one from code): no field is in the Tab order.

use std::cell::Cell;

use kernel::Point;
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerKey};
use ui_kit::TextInput;

use super::{Field, Hit};

/// One field of the form, with where the step's parts were drawn last.
pub(super) struct FormField<'a> {
    pub(super) field: Field,
    pub(super) input: &'a mut TextInput,
    /// The step's parts as drawn (the field is pressed where it's the topmost: never under
    /// the eye on the secret, nor under the suggestions over it).
    pub(super) hits: &'a [(kernel::Rect, Hit)],
    /// It's one of the step's fields (only then does it take the keys and the text).
    pub(super) in_step: bool,
    /// The field whose text changed (the form follows it): one for all of them.
    pub(super) edited: &'a Cell<Option<Field>>,
}

impl Control for FormField<'_> {
    fn hit(&self, p: Point) -> bool {
        let top = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        top == Some(Hit::Field(self.field))
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        self.input.click(p.x);
        Press::Took
    }

    fn focusable(&self) -> bool {
        true
    }

    fn in_tab_order(&self) -> bool {
        false
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if !self.in_step {
            return false;
        }
        let KeyReach { word, extend } = reach;
        match key {
            ViewerKey::Backspace => self.input.backspace(word),
            ViewerKey::Delete => self.input.delete(),
            ViewerKey::Left => self.input.move_left(word, extend),
            ViewerKey::Right => self.input.move_right(word, extend),
            ViewerKey::Home => self.input.home(extend),
            ViewerKey::End => self.input.end(extend),
            _ => return false,
        }
        if matches!(key, ViewerKey::Backspace | ViewerKey::Delete) {
            self.edited.set(Some(self.field));
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        if !self.in_step {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
        self.edited.set(Some(self.field));
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id != "select_all" || !self.in_step {
            return false;
        }
        self.input.select_all();
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/connect_view/fields.rs"]
mod tests;
