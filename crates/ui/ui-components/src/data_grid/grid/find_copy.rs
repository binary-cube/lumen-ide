//! Finding in the rows, and what's copied of them.

use super::*;

impl<S: GridSource> DataGrid<S> {
    pub fn open_find(&mut self) {
        self.find.open = true;
        self.find.search.input.select_all();
        self.field = Some(Field::Find);
        self.find.stale = true;
    }

    pub(super) fn close_find(&mut self) {
        self.find.open = false;
        self.find.matches.clear();
        if self.field == Some(Field::Find) {
            self.field = None;
        }
    }

    /// The matches of the find field among the page's rows that came (case ignored).
    pub(in crate::data_grid) fn refresh_find(&mut self) {
        if !self.find.stale {
            return;
        }
        self.find.stale = false;
        self.find.matches.clear();
        let needle = self.find.search.input.text().trim().to_lowercase();
        if !self.find.open || needle.is_empty() {
            return;
        }
        let visible = self.layout.visible();
        let mut buffer = String::new();
        'rows: for i in 0..self.shown_rows() {
            let row = self.row_at(i);
            if self.source.row_state(row) != RowState::Ready {
                continue;
            }
            for (pos, &col) in visible.iter().enumerate() {
                buffer.clear();
                self.source.cell(row, col).write_display(&mut buffer);
                if buffer.to_lowercase().contains(&needle) {
                    self.find.matches.push(CellPos::new(i, pos));
                    if self.find.matches.len() >= FIND_LIMIT {
                        break 'rows;
                    }
                }
            }
        }
        self.find.current = self.find.current.min(self.find.matches.len().saturating_sub(1));
    }

    /// The next (or previous) match selected and shown.
    pub(super) fn find_step(&mut self, forward: bool) {
        self.refresh_find();
        let n = self.find.matches.len();
        if n == 0 {
            return;
        }
        // The first step goes to the current one; the next ones move.
        let at = self.selection.cursor();
        if at == Some(self.find.matches[self.find.current]) {
            self.find.current =
                if forward { (self.find.current + 1) % n } else { (self.find.current + n - 1) % n };
        }
        self.selection.set(self.find.matches[self.find.current]);
        self.aggregates = None;
        self.reveal_cursor();
    }

    /// The selected cells' rows and columns (by source index), the cells not selected of
    /// those rows left out as `None` — what's copied.
    pub(super) fn selected_block(&self) -> Option<(Vec<u64>, Vec<usize>)> {
        if self.selection.is_empty() {
            return None;
        }
        let visible = self.layout.visible();
        let rows: Vec<u64> = self
            .selection
            .row_set()
            .into_iter()
            .filter(|&r| self.source.row_state(self.row_at(r)) == RowState::Ready)
            .collect();
        let cols: Vec<usize> =
            self.selection.col_set().into_iter().filter_map(|p| visible.get(p).copied()).collect();
        (!rows.is_empty() && !cols.is_empty()).then_some((rows, cols))
    }

    /// The selection as extractor `id` writes it (only rows that came; `None`: nothing
    /// selected, or no such extractor).
    pub fn copy_as(&self, id: &str) -> Option<String> {
        let extractor = extractors::catalog().find(self.project, id)?;
        if extractor.problem().is_some() {
            return None;
        }
        self.copy(extractor.as_ref())
    }

    /// The selection as `extractor` writes it (only rows that came).
    pub fn copy(&self, extractor: &dyn Extractor) -> Option<String> {
        let (rows, cols) = self.selected_block()?;
        let columns = self.source.columns();
        let copy_columns: Vec<super::super::source::GridColumn> =
            cols.iter().map(|&c| columns[c].clone()).collect();
        let visible = self.layout.visible();
        let values: Vec<Vec<CellRef<'_>>> = rows
            .iter()
            .map(|&r| {
                cols.iter()
                    .map(|&c| {
                        let pos = visible.iter().position(|&v| v == c).unwrap_or(0);
                        if self.selection.contains(CellPos::new(r, pos)) {
                            self.source.cell(self.row_at(r), c)
                        } else {
                            CellRef::Null
                        }
                    })
                    .collect()
            })
            .collect();
        Some(copy_text(
            extractor,
            &self.source.name(),
            &copy_columns,
            self.source.sql(),
            extractors::catalog().options(),
            &values,
        ))
    }

    /// The text field having the keys: the cell's editor, else a field.
    pub(super) fn typing_input(&mut self) -> Option<&mut TextInput> {
        if self.editor.is_some() {
            return self.editor.as_mut().map(|e| &mut e.input);
        }
        let field = self.field?;
        Some(self.input_mut(field))
    }

    /// ⌘Z / ⇧⌘Z in the text field having the keys. `false`: none has them, or nothing
    /// to go back to.
    pub fn undo(&mut self, redo: bool) -> bool {
        let field = self.field;
        let Some(input) = self.typing_input() else { return false };
        let done = if redo { input.redo() } else { input.undo() };
        if done && field == Some(Field::Find) && self.editor.is_none() {
            self.find.stale = true;
            self.find.current = 0;
        }
        done
    }

    /// ⌘X in the text field having the keys: its selected text, taken out.
    pub fn cut(&mut self) -> Option<String> {
        let selected = self.selected_text()?;
        if self.wants_text() && self.big.is_none() {
            self.text_input("");
        }
        Some(selected)
    }

    /// ⌘C: the selected text of the field having the keys, else the cells as the project's
    /// data extractor writes them.
    pub fn selected_text(&self) -> Option<String> {
        if let Some(big) = &self.big {
            let selected = big.area.selected_text();
            return (!selected.is_empty()).then(|| selected.to_owned());
        }
        if let Some(editor) = &self.editor {
            let selected = editor.input.selected_text();
            return (!selected.is_empty()).then(|| selected.to_owned());
        }
        if let Some(field) = self.field {
            let input = match field {
                Field::Where => &self.where_input,
                Field::Order => &self.order_input,
                Field::Find => &self.find.search.input,
            };
            let selected = input.selected_text();
            return (!selected.is_empty()).then(|| selected.to_owned());
        }
        self.copy(self.extractor().as_ref())
    }

    /// The selection's figures (worked out once per change).
    pub fn aggregates(&mut self) -> Option<Aggregates> {
        if self.selection.count() < 2 {
            return None;
        }
        if let Some(a) = self.aggregates {
            return Some(a);
        }
        let visible = self.layout.visible();
        let mut a = Aggregates::new();
        for cell in self.selection.cells().into_iter().take(AGGREGATE_LIMIT) {
            let Some(&col) = visible.get(cell.col) else { continue };
            let row = self.row_at(cell.row);
            if self.source.row_state(row) == RowState::Ready {
                a.add(&self.source.cell(row, col));
            }
        }
        self.aggregates = Some(a);
        Some(a)
    }
}
