//! Editing in the grid: a cell's value typed in place (read by its column's class: a
//! number, a bool, a text…), a long one in the large editor (several lines), `NULL` set,
//! rows added, cloned and deleted, changes reverted — all kept pending by the source
//! ([`GridSource::set`] and the others) until submitted, where the view shows what will
//! run first ([`GridAction::Submit`]).

use ui_kit::TextInput;

use super::grid::{DataGrid, GridAction};
use super::selection::CellPos;
use super::source::{
    CellClass, CellRef, CellValue, Editing, GridSource, NEW_ROWS, RowState, Whole,
};
use super::text_area::TextArea;
use crate::tool_panel::KeyReach;
use crate::viewer::ViewerKey;

/// A cell being edited in place.
#[derive(Debug)]
pub(super) struct CellEditor {
    pub place: u64,
    /// The column's index in the source.
    pub col: usize,
    pub input: TextInput,
    pub error: Option<String>,
}

/// A long value edited in the large editor.
#[derive(Debug)]
pub(super) struct BigEditor {
    pub place: u64,
    pub col: usize,
    pub area: TextArea,
    pub error: Option<String>,
}

/// `text` as a value of a column of `class` (`nullable`: an empty number, bool or date is
/// `NULL`). `Err`: why it isn't one (for the user). The built-in editors' reading (a
/// column's own editor may differ: [`super::value_editor::editor_for`]).
pub fn parse_value(class: CellClass, text: &str, nullable: bool) -> Result<CellValue, String> {
    use super::value_editor::{ValueFormat, read};
    let format = match class {
        CellClass::Binary => return Err(i18n::t("grid.binary_read_only")),
        CellClass::Integer => ValueFormat::Integer,
        CellClass::Decimal => ValueFormat::Decimal,
        CellClass::Bool => ValueFormat::Bool,
        CellClass::DateTime => ValueFormat::DateTime,
        CellClass::Json => ValueFormat::Json,
        CellClass::Other => ValueFormat::Raw,
        CellClass::Text => ValueFormat::Text,
    };
    read(&format, text, nullable)
}

/// A value as its editor starts with it (`NULL`: empty).
pub fn edit_text(value: &CellRef<'_>) -> String {
    match value {
        CellRef::Null => String::new(),
        other => other.display(),
    }
}

impl<S: GridSource> DataGrid<S> {
    /// Rows can be edited now.
    pub fn can_edit(&self) -> bool {
        self.source.editing().allowed()
    }

    /// Why column `col` (the source's) can't be edited now, if it can't.
    fn edit_refusal(&self, col: usize) -> Option<String> {
        if let Editing::ReadOnly(why) = self.source.editing() {
            return Some(why);
        }
        let column = self.source.columns().get(col)?;
        if !column.editable {
            return Some(i18n::t("grid.generated_read_only"));
        }
        (column.class == CellClass::Binary).then(|| i18n::t("grid.binary_read_only"))
    }

    /// Says why something can't be edited (in the bottom bar until the next change).
    fn refuse(&mut self, why: String) {
        self.notice = Some(why);
    }

    /// The source's column at place `pos`.
    fn column_at_place(&self, pos: usize) -> Option<usize> {
        self.layout.visible().get(pos).copied()
    }

    /// The cell at the cursor edited in place, starting with `initial` typed over it
    /// (`None`: its value). `false` when it can't be.
    pub fn start_edit(&mut self, initial: Option<&str>) -> bool {
        let Some(cursor) = self.selection.cursor() else { return false };
        let Some(col) = self.column_at_place(cursor.col) else { return false };
        if let Some(why) = self.edit_refusal(col) {
            self.refuse(why);
            return false;
        }
        let row = self.row_at(cursor.row);
        if self.source.row_state(row) != RowState::Ready || !self.whole_here(cursor.row, col) {
            return false;
        }
        let column = self.source.columns()[col].clone();
        let editor = super::value_editor::editor_for(&column);
        let text = match initial {
            Some(typed) => typed.to_owned(),
            None => editor.text(&self.source.cell(row, col), &column),
        };
        // A value of several lines, or one its editor opens large (JSON): the large
        // editor (what was typed over the cell starts it).
        if text.contains('\n') || editor.large(&column) {
            return match initial {
                Some(typed) => self.start_big_with(Some(typed)),
                None => self.start_big(),
            };
        }
        let mut input = TextInput::new(&text);
        if initial.is_none() {
            input.select_all();
        }
        self.editor = Some(super::edit::CellEditor { place: cursor.row, col, input, error: None });
        self.notice = None;
        true
    }

    /// The cell at the cursor in the large editor.
    pub fn start_big(&mut self) -> bool {
        self.start_big_with(None)
    }

    /// The cell at the cursor in the large editor, starting with `typed` over it
    /// (`None`: its value as its editor writes it).
    fn start_big_with(&mut self, typed: Option<&str>) -> bool {
        let Some(cursor) = self.selection.cursor() else { return false };
        let Some(col) = self.column_at_place(cursor.col) else { return false };
        if let Some(why) = self.edit_refusal(col) {
            self.refuse(why);
            return false;
        }
        let row = self.row_at(cursor.row);
        if self.source.row_state(row) != RowState::Ready || !self.whole_here(cursor.row, col) {
            return false;
        }
        let column = self.source.columns()[col].clone();
        let text = match (&self.editor, typed) {
            (_, Some(typed)) => typed.to_owned(),
            (Some(e), None) if e.place == cursor.row && e.col == col => e.input.text().to_owned(),
            _ => {
                super::value_editor::editor_for(&column).text(&self.source.cell(row, col), &column)
            }
        };
        self.editor = None;
        self.big =
            Some(BigEditor { place: cursor.row, col, area: TextArea::new(&text), error: None });
        true
    }

    /// Whether the cell at `place` / `col` holds its whole value (a big one kept cut by the
    /// source is asked for: its large editor opens once it comes — [`Self::follow_whole`]).
    fn whole_here(&mut self, place: u64, col: usize) -> bool {
        let row = self.row_at(place);
        match self.source.whole(row, col) {
            Whole::Here => true,
            Whole::Coming => {
                self.whole_wanted = Some((place, col));
                self.notice = Some(i18n::t("grid.reading_whole"));
                false
            }
            Whole::Failed(why) => {
                self.whole_wanted = None;
                self.refuse(why);
                false
            }
        }
    }

    /// The whole value asked for came (or couldn't be read): its large editor opened, if
    /// the cursor is still on its cell.
    pub(super) fn follow_whole(&mut self) {
        let Some((place, col)) = self.whole_wanted else { return };
        let row = self.row_at(place);
        match self.source.whole(row, col) {
            Whole::Coming => {}
            Whole::Failed(why) => {
                self.whole_wanted = None;
                self.refuse(why);
            }
            Whole::Here => {
                self.whole_wanted = None;
                self.notice = None;
                let on_it = self
                    .selection
                    .cursor()
                    .is_some_and(|c| c.row == place && self.column_at_place(c.col) == Some(col));
                if on_it {
                    self.start_big();
                }
            }
        }
    }

    /// The value typed set (kept pending). `false` when it isn't one: the editor stays,
    /// saying why.
    pub fn commit_edit(&mut self) -> bool {
        let Some(editor) = &self.editor else { return true };
        let (place, col, text) = (editor.place, editor.col, editor.input.text().to_owned());
        match self.write(place, col, &text) {
            Ok(()) => {
                self.editor = None;
                true
            }
            Err(why) => {
                if let Some(e) = &mut self.editor {
                    e.error = Some(why);
                }
                false
            }
        }
    }

    pub fn commit_big(&mut self) -> bool {
        let Some(big) = &self.big else { return true };
        let (place, col, text) = (big.place, big.col, big.area.text().to_owned());
        match self.write(place, col, &text) {
            Ok(()) => {
                self.big = None;
                true
            }
            Err(why) => {
                if let Some(b) = &mut self.big {
                    b.error = Some(why);
                }
                false
            }
        }
    }

    /// `text` read for column `col` and set on the row at `place` (unchanged: nothing).
    fn write(&mut self, place: u64, col: usize, text: &str) -> Result<(), String> {
        let column = self.source.columns().get(col).cloned().ok_or_else(String::new)?;
        if column.class == CellClass::Binary {
            return Err(i18n::t("grid.binary_read_only"));
        }
        let editor = super::value_editor::editor_for(&column);
        let value = editor.parse(text, &column)?;
        let row = self.row_at(place);
        if editor.same(&self.source.cell(row, col), &value) {
            return Ok(());
        }
        self.source.set(row, col, value)?;
        self.edited();
        Ok(())
    }

    /// What depends on the values is worked out again.
    fn edited(&mut self) {
        self.sync_new_rows();
        self.find.stale = true;
        self.aggregates_stale();
    }

    pub fn cancel_edit(&mut self) {
        self.editor = None;
        self.big = None;
    }

    /// The selected cells set to `NULL` (those that can be).
    pub fn set_null(&mut self) {
        let cells = self.selection.cells();
        let mut refused = None;
        for cell in cells.into_iter().take(10_000) {
            let Some(col) = self.column_at_place(cell.col) else { continue };
            if let Some(why) = self.edit_refusal(col) {
                refused = Some(why);
                continue;
            }
            if !self.source.columns()[col].nullable {
                refused = Some(i18n::tr!(
                    "grid.not_nullable",
                    column = self.source.columns()[col].name.as_str()
                ));
                continue;
            }
            let row = self.row_at(cell.row);
            if self.source.row_state(row) == RowState::Ready
                && !self.source.cell(row, col).is_null()
            {
                let _ = self.source.set(row, col, CellValue::Null);
            }
        }
        self.edited();
        if let Some(why) = refused {
            self.refuse(why);
        }
    }

    /// A bool cell at the cursor switched.
    pub fn toggle_bool(&mut self) -> bool {
        let Some(cursor) = self.selection.cursor() else { return false };
        let Some(col) = self.column_at_place(cursor.col) else { return false };
        if self.source.columns()[col].class != CellClass::Bool || self.edit_refusal(col).is_some() {
            return false;
        }
        let row = self.row_at(cursor.row);
        let next = match self.source.cell(row, col) {
            CellRef::Bool(b) => CellValue::Bool(!b),
            _ => CellValue::Bool(true),
        };
        let _ = self.source.set(row, col, next);
        self.edited();
        true
    }

    /// The selected rows' places, those that came.
    fn selected_places(&self) -> Vec<u64> {
        self.selection
            .row_set()
            .into_iter()
            .filter(|&p| self.source.row_state(self.row_at(p)) == RowState::Ready)
            .collect()
    }

    /// The selected rows marked for deletion (new ones dropped).
    pub fn delete_rows(&mut self) {
        if let Editing::ReadOnly(why) = self.source.editing() {
            self.refuse(why);
            return;
        }
        let rows: Vec<u64> = self.selected_places().into_iter().map(|p| self.row_at(p)).collect();
        if rows.is_empty() {
            return;
        }
        self.source.delete(&rows);
        self.edited();
        // New rows dropped: the places after them moved up.
        let shown = self.shown_rows();
        if let Some((first, last)) = (shown > 0).then(|| (0, shown - 1)) {
            self.selection.clamp((first, last), self.layout.visible().len());
        }
    }

    /// A new row (a copy of the cursor's row with `clone`), shown last and selected.
    pub fn add_row(&mut self, clone: bool) -> bool {
        if let Editing::ReadOnly(why) = self.source.editing() {
            self.refuse(why);
            return false;
        }
        let like = if clone { self.selection.cursor().map(|c| self.row_at(c.row)) } else { None };
        if self.source.insert(like).is_none() {
            return false;
        }
        self.edited();
        let place = self.shown_rows().saturating_sub(1);
        // The first column one can type in.
        let pos = self
            .layout
            .visible()
            .iter()
            .position(|&c| self.edit_refusal(c).is_none() && !self.source.columns()[c].key)
            .unwrap_or(0);
        self.selection.set(CellPos::new(place, pos));
        self.reveal();
        true
    }

    /// The selected rows' changes dropped (`all`: every change).
    pub fn revert(&mut self, all: bool) {
        self.cancel_edit();
        if all {
            self.source.revert(None);
        } else {
            let rows: Vec<u64> =
                self.selection.row_set().into_iter().map(|p| self.row_at(p)).collect();
            self.source.revert(Some(&rows));
        }
        self.edited();
        let shown = self.shown_rows();
        if shown == 0 {
            self.selection.clear();
        } else {
            self.selection.clamp((0, shown - 1), self.layout.visible().len());
        }
    }

    /// Asks the view to submit (it shows what will run first).
    pub fn submit(&mut self) -> bool {
        if !self.commit_edit() || !self.commit_big() {
            return false;
        }
        if self.source.pending() == 0 {
            return false;
        }
        self.set_action(GridAction::Submit);
        true
    }

    /// A key while a cell is edited: `Some` when the editor took it.
    pub(super) fn editor_key(&mut self, key: ViewerKey, reach: KeyReach) -> Option<bool> {
        if let Some(big) = &mut self.big {
            let area = &mut big.area;
            match key {
                ViewerKey::Left => area.left(reach.word, reach.extend),
                ViewerKey::Right => area.right(reach.word, reach.extend),
                ViewerKey::Up => area.vertical(false, reach.extend),
                ViewerKey::Down => area.vertical(true, reach.extend),
                ViewerKey::Home => area.home(reach.word, reach.extend),
                ViewerKey::End => area.end(reach.word, reach.extend),
                ViewerKey::Backspace => area.backspace(reach.word),
                ViewerKey::Delete => area.delete(),
                ViewerKey::Tab => area.insert("\t"),
                // ⇧↵ applies; ↵ is a line break.
                ViewerKey::Enter if reach.extend => {
                    self.commit_big();
                }
                ViewerKey::Enter => area.insert("\n"),
                ViewerKey::Escape => self.big = None,
                _ => return Some(false),
            }
            return Some(true);
        }
        let editor = self.editor.as_mut()?;
        let input = &mut editor.input;
        match key {
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            ViewerKey::Escape => self.editor = None,
            // Applied, then on to the next cell (↵ down, ⇥ right, ↑ ↓ as they go).
            ViewerKey::Enter | ViewerKey::Tab | ViewerKey::Up | ViewerKey::Down => {
                if self.commit_edit() {
                    let (rows, cols) = match key {
                        ViewerKey::Tab => (0, 1),
                        ViewerKey::Up => (-1, 0),
                        _ => (1, 0),
                    };
                    let shown = self.shown_rows();
                    if shown > 0 {
                        let ncols = self.layout.visible().len();
                        self.selection.step(rows, cols, false, (0, shown - 1), ncols);
                        self.reveal();
                    }
                }
            }
            _ => return Some(false),
        }
        Some(true)
    }

    /// A character typed over the selected cell (no field has the keys): its editor
    /// starts with it — a space switches a bool.
    pub fn type_char(&mut self, c: char) -> bool {
        if self.selection.cursor().is_none() || c.is_control() {
            return false;
        }
        if c == ' ' && self.toggle_bool() {
            return true;
        }
        self.start_edit(Some(&c.to_string()))
    }

    /// Whether a row id is one of the new rows.
    pub fn is_new_row(row: u64) -> bool {
        row >= NEW_ROWS
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/edit.rs"]
mod tests;
