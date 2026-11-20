//! The octal field (`0755`) as a form's control: a press puts the caret and a drag selects
//! (only while the permissions can be edited); with the focus it takes octal digits only,
//! four at most, its moving and erasing keys and Select All. Tab, ↵ and Esc stay the
//! dialog's (Tab gives the field the keys and takes them back).

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

/// The most digits a mode has (`7755`).
const DIGITS: usize = 4;

pub(super) struct OctalField<'a> {
    pub input: &'a mut TextInput,
    /// Where it was drawn last (`None`: not drawn, or not editable there).
    pub area: Option<Rect>,
    /// The permissions can be changed now (a press puts the caret).
    pub editable: bool,
    /// There's a mode to edit (Tab gives the field the keys).
    pub has_mode: bool,
    /// Its text changed: the boxes follow it.
    pub typed: &'a mut bool,
}

impl Control for OctalField<'_> {
    fn area(&self) -> Option<Rect> {
        self.area
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if !self.editable {
            return Press::Ignored;
        }
        TextInput::press(self.input, p.x, clicks);
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
        self.has_mode
    }

    /// Tab is the dialog's: it takes the keys back from the field.
    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Left => self.input.move_left(reach.word, reach.extend),
            ViewerKey::Right => self.input.move_right(reach.word, reach.extend),
            ViewerKey::Home => self.input.home(reach.extend),
            ViewerKey::End => self.input.end(reach.extend),
            ViewerKey::Backspace => {
                self.input.backspace(reach.word);
                *self.typed = true;
            }
            ViewerKey::Delete => {
                self.input.delete();
                *self.typed = true;
            }
            _ => return false,
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        let digits: String = typed.chars().filter(|c| ('0'..='7').contains(c)).collect();
        if digits.is_empty() {
            return true;
        }
        let kept = TextInput::text(self.input).len() - TextInput::selected_text(self.input).len();
        let room = DIGITS.saturating_sub(kept);
        self.input.insert(&digits[..digits.len().min(room)]);
        *self.typed = true;
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = TextInput::selected_text(self.input);
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        if id != "select_all" {
            return false;
        }
        self.input.select_all();
        true
    }
}
