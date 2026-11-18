//! Scrolling: the scrollbars (pressed, dragged), the cursor kept in view, the visible lines.

use super::*;

impl EditorView {
    /// Scroll by `(dx, dy)` physical pixels. Returns `true` if it moved.
    pub fn scroll_by(&mut self, dx: f32, dy: f32, doc: &Document, folds: &Folds) -> bool {
        let before = (self.scroll_x, self.scroll_y);
        self.scroll_x =
            (self.scroll_x + dx).clamp(0.0, self.max_scroll_x.max(self.scroll_x)).round();
        self.scroll_y += dy;
        self.clamp_scroll_y(doc, folds);
        (self.scroll_x, self.scroll_y) != before
    }

    /// Scrolls so that the primary cursor is visible.
    pub fn reveal_cursor(
        &mut self,
        doc: &Document,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
    ) {
        let layout = self.layout(text, doc, palette);
        let buffer = doc.buffer();
        let head = doc.selections().primary().head;
        let line = buffer.line_of(head);

        let top = folds.row_of(line as u32) as f32 * layout.line_height;
        let height = self.bounds.size.height;
        let margin = layout.line_height;
        if top - margin < self.scroll_y {
            self.scroll_y = (top - margin).max(0.0);
        } else if top + layout.line_height + margin > self.scroll_y + height {
            self.scroll_y = top + layout.line_height + margin - height;
        }
        self.clamp_scroll_y(doc, folds);

        buffer.write_line(line, &mut self.line);
        let col = head - buffer.line_start(line);
        let x = self.x_in_line(text, doc, col);
        let width = self.bounds.size.width - layout.text_left;
        let margin = layout.advance * 4.0;
        if x < self.scroll_x + margin {
            self.scroll_x = (x - margin).max(0.0);
        } else if x > self.scroll_x + width - margin {
            self.scroll_x = x - width + margin;
        }
        self.scroll_x = self.scroll_x.round();
    }

    /// After a jump (Go to Line, a definition, a search result): the cursor's line brought
    /// to the middle when it's off screen or near an edge (where the eye isn't), else left
    /// where it is; then as [`Self::reveal_cursor`] (the column).
    pub fn center_cursor(
        &mut self,
        doc: &Document,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
    ) {
        /// The lines kept from an edge for the line to count as seen.
        const COMFORT: f32 = 3.0;
        let line_height = self.layout(text, doc, palette).line_height;
        let line = doc.buffer().line_of(doc.selections().primary().head);
        let top = folds.row_of(line as u32) as f32 * line_height;
        let height = self.bounds.size.height;
        let margin = (COMFORT * line_height).min(height / 4.0);
        let seen =
            top >= self.scroll_y + margin && top + line_height <= self.scroll_y + height - margin;
        if !seen {
            self.scroll_y = (top + line_height / 2.0 - height / 2.0).max(0.0);
            self.clamp_scroll_y(doc, folds);
        }
        self.reveal_cursor(doc, folds, text, palette);
    }

    pub fn set_scrollbar_style(&mut self, style: ScrollbarStyle) {
        self.vbar.set_style(style.clone());
        self.hbar.set_style(style);
    }

    /// Content length, visible area and position, vertically and horizontally.
    pub(super) fn scroll_metrics(
        &self,
        doc: &Document,
        folds: &Folds,
    ) -> (ScrollMetrics, ScrollMetrics) {
        let lh = self.line_height();
        let rows = folds.visible_rows(doc.buffer().line_count() as u32);
        let height = self.bounds.size.height;
        // Can scroll until the last row reaches the top ("scroll past end").
        let vertical = ScrollMetrics {
            content: rows.saturating_sub(1) as f32 * lh + height,
            viewport: height,
            offset: self.scroll_y,
        };
        let text_width = self.bounds.size.width
            - self.layout.map_or(0.0, |l| l.gutter_width)
            - lane_width(self.scale);
        let horizontal = ScrollMetrics {
            content: text_width + self.max_scroll_x,
            viewport: text_width,
            offset: self.scroll_x,
        };
        (vertical, horizontal)
    }

    pub fn over_scrollbar(&self, p: Point) -> bool {
        self.vbar.contains(p) || self.hbar.contains(p)
    }

    /// `p` is on the right lane's thumb (the part in view).
    pub fn on_lane_thumb(&self, p: Point, doc: &Document, folds: &Folds) -> bool {
        let (v, _) = self.scroll_metrics(doc, folds);
        self.vbar.thumb_rect(v).is_some_and(|t| t.contains(p))
    }

    /// Click on a scrollbar (thumb or track). `true` if it was on one. On the lane's thumb
    /// it's held even over a mark (the mark's line taken on letting go, if it wasn't moved).
    pub fn scrollbar_press(&mut self, p: Point, doc: &Document, folds: &Folds) -> bool {
        let (v, h) = self.scroll_metrics(doc, folds);
        let on_thumb = self.vbar.thumb_rect(v).is_some_and(|t| t.contains(p));
        self.press_mark = on_thumb.then(|| self.error_mark_at(p).map(|m| (m, p.y))).flatten();
        if let Some(offset) = self.vbar.begin_drag(p, v) {
            self.scroll_y = offset.round();
            return true;
        }
        if let Some(offset) = self.hbar.begin_drag(p, h) {
            self.scroll_x = offset.round();
            return true;
        }
        self.over_scrollbar(p)
    }

    /// Dragging a scrollbar. `true` if one was being dragged.
    pub fn scrollbar_drag(&mut self, p: Point, doc: &Document, folds: &Folds) -> bool {
        let (v, h) = self.scroll_metrics(doc, folds);
        // Moved more than a little: a drag, not a click on the mark.
        if self.press_mark.is_some_and(|(_, y)| (p.y - y).abs() > 3.0 * self.scale) {
            self.press_mark = None;
        }
        if let Some(offset) = self.vbar.drag_to(p, v) {
            self.scroll_y = offset.round();
            return true;
        }
        if let Some(offset) = self.hbar.drag_to(p, h) {
            self.scroll_x = offset.round();
            return true;
        }
        false
    }

    /// The mark the lane's thumb was pressed on and let go of without moving: its offset
    /// (the cursor goes there, as a click on a mark outside the thumb).
    pub fn take_mark_press(&mut self) -> Option<usize> {
        self.press_mark.take().map(|(offset, _)| offset)
    }

    pub fn scrollbar_release(&mut self) -> bool {
        let now = std::time::Instant::now();
        let v = self.vbar.end_drag();
        let h = self.hbar.end_drag();
        // Let go: it stays a moment, then goes.
        if v {
            self.vauto.touch(now);
        }
        if h {
            self.hauto.touch(now);
        }
        v | h
    }

    /// The scrollbars: on the right a lane of its own, always shown (as the comparison's
    /// overview) — the part in view a block across it, easy to take wherever the marks are;
    /// at the bottom a bar over the text. With the mouse on the lane its marks grow (easy to
    /// hit). Returns the lane and how far its marks have grown (0 → 1).
    pub(super) fn paint_scrollbars(
        &mut self,
        f: &Frame<'_>,
        scene: &mut Scene,
        palette: &Palette,
    ) -> (Rect, f32) {
        let (b, text_area) = (f.bounds, f.text_area);
        let s = self.scale;
        let base = Scrollbar::track_thickness(s);
        let w = self.bar_wide.value();
        let lane = lane_width(s);
        let vtrack = Rect::new(b.max_x() - lane, b.min_y(), lane, (b.size.height - base).max(0.0));
        self.vbar.set_track(vtrack, s);
        self.hbar.set_track(
            Rect::new(
                text_area.min_x(),
                b.max_y() - base,
                (text_area.size.width - base).max(0.0),
                base,
            ),
            s,
        );
        self.vbar.set_expansion(w);
        // Hidden at rest, shown by a scroll (the marks on the strip are drawn apart: they stay).
        let now = std::time::Instant::now();
        self.vauto.observe(self.scroll_y, now);
        self.hauto.observe(self.scroll_x, now);
        let (v_held, h_held) = self.bars_held();
        self.vbar.set_opacity(self.vauto.opacity_at(now, v_held));
        self.hbar.set_opacity(self.hauto.opacity_at(now, h_held));
        let (v, h) = self.scroll_metrics(f.doc, f.deco.folds);
        // The lane: the editor's background (no text under it), a light tint, and the part in
        // view (lighter on hover, more while held).
        let layer = scene.push_layer(Some(vtrack));
        layer.quad(Quad::filled(vtrack, palette.editor_background));
        layer.quad(Quad::filled(vtrack, palette.text.with_alpha(0.025)));
        if let Some(thumb) = self.vbar.thumb_rect(v) {
            let alpha = if self.vbar.is_dragging() {
                0.14
            } else if self.vbar.is_hovered() {
                0.10
            } else {
                0.06
            };
            layer.quad(Quad::filled(thumb, palette.text.with_alpha(alpha)));
        }
        self.hbar.paint(scene, h);
        (vtrack, w)
    }

    /// The document line (0-based, fractional: how far down it the view starts) at the
    /// top of the view (a preview beside it follows it).
    pub fn top_line(&self, folds: &Folds) -> f32 {
        let row = (self.scroll_y / self.line_height()).max(0.0);
        folds.line_of(row.floor() as u32) as f32 + row.fract()
    }

    /// Scrolled so document line `line` (fractional) is at the top (a preview beside it
    /// was scrolled). `true` if it moved.
    pub fn scroll_to_line(&mut self, line: f32, doc: &Document, folds: &Folds) -> bool {
        let before = self.scroll_y;
        let line = line.max(0.0);
        let row = folds.row_of(line.floor() as u32) as f32 + line.fract();
        self.scroll_y = row * self.line_height();
        self.clamp_scroll_y(doc, folds);
        self.scroll_y != before
    }

    pub(super) fn clamp_scroll_y(&mut self, doc: &Document, folds: &Folds) {
        // Can scroll until the last row reaches the top ("scroll past end").
        let rows = folds.visible_rows(doc.buffer().line_count() as u32);
        let max = rows.saturating_sub(1) as f32 * self.line_height();
        self.scroll_y = self.scroll_y.clamp(0.0, max.max(0.0)).round();
    }
}

/// The right lane's width (physical pixels): kept by the text, the comparison's overview's.
pub(super) fn lane_width(scale: f32) -> f32 {
    (12.0 * scale).round()
}
