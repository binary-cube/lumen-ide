//! What is under the pointer: the folding arrow or label hovered, the scroll strip's marks,
//! the line, column and byte at a point (a wrapped line's row's part), where the caret is.

use super::*;

impl EditorView {
    /// Updates the arrow under the mouse. Returns `true` if a redraw is needed.
    pub fn hover(&mut self, p: Point, doc: &Document, folds: &Folds) -> bool {
        let hovered = self.fold_marker_at(p, doc, folds).or_else(|| self.fold_pill_at(p));
        let changed = hovered != self.hovered_marker;
        self.hovered_marker = hovered;
        let now = std::time::Instant::now();
        let (v_held, h_held) = self.bars_held();
        let (v_seen, h_seen) = (self.vauto.showing(now, v_held), self.hauto.showing(now, h_held));
        let bars = self.hover_bars(p);
        let v_seen = self.vauto.pointer(self.vbar.is_hovered(), v_seen, true, now);
        let h_seen = self.hauto.pointer(self.hbar.is_hovered(), h_seen, true, now);
        let bars = bars && (v_seen || h_seen);
        let error = self.error_mark_hit(p);
        let error_changed = error.map(|e| e.0) != self.hovered_error.map(|e| e.0);
        self.hovered_error = error;
        if error_changed {
            self.preview = error.map(|(offset, _)| {
                let mut appear = Tween::new(0.0, Duration::from_millis(150));
                appear.retarget(1.0);
                (offset, appear)
            });
        }
        // The lane's marks grow under the mouse (the lane always shows).
        let wide = self.vbar.is_hovered() || error.is_some();
        let widen = self.bar_wide.retarget(if wide { 1.0 } else { 0.0 });
        changed | error_changed | bars | widen
    }

    /// The mouse left this editor (another group, a panel over it): nothing stays hovered
    /// (the widened scroll strip shrinks back).
    pub fn unhover(&mut self, doc: &Document, folds: &Folds) -> bool {
        let hovered = self.vbar.is_hovered()
            || self.hbar.is_hovered()
            || self.hovered_marker.is_some()
            || self.hovered_error.is_some();
        hovered && self.hover(Point::new(f32::MIN, f32::MIN), doc, folds)
    }

    /// The line and column (in displayed characters) under `p`, also past the end of the line
    /// (for column selection).
    pub fn column_at(
        &mut self,
        p: Point,
        doc: &Document,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
    ) -> (usize, usize) {
        let layout = self.layout(text, doc, palette);
        self.refresh_rows(doc, folds);
        let (line, k) = self.row_at_y(p.y, doc, folds, &layout);
        let line = line as usize;
        let x = p.x - self.bounds.min_x() - layout.text_left + self.scroll_x;
        let col = (x / layout.advance.max(1.0)).round().max(0.0) as usize;
        if k == 0 {
            return (line, col);
        }
        // A wrapped line's later row: its columns go on from where the row starts.
        self.load_line(doc, line);
        let seg = self.seg_of(doc, k);
        let tab = doc.tab_width();
        let col = col.saturating_sub(seg.hang);
        (line, col + source_to_display_with(&self.line, seg.from, tab, &self.line_inlays))
    }

    /// The line of the collapsed region whose label (`⋯`) is under point `p`.
    pub fn fold_pill_at(&self, p: Point) -> Option<u32> {
        self.pills.iter().find(|(r, _)| r.contains(p)).map(|(_, line)| *line)
    }

    /// The document offset under point `p` (physical pixels).
    pub fn offset_at(
        &mut self,
        p: Point,
        doc: &Document,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
    ) -> usize {
        let layout = self.layout(text, doc, palette);
        let buffer = doc.buffer();
        self.refresh_rows(doc, folds);
        let (line, k) = self.row_at_y(p.y, doc, folds, &layout);
        let line = line as usize;
        self.load_line(doc, line);
        let seg = self.seg_of(doc, k);
        let x = p.x - self.bounds.min_x() - layout.text_left + self.scroll_x
            - seg.hang as f32 * layout.advance;
        buffer.line_start(line) + self.byte_in_row(text, doc, palette, seg, x)
    }

    /// The byte of the line in `self.line` at `x` from the start of its row `seg` (past a
    /// row's end that isn't the line's: its last character, the next row starting at it).
    pub(super) fn byte_in_row(
        &mut self,
        text: &mut TextSystem,
        doc: &Document,
        palette: &Palette,
        seg: Seg,
        x: f32,
    ) -> usize {
        if x <= 0.0 {
            return seg.from;
        }
        let tab = doc.tab_width();
        expand_tabs_with(&self.line, usize::MAX, tab, &self.line_inlays, &mut self.display);
        let font = self.font(palette);
        let shaped = text.shape_line(&self.display, &font);
        // On a wrapped line's row: from where the row starts.
        let base = source_to_display_with(&self.line, seg.from, tab, &self.line_inlays);
        let disp = shaped.byte_at(x + shaped.x_at(base), self.display.len());
        let mut at = display_to_source_with(&self.line, disp, tab, &self.line_inlays).max(seg.from);
        if !seg.last && at >= seg.to {
            at = self.line[..seg.to].char_indices().next_back().map_or(seg.from, |(i, _)| i);
        }
        at
    }

    /// The primary cursor's rectangle, for positioning the IME window.
    pub fn caret_rect(
        &mut self,
        doc: &Document,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
    ) -> Rect {
        let layout = self.layout(text, doc, palette);
        let buffer = doc.buffer();
        let head = doc.selections().primary().head;
        let line = buffer.line_of(head);
        self.refresh_rows(doc, folds);
        let (row, seg) = self.row_of_offset(doc, folds, head);
        self.load_line(doc, line);
        let x = self.x_in_row(text, doc, seg, head - buffer.line_start(line))
            + seg.hang as f32 * layout.advance;
        Rect::new(
            self.bounds.min_x() + layout.text_left + x - self.scroll_x,
            self.bounds.min_y() + row as f32 * layout.line_height - self.scroll_y,
            layout.caret_width,
            layout.line_height,
        )
    }

    pub(super) fn line_at_y(&self, y: f32, doc: &Document, folds: &Folds, layout: &Layout) -> u32 {
        self.row_at_y(y, doc, folds, layout).0
    }

    /// The line and which of its rows is at `y` (the last row's below the text).
    pub(super) fn row_at_y(
        &self,
        y: f32,
        doc: &Document,
        folds: &Folds,
        layout: &Layout,
    ) -> (u32, u32) {
        let row = ((y - self.bounds.min_y() + self.scroll_y) / layout.line_height).floor().max(0.0)
            as u32;
        let rows = self.total_rows(doc, folds);
        self.row_at(folds, row.min(rows.saturating_sub(1)))
    }
}
