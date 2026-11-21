//! The grid's keyboard: the fields and the cursor among the cells.

use super::*;

impl<S: GridSource> DataGrid<S> {
    /// One of its fields or editors has the keys (typed text goes there).
    pub fn wants_text(&self) -> bool {
        self.field.is_some() || self.editor.is_some() || self.big.is_some()
    }

    pub fn text_input(&mut self, typed: &str) -> bool {
        if let Some(big) = &mut self.big {
            big.area.insert(typed);
            return true;
        }
        if let Some(editor) = &mut self.editor {
            editor.input.insert(typed.lines().next().unwrap_or_default());
            editor.error = None;
            return true;
        }
        let typed = typed.lines().next().unwrap_or_default();
        match self.field {
            Some(Field::Where) => self.where_input.insert(typed),
            Some(Field::Order) => self.order_input.insert(typed),
            Some(Field::Find) => {
                self.find.search.input.insert(typed);
                self.find.stale = true;
                self.find.current = 0;
            }
            None => return false,
        }
        true
    }

    pub(super) fn input_mut(&mut self, field: Field) -> &mut TextInput {
        match field {
            Field::Where => &mut self.where_input,
            Field::Order => &mut self.order_input,
            Field::Find => &mut self.find.search.input,
        }
    }

    /// A key: in the field having it, else to move in the rows. `true` if used.
    pub fn edit_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if (self.sizes_open || self.more_open || self.extract_open) && key == ViewerKey::Escape {
            self.sizes_open = false;
            self.more_open = false;
            self.extract_open = false;
            return true;
        }
        if let Some(used) = self.editor_key(key, reach) {
            return used;
        }
        if let Some(field) = self.field {
            return self.field_key(field, key, reach);
        }
        self.cell_key(key, reach)
    }

    pub(super) fn field_key(&mut self, field: Field, key: ViewerKey, reach: KeyReach) -> bool {
        let input = self.input_mut(field);
        match key {
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            ViewerKey::Enter if field == Field::Find => self.find_step(!reach.extend),
            ViewerKey::Down | ViewerKey::Up if field == Field::Find => {
                self.find_step(key == ViewerKey::Down)
            }
            ViewerKey::Enter | ViewerKey::Tab => {
                self.apply_field(field);
                if self.field_error(field).is_none() {
                    self.field = None;
                }
            }
            ViewerKey::Escape => {
                if field == Field::Find {
                    self.close_find();
                } else {
                    // Given up: the field shows what's applied.
                    self.revert_field(field);
                    self.field = None;
                }
            }
            _ => return false,
        }
        if field == Field::Find && matches!(key, ViewerKey::Backspace | ViewerKey::Delete) {
            self.find.stale = true;
            self.find.current = 0;
        }
        true
    }

    pub(super) fn field_error(&self, field: Field) -> Option<&String> {
        match field {
            Field::Where => self.where_error.as_ref(),
            Field::Order => self.order_error.as_ref(),
            Field::Find => None,
        }
    }

    /// The field's text applied (an error kept beside it when it isn't understood).
    pub(in crate::data_grid) fn apply_field(&mut self, field: Field) {
        let columns = self.source.columns().to_vec();
        match field {
            Field::Where => match parse_where(self.where_input.text(), &columns) {
                Ok(filter) => {
                    self.where_error = None;
                    if filter != self.query.filter {
                        self.query.filter = filter;
                        self.query_changed();
                    }
                }
                Err(e) => self.where_error = Some(e.message()),
            },
            Field::Order => match parse_order(self.order_input.text(), &columns) {
                Ok(keys) => {
                    self.order_error = None;
                    if keys != self.query.sort {
                        self.query.sort = keys;
                        self.query_changed();
                    }
                }
                Err(e) => self.order_error = Some(e.message()),
            },
            Field::Find => {}
        }
    }

    pub(super) fn revert_field(&mut self, field: Field) {
        match field {
            Field::Where => {
                let text = self.query.filter.as_ref().map(where_text).unwrap_or_default();
                self.where_input.set_text(&text);
                self.where_error = None;
            }
            Field::Order => {
                self.order_input.set_text(&order_text(&self.query.sort));
                self.order_error = None;
            }
            Field::Find => {}
        }
    }

    /// The places of the first and the last row shown (`None`: there are none).
    pub(super) fn rows_span(&self) -> Option<(u64, u64)> {
        let rows = self.shown_rows();
        (rows > 0).then(|| (0, rows - 1))
    }

    /// Moving in the rows: arrows, a page, the row's ends (with ⌘ / far: the first or the
    /// last row); ⇧ extends.
    pub(super) fn cell_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let ncols = self.layout.visible().len();
        let Some(span) = self.rows_span() else { return false };
        let page = ((self.geometry.body.size.height - self.geometry.row_h)
            / self.geometry.row_h.max(1.0))
        .floor()
        .max(1.0) as i64;
        let extend = reach.extend;
        if self.transpose {
            // Transposed: ↑ ↓ through the columns, ← → through the rows.
            return match key {
                ViewerKey::Up => self.selection.step(0, -1, false, span, ncols),
                ViewerKey::Down => self.selection.step(0, 1, false, span, ncols),
                ViewerKey::Left => self.selection.step(-1, 0, false, span, ncols),
                ViewerKey::Right => self.selection.step(1, 0, false, span, ncols),
                ViewerKey::Escape => self.toggle_transpose(),
                _ => false,
            } && {
                self.aggregates = None;
                true
            };
        }
        let moved = match key {
            ViewerKey::Up => self.selection.step(-1, 0, extend, span, ncols),
            ViewerKey::Down => self.selection.step(1, 0, extend, span, ncols),
            ViewerKey::Left => self.selection.step(0, -1, extend, span, ncols),
            ViewerKey::Right => self.selection.step(0, 1, extend, span, ncols),
            ViewerKey::Tab => self.selection.step(0, 1, false, span, ncols),
            ViewerKey::PageUp => self.selection.step(-page, 0, extend, span, ncols),
            ViewerKey::PageDown => self.selection.step(page, 0, extend, span, ncols),
            ViewerKey::Home if reach.word => {
                self.selection.step(i64::MIN / 2, 0, extend, span, ncols)
            }
            ViewerKey::End if reach.word => {
                self.selection.step(i64::MAX / 2, 0, extend, span, ncols)
            }
            ViewerKey::Home => self.selection.step(0, -(ncols as i64), extend, span, ncols),
            ViewerKey::End => self.selection.step(0, ncols as i64, extend, span, ncols),
            ViewerKey::Escape if self.find.open => {
                self.close_find();
                true
            }
            ViewerKey::Escape if !self.selection.is_empty() => {
                self.selection.clear();
                true
            }
            // ↵ edits the cell (⇧↵ in the large editor); ⌫ / ⌦ set NULL, or delete whole
            // rows selected.
            ViewerKey::Enter if reach.extend => return self.start_big(),
            ViewerKey::Enter => return self.start_edit(None),
            ViewerKey::Backspace | ViewerKey::Delete => {
                if self.whole_rows() {
                    self.delete_rows();
                } else {
                    self.set_null();
                }
                return true;
            }
            _ => false,
        };
        if moved {
            self.aggregates = None;
            self.reveal_cursor();
        }
        moved
    }

    /// The selection is of whole rows (their numbers clicked).
    pub(super) fn whole_rows(&self) -> bool {
        let ncols = self.layout.visible().len();
        !self.selection.is_empty()
            && self
                .selection
                .spans()
                .iter()
                .all(|s| *s.cols().start() == 0 && *s.cols().end() + 1 >= ncols && ncols > 0)
            && ncols > 1
    }
}
