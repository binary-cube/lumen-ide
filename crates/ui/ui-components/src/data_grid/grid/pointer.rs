//! The mouse over the grid.

use super::*;

impl<S: GridSource> DataGrid<S> {
    pub(super) fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The column shown at `x` (its place), and whether `x` is at its right edge.
    pub(super) fn column_at(&self, x: f32) -> Option<(usize, bool)> {
        let g = self.geometry;
        let logical = (x - g.body.min_x() - g.gutter + self.scroll_x) / g.scale.max(0.01);
        self.layout.at(logical, GRIP)
    }

    /// The cell under `p` (rows past the page's end: the last).
    pub(super) fn cell_at(&self, p: Point, clamp: bool) -> Option<CellPos> {
        let g = self.geometry;
        let rows = self.shown_rows();
        if rows == 0 || g.row_h <= 0.0 {
            return None;
        }
        let y = p.y - g.body.min_y() + self.scroll_y;
        let i = (y / g.row_h).floor();
        if !clamp && (i < 0.0 || i >= rows as f32) {
            return None;
        }
        let i = (i.max(0.0) as u64).min(rows - 1);
        let ncols = self.layout.visible().len();
        let col = match self.column_at(p.x) {
            Some((pos, _)) => pos,
            None if clamp && ncols > 0 => {
                if p.x < g.body.min_x() + g.gutter {
                    0
                } else {
                    ncols - 1
                }
            }
            None => return None,
        };
        Some(CellPos::new(i, col))
    }

    /// The page's row at `y` (`None`: past its rows).
    pub(super) fn row_place(&self, y: f32) -> Option<u64> {
        let g = self.geometry;
        let i = ((y - g.body.min_y() + self.scroll_y) / g.row_h.max(1.0)).floor();
        (i >= 0.0 && (i as u64) < self.shown_rows()).then_some(i as u64)
    }

    pub(super) fn in_body(&self, p: Point) -> bool {
        self.geometry.body.contains(p)
    }

    pub(super) fn in_gutter(&self, p: Point) -> bool {
        self.in_body(p) && p.x < self.geometry.body.min_x() + self.geometry.gutter
    }

    pub(super) fn in_header(&self, p: Point) -> bool {
        let h = self.geometry.header;
        h.contains(p) && p.x >= h.min_x() + self.geometry.gutter
    }

    /// A press at `p`, `clicks` in a row, with modifiers. `true` if something changed.
    pub fn press(&mut self, p: Point, clicks: u32, mods: PressMods) -> bool {
        self.press_clicks = clicks;
        self.press_shift = mods.shift;
        // The "…" menu first: a press elsewhere closes it (one on a row runs it).
        if self.more_open {
            self.more_open = false;
            if let Some(Hit::MoreChoice(tool)) = self.hit(p) {
                if tool == Tool::PageSize {
                    self.sizes_open = true;
                } else if tool == Tool::Extractor {
                    self.open_extractors();
                } else {
                    self.run_tool(tool);
                }
            }
            return true;
        }
        // The extractors' popover: a row used; a press elsewhere closes it (one inside it,
        // between rows, does nothing).
        if self.extract_open {
            match self.hit(p) {
                Some(Hit::ExtractorRow(at)) => self.use_extractor_row(at),
                _ if self.extract_area.is_some_and(|r| r.contains(p)) => {}
                _ => self.extract_open = false,
            }
            return true;
        }
        // The page sizes' popover first: a press elsewhere closes it.
        if self.sizes_open {
            self.sizes_open = false;
            if let Some(Hit::PageChoice(size)) = self.hit(p) {
                self.choose_page_size(size);
            }
            return true;
        }
        if let Some(offset) = self.vbar.press(p) {
            self.scroll_y = offset;
            self.drag = Some(Drag::VBar);
            return true;
        }
        if let Some(bar) = &mut self.hbar
            && bar.contains(p)
            && let Some(offset) = bar.begin_drag(p, self.hbar_metrics)
        {
            self.scroll_x = offset;
            self.drag = Some(Drag::HBar);
            return true;
        }
        // The large editor stays until applied or given up (a press elsewhere does nothing).
        if self.big.is_some() {
            return match self.hit(p) {
                Some(hit @ (Hit::BigApply | Hit::BigCancel | Hit::BigArea)) => {
                    self.press_hit(hit, p)
                }
                _ => false,
            };
        }
        if let Some(hit) = self.hit(p) {
            return self.press_hit(hit, p);
        }
        // A press elsewhere applies the cell being edited (a value not understood keeps it).
        if self.editor.is_some() && !self.commit_edit() {
            return true;
        }
        // A press in the rows or the headers: the fields let go of the keys.
        if self.field.is_some() && self.field != Some(Field::Find) {
            let field = self.field.take();
            if let Some(field) = field {
                self.apply_field(field);
            }
        } else if self.field == Some(Field::Find) {
            self.field = None;
        }
        if self.in_header(p) {
            return self.press_header(p, clicks, mods);
        }
        if self.transpose {
            return self.press_record(p);
        }
        if self.in_gutter(p) {
            let Some(cell) = self.row_place(p.y).map(|row| CellPos::new(row, 0)) else {
                return false;
            };
            let ncols = self.layout.visible().len();
            if mods.shift
                && let Some(cursor) = self.selection.cursor()
            {
                let (a, b) = (cursor.row.min(cell.row), cursor.row.max(cell.row));
                self.selection.rows(a, b, ncols, false);
            } else {
                self.selection.rows(cell.row, cell.row, ncols, mods.primary);
            }
            self.drag = Some(Drag::Rows);
            self.aggregates = None;
            return true;
        }
        if !self.in_body(p) {
            return false;
        }
        let Some(cell) = self.cell_at(p, false) else {
            self.selection.clear();
            self.aggregates = None;
            return true;
        };
        if mods.shift {
            self.selection.extend(cell);
        } else if mods.primary {
            self.selection.toggle(cell);
        } else {
            self.selection.set(cell);
        }
        self.drag = Some(Drag::Select);
        self.aggregates = None;
        // A double click edits it.
        if clicks == 2 && !mods.shift && !mods.primary {
            self.drag = None;
            self.start_edit(None);
        }
        true
    }

    pub(super) fn press_hit(&mut self, hit: Hit, p: Point) -> bool {
        let total = self.source.total();
        match hit {
            Hit::First => self.go(|pg, _| pg.first()),
            Hit::Back => self.go(|pg, _| pg.back()),
            Hit::Forward => self.go(|pg, _| pg.forward()),
            Hit::Last => {
                if let Some(total) = total.known() {
                    self.go(|pg, _| pg.last(total));
                }
            }
            Hit::PageSize => self.sizes_open = true,
            Hit::More => {
                self.open_more();
            }
            Hit::MoreChoice(tool) => {
                self.run_tool(tool);
            }
            Hit::Extractor => {
                self.open_extractors();
            }
            Hit::ExtractorRow(at) => self.use_extractor_row(at),
            Hit::PageChoice(size) => self.choose_page_size(size),
            Hit::Refresh => self.refresh(),
            Hit::Cancel => self.source.cancel(),
            Hit::Count => self.source.count(),
            Hit::Transpose => {
                self.toggle_transpose();
            }
            Hit::FindToggle => {
                if self.find.open {
                    self.close_find();
                } else {
                    self.open_find();
                }
            }
            Hit::Where | Hit::Order | Hit::Find => {
                let field = match hit {
                    Hit::Where => Field::Where,
                    Hit::Order => Field::Order,
                    _ => Field::Find,
                };
                if let Some(old) = self.field.filter(|f| *f != field && *f != Field::Find) {
                    self.apply_field(old);
                }
                let (clicks, shift) = (self.press_clicks, self.press_shift);
                let same = self.field == Some(field);
                self.field = Some(field);
                let input = self.input_mut(field);
                // ⇧: the selection stretched; two clicks: a word; three: everything.
                if shift && same {
                    input.extend_to(p.x);
                } else {
                    input.press(p.x, clicks);
                }
                self.drag = Some(Drag::Field(field));
            }
            Hit::ClearWhere => {
                self.where_input.set_text("");
                self.apply_field(Field::Where);
            }
            Hit::ClearOrder => {
                self.order_input.set_text("");
                self.apply_field(Field::Order);
            }
            Hit::FindPrev => self.find_step(false),
            Hit::FindNext => self.find_step(true),
            Hit::FindClose => self.close_find(),
            Hit::RecordPrev | Hit::RecordNext => {
                let ncols = self.layout.visible().len();
                if let Some(span) = self.rows_span() {
                    let step = if hit == Hit::RecordNext { 1 } else { -1 };
                    self.selection.step(step, 0, false, span, ncols);
                }
            }
            Hit::Corner => {
                self.select_all();
            }
            Hit::AddRow => {
                self.add_row(false);
            }
            Hit::DeleteRows => self.delete_rows(),
            Hit::Revert => self.revert(true),
            Hit::Submit => {
                self.submit();
            }
            Hit::BigApply => {
                self.commit_big();
            }
            Hit::BigCancel => self.big = None,
            Hit::BigArea => self.big_click = Some(p),
        }
        true
    }

    /// Moves the page (`step` given the paging and the total).
    pub(super) fn go(&mut self, step: impl FnOnce(&mut Paging, Option<u64>)) {
        let before = self.paging.start;
        step(&mut self.paging, self.source.total().known());
        if self.paging.start != before {
            self.scroll_y = 0.0;
            self.changed_rows();
        }
    }

    /// Reads everything again (what's applied stays).
    pub fn refresh(&mut self) {
        self.source.refresh();
        self.requested = None;
        self.end = None;
        self.changed_data();
    }

    pub(super) fn press_header(&mut self, p: Point, clicks: u32, mods: PressMods) -> bool {
        let Some((pos, edge)) = self.column_at(p.x) else { return false };
        let visible = self.layout.visible();
        let Some(&col) = visible.get(pos) else { return false };
        if edge {
            if clicks >= 2 {
                self.fit_pending = Some(col);
                self.drag = None;
            } else {
                self.drag = Some(Drag::Resize { col, x: p.x, width: self.layout.width(col) });
            }
            return true;
        }
        self.drag = Some(Drag::Header { pos, from: p, shift: mods.shift, moved: false });
        true
    }

    pub(super) fn press_record(&mut self, p: Point) -> bool {
        // Transposed: a press on a field's line puts the cursor there.
        let g = self.geometry;
        if !self.in_body(p) || g.row_h <= 0.0 {
            return false;
        }
        let i = ((p.y - g.body.min_y() + self.scroll_y) / g.row_h).floor();
        let ncols = self.layout.visible().len();
        if i < 0.0 || i as usize >= ncols {
            return false;
        }
        if let Some(row) = self.record_row() {
            self.selection.set(CellPos::new(row, i as usize));
            self.aggregates = None;
        }
        true
    }

    /// The mouse moved with the button down. `true` if something changed.
    pub fn drag(&mut self, p: Point) -> bool {
        let Some(drag) = self.drag else { return false };
        match drag {
            Drag::Field(field) => self.input_mut(field).extend_to(p.x),
            Drag::VBar => {
                if let Some(offset) = self.vbar.drag(p) {
                    self.scroll_y = offset;
                }
            }
            Drag::HBar => {
                if let Some(offset) =
                    self.hbar.as_ref().and_then(|b| b.drag_to(p, self.hbar_metrics))
                {
                    self.scroll_x = offset;
                }
            }
            Drag::Resize { col, x, width } => {
                let scale = self.geometry.scale.max(0.01);
                self.layout.set_width(col, width + (p.x - x) / scale);
            }
            Drag::Header { pos, from, shift, moved } => {
                let moved = moved || (p.x - from.x).abs() > 6.0 * self.geometry.scale;
                self.drag = Some(Drag::Header { pos, from, shift, moved });
                if moved {
                    self.drop_at = self.column_at(p.x).map(|(at, _)| at);
                }
            }
            Drag::Select => {
                if let Some(cell) = self.cell_at(p, true) {
                    self.selection.extend(cell);
                    self.aggregates = None;
                    self.autoscroll(p);
                }
            }
            Drag::Rows => {
                let row = self.row_place(p.y).or_else(|| self.cell_at(p, true).map(|c| c.row));
                if let (Some(row), Some(span)) = (row, self.selection.spans().last().copied()) {
                    let ncols = self.layout.visible().len();
                    let anchor = span.anchor.row;
                    self.selection.rows(anchor, row, ncols, false);
                    self.aggregates = None;
                    self.autoscroll(p);
                }
            }
        }
        true
    }

    /// Selecting past the rows' edges scrolls them.
    pub(super) fn autoscroll(&mut self, p: Point) {
        let body = self.geometry.body;
        let step = self.geometry.row_h;
        if p.y < body.min_y() {
            self.scroll_y = (self.scroll_y - step).max(0.0);
        } else if p.y > body.max_y() {
            self.scroll_y += step;
        }
    }

    /// The button released. `true` if something changed.
    pub fn release(&mut self) -> bool {
        let Some(drag) = self.drag.take() else { return false };
        if matches!(drag, Drag::Resize { .. } | Drag::Header { moved: true, .. }) {
            self.layout_touched();
        }
        match drag {
            Drag::VBar => {
                self.vbar.release();
            }
            Drag::HBar => {
                if let Some(bar) = &mut self.hbar {
                    bar.end_drag();
                }
            }
            Drag::Header { pos, shift, moved, .. } => {
                let target = self.drop_at.take();
                if moved {
                    if let Some(to) = target {
                        self.layout.move_column(pos, to);
                        self.selection.clear();
                        self.find.stale = true;
                    }
                } else if self.source.capabilities().sort {
                    let visible = self.layout.visible();
                    if let Some(&col) = visible.get(pos) {
                        let name = self.source.columns()[col].name.clone();
                        let mut keys = self.query.sort.clone();
                        cycle_sort(&mut keys, &name, shift);
                        self.set_sort(keys);
                    }
                }
            }
            Drag::Resize { .. } | Drag::Select | Drag::Rows | Drag::Field(_) => {}
        }
        true
    }

    /// The mouse over `p`. `true` if what's drawn changes.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let header = if self.in_header(p) { self.column_at(p.x).map(|(pos, _)| pos) } else { None };
        let bar = self.vbar.hover(p) | self.hbar.as_mut().is_some_and(|b| b.hover(p));
        let changed = hover != self.hover || header != self.hover_header || bar;
        self.hover = hover;
        self.hover_header = header;
        changed
    }

    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if matches!(self.drag, Some(Drag::Resize { .. })) {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        match self.hit(p) {
            Some(Hit::Where | Hit::Order | Hit::Find) => return Some(ViewerCursor::Text),
            Some(_) => return Some(ViewerCursor::Pointer),
            None => {}
        }
        if self.in_header(p) {
            return match self.column_at(p.x) {
                Some((_, true)) => Some(ViewerCursor::ResizeHorizontal),
                Some(_) if self.source.capabilities().sort => Some(ViewerCursor::Pointer),
                _ => None,
            };
        }
        None
    }

    /// The tooltip of what's under `p`: a toolbar's button (its name and keys), a field
    /// (how to write it), a header's mark (the key, the order). None while a popover or the
    /// large editor is open.
    pub fn tooltip_at(&self, p: Point) -> Option<crate::tooltip::Tip> {
        use crate::tooltip::Tip;
        if self.extract_open {
            // A template that can't be used says why.
            let &(r, hit) = self.hits.iter().rev().find(|(r, _)| r.contains(p))?;
            let Hit::ExtractorRow(at) = hit else { return None };
            let Some(PopRow::Choice { problem: Some(why), .. }) = self.extract_rows.get(at) else {
                return None;
            };
            return Some(Tip::new(r, why.clone()));
        }
        if self.more_open || self.sizes_open || self.big.is_some() || self.drag.is_some() {
            return None;
        }
        let tool_tip = |r: Rect, tool: Tool| {
            let label = if tool == Tool::Extractor {
                i18n::tr!("grid.tip_extractor", name = self.extractor().name())
            } else {
                super::super::paint::tool_label(self, tool)
            };
            Tip::new(r, label).with_shortcut(self.tool_keys(tool))
        };
        if let Some(&(r, hit)) = self.hits.iter().rev().find(|(r, _)| r.contains(p)) {
            if let Some(tool) = hit_tool(hit) {
                return Some(tool_tip(r, tool));
            }
            let key = match hit {
                Hit::Count => "grid.tip_count",
                Hit::Where if self.field != Some(Field::Where) => "grid.tip_where",
                Hit::Order if self.field != Some(Field::Order) => "grid.tip_order",
                Hit::ClearWhere | Hit::ClearOrder => "grid.tip_clear",
                Hit::FindPrev => "grid.tip_find_prev",
                Hit::FindNext => "grid.tip_find_next",
                Hit::FindClose => "grid.tip_find_close",
                Hit::RecordPrev => "grid.tip_record_prev",
                Hit::RecordNext => "grid.tip_record_next",
                Hit::Corner => "grid.tip_select_all",
                _ => return None,
            };
            return Some(Tip::new(r, i18n::t(key)));
        }
        let &(r, mark) = self.marks.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(match mark {
            Mark::Disabled(tool) => tool_tip(r, tool),
            Mark::Key => Tip::new(r, i18n::t("grid.tip_key")),
            Mark::Sort { descending, place, of } => {
                let way = i18n::t(if descending { "grid.tip_desc" } else { "grid.tip_asc" });
                let label = if of > 1 {
                    i18n::tr!("grid.tip_sorted_nth", way = way, place = place, of = of)
                } else {
                    i18n::tr!("grid.tip_sorted", way = way)
                };
                Tip::new(r, label)
            }
        })
    }

    /// The wheel (physical pixels). `true` if it moved.
    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.extract_open {
            let before = self.extract_scroll;
            self.extract_scroll = (self.extract_scroll + dy).max(0.0);
            // Clamped to its rows at the next paint.
            return (self.extract_scroll - before).abs() > 0.1;
        }
        if self.sizes_open || self.more_open {
            return false;
        }
        let (y, x) = (self.scroll_y, self.scroll_x);
        self.scroll_y = (self.scroll_y + dy).max(0.0);
        self.scroll_x = (self.scroll_x + dx).max(0.0);
        // Clamped to the content at the next paint.
        (self.scroll_y - y).abs() > 0.1 || (self.scroll_x - x).abs() > 0.1
    }
}
