//! The Push… dialog's own part as a form's control: the branches' names to type (one field
//! per row, Tab going from row to row, keeping the row in sight).

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, ViewerCursor, ViewerKey};

use super::form::BranchLine;
use super::view::{Hit, ROWS_SHOWN};

/// The names' column as one control: a press puts the caret in a row's name (two a word,
/// three all of it) and a drag selects there; with the focus the row typed in takes the
/// text, the editing keys and the edit commands, and Tab moves to the next row's (⇧ the
/// previous), the rows scrolled so it's shown. An edit asks again before a force push.
pub(crate) struct NameFields<'a> {
    pub(crate) lines: &'a mut [BranchLine],
    /// Where the form's parts were drawn last (the names' fields among them).
    pub(crate) hits: &'a [(Rect, Hit)],
    /// The row typed in.
    pub(crate) typing: &'a mut Option<usize>,
    /// The first row shown.
    pub(crate) first: &'a mut usize,
    pub(crate) confirming: &'a mut bool,
}

impl NameFields<'_> {
    /// The row whose name field is at `p`.
    fn row_at(&self, p: Point) -> Option<usize> {
        match self.hits.iter().rev().find(|(r, _)| r.contains(p))?.1 {
            Hit::Name(i) => Some(i),
            _ => None,
        }
    }

    /// Row `at` typed in, the rows scrolled so it's shown.
    fn type_in(&mut self, at: usize) {
        *self.typing = Some(at);
        if at < *self.first {
            *self.first = at;
        } else if at >= *self.first + ROWS_SHOWN {
            *self.first = at + 1 - ROWS_SHOWN;
        }
    }

    /// The next (or previous) row's name to type.
    fn tab(&mut self, back: bool) {
        let count = self.lines.len();
        if count == 0 {
            return;
        }
        let at =
            self.typing.map_or(0, |i| if back { (i + count - 1) % count } else { (i + 1) % count });
        self.type_in(at);
    }

    fn input(&mut self) -> Option<&mut ui_kit::TextInput> {
        Some(&mut self.lines.get_mut((*self.typing)?)?.remote_name)
    }
}

impl Control for NameFields<'_> {
    fn hit(&self, p: Point) -> bool {
        self.row_at(p).is_some()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let Some(row) = self.row_at(p) else { return Press::Ignored };
        let Some(line) = self.lines.get_mut(row) else { return Press::Ignored };
        line.remote_name.press(p.x, clicks);
        *self.typing = Some(row);
        *self.confirming = false;
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.input().is_some_and(|input| {
            input.extend_to(p.x);
            true
        })
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Given by Tab, it starts at the first row; taken away, no row is typed in.
    fn set_focus(&mut self, focused: bool) {
        match (focused, *self.typing) {
            (true, None) if !self.lines.is_empty() => self.type_in(0),
            (false, _) => *self.typing = None,
            _ => {}
        }
    }

    /// Tab goes from row to row, never out of the names.
    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if key == ViewerKey::Tab {
            self.tab(reach.extend);
            return true;
        }
        let Some(input) = self.input() else { return false };
        match key {
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            _ => return false,
        }
        *self.confirming = false;
        true
    }

    /// What's typed, its control characters dropped.
    fn text(&mut self, typed: &str) -> bool {
        *self.confirming = false;
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input().is_some_and(|input| {
            input.insert(&typed);
            true
        })
    }

    fn selected_text(&self) -> Option<String> {
        let selected = self.lines.get((*self.typing)?)?.remote_name.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        let Some(input) = self.input() else { return false };
        match id {
            "select_all" => input.select_all(),
            "undo" => return input.undo(),
            "redo" => return input.redo(),
            "cut" => input.insert(""),
            _ => return false,
        }
        true
    }
}
