//! The panel's parts as a form's controls (see `SessionsPanel::with_controls`), each with
//! the behavior it has here: the agents' list (over everything while open, its keys too; a
//! press is one click, never a selection dragged), the field renaming a row and the search
//! (a press puts the caret, the editing keys reach a character or a word, ↵ and Esc are the
//! panel's to act on), a session pressed and dragged toward a folder (held from the panel's
//! own press: what it's dropped on is the panel's).
//! What a part asks of the panel (a list's row chosen, a name given, the search changed) is
//! written to the cell it's given; the panel does it after.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::picker::{Picker, PickerEvent};
use ui_components::{KeyReach, ViewerKey};
use ui_kit::TextInput;

/// The edit commands a field takes.
const SELECT_ALL: &str = "select_all";
const CUT: &str = "cut";
const UNDO: &str = "undo";
const REDO: &str = "redo";

/// The agents' list under its field, as the panel uses it: hit anywhere while open (a press
/// outside closes it), one click a press, the keys its own while open; what's typed isn't
/// (it goes on to the field with the keys), nor are the edit commands.
pub(super) struct AgentsList<'a> {
    pub list: &'a mut Picker,
    pub event: &'a mut Option<PickerEvent>,
}

impl std::fmt::Debug for AgentsList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentsList").field("open", &self.list.is_open()).finish()
    }
}

impl Control for AgentsList<'_> {
    fn hit(&self, _p: Point) -> bool {
        self.list.is_open()
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        *self.event = self.list.press(p);
        Press::Took
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        p.is_some_and(|p| self.list.hover(p))
    }

    fn captures_keys(&self) -> bool {
        self.list.is_open()
    }

    /// Every key while open (↑ ↓ ↵ move and choose, Esc closes it).
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        *self.event = self.list.key(key, reach);
        true
    }

    /// What's typed goes to its search when it shows one (many agents); otherwise on to the
    /// field under it.
    fn text(&mut self, typed: &str) -> bool {
        self.list.searches() && self.list.text_input(typed)
    }

    fn paste(&mut self, text: &str) -> bool {
        self.list.searches() && self.list.paste(text)
    }

    fn selected_text(&self) -> Option<String> {
        self.list.selected_text()
    }

    /// ⌘A and the like for its search, when it has one.
    fn command(&mut self, id: &str) -> bool {
        self.list.searches() && self.list.command(id)
    }

    fn cursor(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        Control::cursor(&*self.list, p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.list.scroll(dy)
    }

    /// While open the wheel is its own (nothing under it scrolls).
    fn takes_wheel(&self, _p: Point) -> bool {
        self.list.is_open()
    }
}

/// What a field's key or command asks of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldAsk {
    /// ↵: the name given, the first session found taken up again.
    Enter,
    /// Esc: the name left as it was; the search, empty, lets go of the keys.
    Escape,
    /// The search's text changed (its results from the top).
    Searched,
}

/// A row's field renaming it, or the search: hit in `area` (the search's × aside); a press
/// puts the caret; the editing keys reach a character or a word; ↵ and Esc ask the panel
/// (the search, not empty, is emptied by Esc); ⌘A, ⌘X, ⌘Z, ⇧⌘Z edit it (always used).
/// Tab goes through it (no other part has the keys) and isn't used.
#[derive(Debug)]
pub(super) struct FieldPart<'a> {
    pub input: &'a mut TextInput,
    pub area: Rect,
    /// What a press on it leaves to others (the search's ×).
    pub aside: Option<Rect>,
    /// The search (Esc empties it first, its changes are said), or a row's name.
    pub search: bool,
    pub asked: &'a mut Option<FieldAsk>,
}

impl FieldPart<'_> {
    /// The text changed by what was just done: said, for the search.
    fn said_if_changed(&mut self, before: &str) {
        if self.search && TextInput::text(self.input) != before {
            *self.asked = Some(FieldAsk::Searched);
        }
    }
}

impl Control for FieldPart<'_> {
    fn hit(&self, p: Point) -> bool {
        self.area.contains(p) && !self.aside.is_some_and(|r| r.contains(p))
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        TextInput::press(self.input, p.x, 1);
        Press::Took
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn in_tab_order(&self) -> bool {
        false
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let before = TextInput::text(self.input).to_owned();
        match key {
            ViewerKey::Enter => *self.asked = Some(FieldAsk::Enter),
            ViewerKey::Escape if self.search && !before.is_empty() => {
                *self.input = TextInput::default();
            }
            ViewerKey::Escape => *self.asked = Some(FieldAsk::Escape),
            _ => {
                let used = reach.edit(self.input, key);
                self.said_if_changed(&before);
                return used;
            }
        }
        self.said_if_changed(&before);
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
        if self.search {
            *self.asked = Some(FieldAsk::Searched);
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        let edited = match id {
            SELECT_ALL => {
                self.input.select_all();
                false
            }
            CUT => {
                self.input.insert("");
                true
            }
            UNDO => self.input.undo(),
            REDO => self.input.redo(),
            _ => return false,
        };
        if edited && self.search {
            *self.asked = Some(FieldAsk::Searched);
        }
        true
    }
}

/// A session pressed in the list (its index, where), held from the panel's press: moved
/// far enough, it's dragged (`dragging`); where it would go is the panel's.
#[derive(Debug)]
pub(super) struct RowDrag<'a> {
    pub pressed: Option<(usize, Point)>,
    pub dragging: &'a mut Option<usize>,
    /// How far it moves before it's a drag (pixels).
    pub from: f32,
}

impl Control for RowDrag<'_> {
    /// `true` while it's dragged.
    fn drag(&mut self, p: Point) -> bool {
        let Some((i, at)) = self.pressed else { return false };
        let far = (p.x - at.x).hypot(p.y - at.y) >= self.from;
        if self.dragging.is_none() && !far {
            return false;
        }
        *self.dragging = Some(i);
        true
    }
}
