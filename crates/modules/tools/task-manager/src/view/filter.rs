//! The filter field as one of the view's parts: with the focus it takes the text typed and
//! pasted, its editing keys (← → Home End, Backspace, Delete, by word and extending) and
//! the edit commands (⌘A, ⌘X, ⌘Z, ⌘⇧Z); what it has selected is the copy's. It is
//! focused only by the view's own ring (the toolbar's stop it is), never by Tab among the
//! parts; a press on it is the view's (its magnifier, its count and its × are drawn inside
//! it and answered by the view's hits).

use kernel::Point;
use ui_components::control::Control;
use ui_components::{KeyReach, ViewerKey};
use ui_kit::TextInput;

/// What of `typed` goes into the field: its control characters (line breaks too) dropped;
/// `None` when nothing is left.
pub(super) fn typed_line(typed: &str) -> Option<String> {
    let line: String = typed.chars().filter(|c| !c.is_control()).collect();
    (!line.is_empty()).then_some(line)
}

/// The filter's field, lent to the view's parts.
#[derive(Debug)]
pub(super) struct FilterField<'a> {
    pub input: &'a mut TextInput,
}

impl Control for FilterField<'_> {
    /// Never hit: a press on it goes through the view's hits (see the module's doc).
    fn hit(&self, _p: Point) -> bool {
        false
    }

    fn focusable(&self) -> bool {
        true
    }

    /// The view's ring stops at it, in the order it draws its controls.
    fn in_tab_order(&self) -> bool {
        false
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        Control::key(&mut *self.input, key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some(line) = typed_line(typed) else { return false };
        self.input.insert(&line);
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text(text)
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.input)
    }

    fn command(&mut self, id: &str) -> bool {
        Control::command(&mut *self.input, id)
    }
}
