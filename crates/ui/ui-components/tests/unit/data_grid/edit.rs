//! Editing through the grid, over a fake source keeping its changes: typing in a cell,
//! values read by their column, NULL, rows added, cloned, deleted, reverted, submitted.

use std::collections::HashMap;
use std::ops::Range;

use kernel::Rect;

use super::*;
use crate::data_grid::GridAction;
use crate::data_grid::source::{
    CellMark, Editing, GridCaps, GridColumn, GridQuery, SourceStatus, Total, Whole,
};

#[derive(Default)]
struct Editable {
    columns: Vec<GridColumn>,
    rows: Vec<Vec<CellValue>>,
    set: HashMap<(u64, usize), CellValue>,
    added: Vec<(u64, Vec<CellValue>)>,
    deleted: Vec<u64>,
    read_only: Option<String>,
    /// A cell kept cut (its place), and whether its whole value came / failed.
    cut: Option<(u64, usize)>,
    whole: Option<Whole>,
    asked_whole: usize,
}

impl Editable {
    fn new() -> Self {
        let mut id = GridColumn::new("id", "INTEGER", CellClass::Integer);
        id.key = true;
        id.nullable = false;
        let mut total = GridColumn::new("total", "REAL", CellClass::Decimal);
        total.nullable = true;
        let mut paid = GridColumn::new("paid", "BOOL", CellClass::Bool);
        paid.nullable = false;
        let mut sum = GridColumn::new("sum", "INTEGER", CellClass::Integer);
        sum.editable = false;
        Self {
            columns: vec![id, total, paid, sum],
            rows: (0..3)
                .map(|i| {
                    vec![
                        CellValue::Int(i + 1),
                        CellValue::Float(1.5),
                        CellValue::Bool(false),
                        CellValue::Int(0),
                    ]
                })
                .collect(),
            ..Self::default()
        }
    }

    fn value(&self, row: u64, col: usize) -> Option<&CellValue> {
        if let Some(v) = self.set.get(&(row, col)) {
            return Some(v);
        }
        if row >= NEW_ROWS {
            return self.added.iter().find(|(id, _)| *id == row).and_then(|(_, v)| v.get(col));
        }
        self.rows.get(row as usize).and_then(|r| r.get(col))
    }
}

impl GridSource for Editable {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }
    fn total(&self) -> Total {
        Total::Known(self.rows.len() as u64)
    }
    fn row_state(&self, row: u64) -> RowState {
        if (row as usize) < self.rows.len() || self.added.iter().any(|(id, _)| *id == row) {
            RowState::Ready
        } else {
            RowState::Missing
        }
    }
    fn cell(&self, row: u64, column: usize) -> CellRef<'_> {
        self.value(row, column).map_or(CellRef::Null, CellValue::as_ref)
    }
    fn request(&mut self, _: &GridQuery, _: Range<u64>) {}
    fn poll(&mut self) -> bool {
        true
    }
    fn status(&self) -> SourceStatus {
        SourceStatus::Ready
    }
    fn capabilities(&self) -> GridCaps {
        GridCaps::default()
    }
    fn name(&self) -> String {
        "orders".into()
    }
    fn editing(&self) -> Editing {
        match &self.read_only {
            Some(why) => Editing::ReadOnly(why.clone()),
            None => Editing::Allowed,
        }
    }
    fn mark(&self, row: u64, column: usize) -> CellMark {
        if self.deleted.contains(&row) {
            CellMark::Deleted
        } else if row >= NEW_ROWS {
            CellMark::Inserted
        } else if self.set.contains_key(&(row, column)) {
            CellMark::Modified
        } else {
            CellMark::None
        }
    }
    fn set(&mut self, row: u64, column: usize, value: CellValue) -> Result<(), String> {
        self.set.insert((row, column), value);
        Ok(())
    }
    fn insert(&mut self, like: Option<u64>) -> Option<u64> {
        let id = NEW_ROWS + self.added.len() as u64;
        let values = match like {
            Some(row) => (0..self.columns.len()).map(|c| self.cell(row, c).to_value()).collect(),
            None => vec![CellValue::Null; self.columns.len()],
        };
        self.added.push((id, values));
        Some(id)
    }
    fn delete(&mut self, rows: &[u64]) {
        for &row in rows {
            if row >= NEW_ROWS {
                self.added.retain(|(id, _)| *id != row);
            } else if !self.deleted.contains(&row) {
                self.deleted.push(row);
            }
        }
    }
    fn revert(&mut self, rows: Option<&[u64]>) {
        match rows {
            None => {
                self.set.clear();
                self.added.clear();
                self.deleted.clear();
            }
            Some(rows) => {
                self.set.retain(|(r, _), _| !rows.contains(r));
                self.deleted.retain(|r| !rows.contains(r));
                self.added.retain(|(id, _)| !rows.contains(id));
            }
        }
    }
    fn pending(&self) -> usize {
        self.set.len() + self.added.len() + self.deleted.len()
    }
    fn new_rows(&self) -> Vec<u64> {
        self.added.iter().map(|(id, _)| *id).collect()
    }
    fn whole(&mut self, row: u64, column: usize) -> Whole {
        if self.cut != Some((row, column)) {
            return Whole::Here;
        }
        self.asked_whole += 1;
        self.whole.clone().unwrap_or(Whole::Coming)
    }
}

fn grid() -> DataGrid<Editable> {
    let mut g = DataGrid::new(Editable::new(), Some(100));
    g.poll();
    g.sync_new_rows();
    g.geometry.scale = 1.0;
    g.geometry.row_h = 28.0;
    g.geometry.body = Rect::new(0.0, 0.0, 400.0, 280.0);
    g
}

fn at(g: &mut DataGrid<Editable>, row: u64, col: usize) {
    g.selection.set(CellPos::new(row, col));
}

#[test]
fn values_are_read_by_their_columns_class() {
    assert_eq!(parse_value(CellClass::Integer, " 42 ", false), Ok(CellValue::Int(42)));
    assert!(parse_value(CellClass::Integer, "4.2", false).is_err());
    assert_eq!(parse_value(CellClass::Integer, "", true), Ok(CellValue::Null));
    assert!(parse_value(CellClass::Integer, "", false).is_err());
    assert_eq!(parse_value(CellClass::Decimal, "2.5", false), Ok(CellValue::Float(2.5)));
    assert!(parse_value(CellClass::Decimal, "inf", false).is_err());
    assert_eq!(parse_value(CellClass::Bool, "Yes", false), Ok(CellValue::Bool(true)));
    assert_eq!(parse_value(CellClass::Bool, "0", false), Ok(CellValue::Bool(false)));
    assert!(parse_value(CellClass::Bool, "maybe", false).is_err());
    assert_eq!(
        parse_value(CellClass::Text, "", true),
        Ok(CellValue::Text(String::new())),
        "a text stays text"
    );
    assert_eq!(parse_value(CellClass::DateTime, "", true), Ok(CellValue::Null));
    assert_eq!(parse_value(CellClass::Json, "{}", false), Ok(CellValue::Json("{}".into())));
    assert!(parse_value(CellClass::Binary, "x", false).is_err());
    assert_eq!(edit_text(&CellRef::Null), "");
    assert_eq!(edit_text(&CellRef::Float(1.5)), "1.5");
}

#[test]
fn typing_over_a_cell_edits_it_and_enter_moves_down() {
    let mut g = grid();
    at(&mut g, 0, 1);
    assert!(g.type_char('7'));
    assert!(g.wants_text());
    g.text_input(".25");
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert!(!g.wants_text());
    assert_eq!(g.source.cell(0, 1), CellRef::Float(7.25));
    assert_eq!(g.source.mark(0, 1), CellMark::Modified);
    assert_eq!(g.selection.cursor(), Some(CellPos::new(1, 1)));
    // The same value again: nothing pending.
    g.start_edit(None);
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert_eq!(g.source.pending(), 1);
}

#[test]
fn a_value_not_of_its_column_keeps_the_editor() {
    let mut g = grid();
    at(&mut g, 0, 0);
    g.type_char('x');
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert!(g.editor.as_ref().is_some_and(|e| e.error.is_some()));
    assert_eq!(g.source.pending(), 0);
    g.edit_key(ViewerKey::Escape, KeyReach::default());
    assert!(g.editor.is_none());
}

#[test]
fn space_switches_a_bool_and_delete_sets_null_where_it_can() {
    let mut g = grid();
    at(&mut g, 1, 2);
    g.type_char(' ');
    assert_eq!(g.source.cell(1, 2), CellRef::Bool(true));
    at(&mut g, 2, 1);
    g.selection.extend(CellPos::new(2, 2));
    g.edit_key(ViewerKey::Delete, KeyReach::default());
    assert_eq!(g.source.cell(2, 1), CellRef::Null);
    assert_eq!(g.source.cell(2, 2), CellRef::Bool(false), "not nullable");
    assert!(g.notice.is_some(), "it says why");
}

#[test]
fn generated_columns_and_read_only_sources_refuse_with_a_reason() {
    let mut g = grid();
    at(&mut g, 0, 3);
    assert!(!g.start_edit(None));
    assert!(g.notice.is_some());
    g.source.read_only = Some("no key".into());
    at(&mut g, 0, 1);
    assert!(!g.type_char('1'));
    assert_eq!(g.notice.as_deref(), Some("no key"));
    assert!(!g.add_row(false));
}

#[test]
fn rows_are_added_cloned_deleted_and_reverted() {
    let mut g = grid();
    assert!(g.add_row(false));
    assert_eq!(g.shown_rows(), 4);
    assert_eq!(g.selection.cursor(), Some(CellPos::new(3, 1)), "the first column one types in");
    at(&mut g, 0, 0);
    g.command("grid:clone-row");
    assert_eq!(g.shown_rows(), 5);
    let clone = g.row_at(4);
    assert_eq!(g.source.cell(clone, 0), CellRef::Int(1));
    assert_eq!(g.source.mark(clone, 0), CellMark::Inserted);
    // Whole rows selected: deleted.
    g.selection.rows(1, 1, 4, false);
    g.edit_key(ViewerKey::Backspace, KeyReach::default());
    assert_eq!(g.source.mark(1, 0), CellMark::Deleted);
    // A new row deleted is just gone.
    g.selection.rows(3, 3, 4, false);
    g.delete_rows();
    assert_eq!(g.shown_rows(), 4);
    g.selection.rows(1, 1, 4, false);
    g.command("grid:revert");
    assert_eq!(g.source.mark(1, 0), CellMark::None);
    assert_eq!(g.source.pending(), 1);
    g.command("grid:revert-all");
    assert_eq!(g.source.pending(), 0);
    assert_eq!(g.shown_rows(), 3);
}

#[test]
fn submit_applies_what_is_typed_and_asks_the_view() {
    let mut g = grid();
    assert!(!g.submit(), "nothing to submit");
    at(&mut g, 0, 1);
    g.type_char('3');
    assert!(g.submit(), "the cell being typed in is applied first");
    assert_eq!(g.take_action(), Some(GridAction::Submit));
    assert_eq!(g.source.cell(0, 1), CellRef::Float(3.0));
}

#[test]
fn the_large_editor_keeps_lines_and_shift_enter_applies() {
    let mut g = grid();
    g.source.columns[1].class = CellClass::Text;
    at(&mut g, 0, 1);
    assert!(g.start_big());
    g.edit_key(ViewerKey::End, KeyReach { word: true, extend: false });
    g.text_input("\nsecond");
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    g.text_input("third");
    assert!(g.big.is_some(), "↵ is a line break");
    g.edit_key(ViewerKey::Enter, KeyReach { word: false, extend: true });
    assert!(g.big.is_none());
    assert_eq!(g.source.cell(0, 1), CellRef::Text("1.5\nsecond\nthird"));
}

#[test]
fn a_big_value_kept_cut_is_read_whole_before_its_editor_opens() {
    let mut g = grid();
    g.source.columns[1].class = CellClass::Text;
    g.source.cut = Some((0, 1));
    at(&mut g, 0, 1);
    // Asked for: no editor yet, said in the bottom bar.
    assert!(!g.start_edit(None));
    assert!(g.editor.is_none() && g.big.is_none());
    assert_eq!(g.notice.as_deref(), Some(i18n::t("grid.reading_whole").as_str()));
    assert!(!g.type_char('x'), "typing over it waits too");
    // It came: the large editor opens on it (the cursor still there).
    g.source.whole = Some(Whole::Here);
    g.poll();
    assert!(g.big.is_some(), "opened once whole");
    g.cancel_edit();
    // Moved away meanwhile: nothing opens.
    g.source.whole = None;
    assert!(!g.start_big());
    at(&mut g, 1, 1);
    g.source.whole = Some(Whole::Here);
    g.poll();
    assert!(g.big.is_none());
    // It can't be read: why, said.
    g.source.whole = Some(Whole::Failed("gone".into()));
    at(&mut g, 0, 1);
    assert!(!g.start_edit(None));
    assert_eq!(g.notice.as_deref(), Some("gone"));
    assert!(g.source.asked_whole >= 3);
}
