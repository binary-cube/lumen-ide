//! Change Signature's table of fields as one form's control: a row per parameter, a field
//! per column (name, type, default, and for a new parameter the value the calls pass), one
//! of them with the keyboard (row, column). A press puts the caret in the field pressed (no
//! drag selecting); Tab goes to the next field row by row (back to the first after the
//! last), ↑ ↓ to the row above / below in the same column; the other keys move and erase
//! one character at a time; what's typed is its first line. What reached a field is told to
//! the view by `edited`, which takes it into the parameter and rewrites the calls.

use kernel::{Point, Rect};
use refactor::signature::NewParam;
use ui_components::ViewerKey;
use ui_components::control::{Control, Press};
use ui_components::tool_panel::KeyReach;
use ui_kit::TextInput;

/// The table's columns.
pub const NAME: usize = 0;
pub const TYPE: usize = 1;
pub const DEFAULT: usize = 2;
pub const VALUE: usize = 3;

/// The edit command choosing a whole field.
const SELECT_ALL: &str = "select_all";

/// A parameter's row: what it becomes and its fields.
#[derive(Debug, Clone)]
pub struct ParamRow {
    pub new: NewParam,
    pub fields: [TextInput; 4],
}

impl ParamRow {
    pub fn new(new: NewParam) -> Self {
        let p = new.param();
        let value = match &new {
            NewParam::Added { value, .. } => value.clone(),
            NewParam::Kept { .. } => String::new(),
        };
        let field = |text: &str| TextInput::new(text);
        let fields = [field(&p.name), field(&p.type_hint), field(&p.default), field(&value)];
        Self { new, fields }
    }

    pub fn is_added(&self) -> bool {
        matches!(self.new, NewParam::Added { .. })
    }

    /// The fields' text into the parameter.
    pub fn sync(&mut self) {
        let name = self.fields[NAME].text().trim().trim_start_matches('$').to_owned();
        let type_hint = self.fields[TYPE].text().trim().to_owned();
        let default = self.fields[DEFAULT].text().trim().to_owned();
        let typed_value = self.fields[VALUE].text().trim().to_owned();
        let param = self.new.param_mut();
        param.name = name;
        param.type_hint = type_hint;
        param.default = default;
        if let NewParam::Added { value, .. } = &mut self.new {
            *value = typed_value;
        }
    }
}

/// The field with the keyboard (`focus`: row, column).
pub fn field(rows: &mut [ParamRow], focus: (usize, usize)) -> Option<&mut TextInput> {
    let (row, col) = focus;
    rows.get_mut(row).map(|r| &mut r.fields[col])
}

/// Gives the keyboard to a field (`col` 3, the calls' value, only for a new parameter), its
/// caret at the end.
pub fn focus_field(rows: &mut [ParamRow], focus: &mut (usize, usize), row: usize, col: usize) {
    let col = if rows.get(row).is_some_and(ParamRow::is_added) { col } else { col.min(DEFAULT) };
    *focus = (row.min(rows.len().saturating_sub(1)), col);
    if let Some(field) = field(rows, *focus) {
        field.end(false);
    }
}

/// Tab: the next field (row by row).
fn next_field(rows: &mut [ParamRow], focus: &mut (usize, usize)) {
    let (row, col) = *focus;
    let last = if rows.get(row).is_some_and(ParamRow::is_added) { VALUE } else { DEFAULT };
    if col < last {
        focus_field(rows, focus, row, col + 1);
    } else if row + 1 < rows.len() {
        focus_field(rows, focus, row + 1, NAME);
    } else {
        focus_field(rows, focus, 0, NAME);
    }
}

/// The table's fields, the one with the keyboard, where each was drawn, and the flag the
/// view follows.
#[derive(Debug)]
pub struct ParamFields<'a> {
    pub rows: &'a mut [ParamRow],
    pub focus: &'a mut (usize, usize),
    /// Each field drawn last, with its (row, column).
    pub areas: &'a [(Rect, (usize, usize))],
    /// Set when a key or a text reached a field.
    pub edited: &'a mut bool,
}

impl ParamFields<'_> {
    /// The field drawn at `p`, the last drawn first.
    fn at(&self, p: Point) -> Option<(usize, usize)> {
        self.areas.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, at)| *at)
    }
}

impl Control for ParamFields<'_> {
    fn hit(&self, p: Point) -> bool {
        self.at(p).is_some()
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let Some(at) = self.at(p) else { return Press::Ignored };
        *self.focus = at;
        if let Some(field) = field(self.rows, at) {
            field.click(p.x);
        }
        Press::Took
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        let (row, col) = *self.focus;
        match key {
            ViewerKey::Tab => {
                next_field(self.rows, self.focus);
                return true;
            }
            ViewerKey::Up => {
                focus_field(self.rows, self.focus, row.saturating_sub(1), col);
                return true;
            }
            ViewerKey::Down => {
                focus_field(self.rows, self.focus, row + 1, col);
                return true;
            }
            _ => {}
        }
        let Some(field) = field(self.rows, *self.focus) else { return false };
        match key {
            ViewerKey::Backspace => field.backspace(false),
            ViewerKey::Delete => field.delete(),
            ViewerKey::Left => field.move_left(false, false),
            ViewerKey::Right => field.move_right(false, false),
            ViewerKey::Home => field.home(false),
            ViewerKey::End => field.end(false),
            _ => return false,
        }
        *self.edited = true;
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        if let Some(field) = field(self.rows, *self.focus) {
            field.insert(typed.lines().next().unwrap_or_default());
        }
        *self.edited = true;
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id != SELECT_ALL {
            return false;
        }
        let Some(field) = field(self.rows, *self.focus) else { return false };
        field.select_all();
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/param_fields.rs"]
mod tests;
