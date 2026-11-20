//! The paths' search above the list as a form's control: a press puts the caret (one click,
//! no drag selection) or, on its ×, empties it (the keys kept); with the focus it takes what's
//! typed, its moving and erasing keys and the edit commands. Esc empties it, then gives the
//! keys back.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

pub(super) struct SearchField<'a> {
    pub query: &'a mut TextInput,
    /// Where it was drawn, and its × (while there's text).
    pub field: Rect,
    pub clear: Option<Rect>,
    /// What's searched for changed: the list from its top.
    pub edited: &'a mut bool,
    /// Esc on an empty search: the keys go back.
    pub left: &'a mut bool,
}

impl SearchField<'_> {
    fn on_clear(&self, p: Point) -> bool {
        self.clear.is_some_and(|r| r.contains(p))
    }

    /// `edit` done, `edited` told when it changed the text.
    fn editing(&mut self, edit: impl FnOnce(&mut TextInput) -> bool) -> bool {
        let before = TextInput::text(self.query).to_owned();
        let done = edit(self.query);
        *self.edited |= TextInput::text(self.query) != before;
        done
    }
}

impl Control for SearchField<'_> {
    fn hit(&self, p: Point) -> bool {
        self.field.contains(p) || self.on_clear(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        if self.on_clear(p) {
            *self.query = TextInput::default();
            *self.edited = true;
        } else {
            TextInput::press(self.query, p.x, 1);
        }
        Press::Took
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Some(if self.on_clear(p) { ViewerCursor::Pointer } else { ViewerCursor::Text })
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if key == ViewerKey::Escape && TextInput::text(self.query).is_empty() {
            *self.left = true;
            return true;
        }
        self.editing(|query| {
            match key {
                ViewerKey::Escape => *query = TextInput::default(),
                ViewerKey::Left => query.move_left(reach.word, reach.extend),
                ViewerKey::Right => query.move_right(reach.word, reach.extend),
                ViewerKey::Home => query.home(reach.extend),
                ViewerKey::End => query.end(reach.extend),
                ViewerKey::Backspace => query.backspace(reach.word),
                ViewerKey::Delete => query.delete(),
                _ => return false,
            }
            true
        })
    }

    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.query.insert(&typed);
        *self.edited = true;
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = TextInput::selected_text(self.query);
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        self.editing(|query| match id {
            "select_all" => {
                query.select_all();
                true
            }
            "undo" => query.undo(),
            "redo" => query.redo(),
            "cut" if !TextInput::selected_text(query).is_empty() => {
                query.insert("");
                true
            }
            _ => false,
        })
    }
}
