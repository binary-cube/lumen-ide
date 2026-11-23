//! The gutter: line numbers, blame annotations, color swatches, change bars, fold arrows; what a
//! click on them hits.

use super::*;

/// The grey squares of the checkerboard under a transparent color (with white).
const CHECKER_GREY: Rgba = Rgba::rgb(0xc8c8c8);

impl EditorView {
    /// The line of the color swatch under point `p` (click in the gutter).
    pub fn swatch_at(&self, p: Point) -> Option<u32> {
        self.swatch_rects.iter().find(|(r, _)| r.contains(p)).map(|(_, line)| *line)
    }

    /// Reserves `chars` characters for annotations (git blame) in the gutter (0: none).
    pub fn set_annotation_chars(&mut self, chars: usize) {
        if chars != self.annotation_chars {
            self.annotation_chars = chars;
            self.layout = None;
        }
    }

    /// The line whose annotation is under `p`.
    pub fn annotation_at(&self, p: Point) -> Option<u32> {
        self.annotation_rects.iter().find(|(r, _)| r.contains(p)).map(|(_, line)| *line)
    }

    /// The annotation row of `line` (for its tooltip).
    pub fn annotation_rect(&self, line: u32) -> Option<Rect> {
        self.annotation_rects.iter().find(|(_, l)| *l == line).map(|(r, _)| *r)
    }

    /// The change (git) whose gutter bar is under `p` (its index in `Decorations::vcs`).
    pub fn vcs_change_at(&self, p: Point) -> Option<usize> {
        // A little wider than the bar: it's thin.
        let slack = 3.0 * self.scale;
        self.vcs_rects
            .iter()
            .find(|(r, _)| {
                p.x >= r.min_x() - slack
                    && p.x <= r.max_x() + slack
                    && p.y >= r.min_y()
                    && p.y <= r.max_y()
            })
            .map(|(_, i)| *i)
    }

    /// The first line of the statement whose gutter mark is under `p` (a module's ▶).
    pub fn statement_mark_at(&self, p: Point) -> Option<u32> {
        self.statement_rects.iter().find(|(r, _)| r.contains(p)).map(|(_, line)| *line)
    }

    /// The color swatch of line `line` (the color picker's anchor).
    pub fn swatch_rect(&self, line: u32) -> Option<Rect> {
        self.swatch_rects.iter().find(|(_, l)| *l == line).map(|(r, _)| *r)
    }

    /// The line with a folding arrow under point `p` (click in the gutter).
    pub fn fold_marker_at(&self, p: Point, doc: &Document, folds: &Folds) -> Option<u32> {
        let layout = self.layout?;
        let b = self.bounds;
        // The whole strip to the right of the line numbers is the arrows' area.
        let marker_left = b.min_x() + layout.gutter_width - layout.advance * 2.4;
        if p.x < marker_left || p.x >= b.min_x() + layout.gutter_width || !b.contains(p) {
            return None;
        }
        let line = self.line_at_y(p.y, doc, folds, &layout);
        folds.fold_at(line).map(|_| line)
    }

    /// Gutter: line numbers (those with a cursor brighter), annotations, color swatches,
    /// change bars and folding arrows.
    pub(super) fn paint_gutter(
        &mut self,
        f: &Frame<'_>,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let (b, buffer) = (f.bounds, f.doc.buffer());
        self.swatch_rects.clear();
        self.vcs_rects.clear();
        self.bulb_rect = None;
        self.annotation_rects.clear();
        self.statement_rects.clear();
        let gutter = scene.push_layer(Some(Rect::new(
            b.min_x(),
            b.min_y(),
            f.layout.gutter_width,
            b.size.height,
        )));
        self.cursor_lines.clear();
        self.cursor_lines.extend(f.doc.selections().iter().map(|s| buffer.line_of(s.head)));
        self.cursor_lines.sort_unstable();
        for i in 0..self.lines.len() {
            let row = Row { line: self.lines[i], top: f.row_top(i), baseline: f.row_baseline(i) };
            // A wrapped line's later rows: its change bar goes on, the wrap's mark where the
            // number would be; the rest is the line's first row's.
            if !self.seg(i).first {
                self.paint_change_bar(f, gutter, row, palette);
                self.paint_wrap_goes_on(f, gutter, text, row, palette);
                continue;
            }
            self.paint_line_number(f, gutter, text, row, palette);
            self.paint_annotation(f, gutter, text, row, palette);
            // The bulb takes the swatch's place on its line.
            match f.deco.bulb.filter(|b| b.line == row.line) {
                Some(bulb) => self.paint_bulb(f, gutter, row, bulb, palette),
                None => self.paint_swatch(f, gutter, row, palette),
            }
            self.paint_change_bar(f, gutter, row, palette);
            self.paint_statement_mark(f, gutter, text, row, palette);
            self.paint_fold_arrow(f, gutter, row, palette);
        }
        self.paint_current_statement(f, gutter, palette);
    }

    /// The statement the cursor is in (a console's: what Run runs): a thin bar in the
    /// accent's color along its visible lines, at the gutter's left edge.
    fn paint_current_statement(
        &self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        palette: &Palette,
    ) {
        let Some((first, end)) = f.deco.current else { return };
        let layout = &f.layout;
        let mut span: Option<(f32, f32)> = None;
        for i in 0..self.lines.len() {
            let line = self.lines[i] as usize;
            if line < first || line >= end {
                continue;
            }
            let top = f.row_top(i);
            let bottom = top + layout.line_height;
            span = Some(span.map_or((top, bottom), |(t, b)| (t.min(top), b.max(bottom))));
        }
        let Some((top, bottom)) = span else { return };
        let (x, w) = current_bar(layout.annotation, self.scale);
        let inset = (2.0 * self.scale).round();
        let r = Rect::new(
            (f.bounds.min_x() + x).round(),
            (top + inset).round(),
            w,
            (bottom - top - 2.0 * inset).max(w).round(),
        );
        gutter.quad(Quad::filled(r, palette.accent.with_alpha(0.85)).rounded(w / 2.0));
    }

    /// On a wrapped line's later row, where its number would be: the mark that it goes on.
    fn paint_wrap_goes_on(
        &self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        palette: &Palette,
    ) {
        let shaped = text.shape_line(super::text::WRAP_GOES_ON, &f.font);
        let x = (f.bounds.min_x() + f.layout.gutter_width - f.layout.advance * 2.6 - shaped.width)
            .round();
        draw_text(gutter, &shaped, x, row.baseline, palette.gutter_text.with_alpha(0.6));
    }

    /// A row's line number, brighter on a cursor's line.
    pub(super) fn paint_line_number(
        &mut self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        palette: &Palette,
    ) {
        self.number.clear();
        let _ = write!(self.number, "{}", row.line + 1);
        let active = self.cursor_lines.binary_search(&(row.line as usize)).is_ok();
        let color = if active { palette.gutter_text_active } else { palette.gutter_text };
        let shaped = text.shape_line(&self.number, &f.font);
        let x = (f.bounds.min_x() + f.layout.gutter_width - f.layout.advance * 2.6 - shaped.width)
            .round();
        draw_text(gutter, &shaped, x, row.baseline, color);
    }

    /// The annotation (blame): the commit's date and author, on the block's first line;
    /// a thin strip whose strength shows how recent the change is.
    pub(super) fn paint_annotation(
        &mut self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        palette: &Palette,
    ) {
        let (b, layout, font) = (f.bounds, &f.layout, &f.font);
        if layout.annotation <= 0.0 {
            return;
        }
        let Some(Some(a)) = f.deco.annotations.get(row.line as usize) else { return };
        let area = Rect::new(b.min_x(), row.top, layout.annotation, layout.line_height);
        let strip = Rect::new(
            b.min_x() + (layout.advance * 0.4).round(),
            area.min_y(),
            (2.0 * self.scale).round().max(1.0),
            layout.line_height,
        );
        let strength = 0.15 + 0.6 * (1.0 - a.age.clamp(0.0, 1.0));
        gutter.quad(Quad::filled(strip, palette.accent.with_alpha(strength)));
        // On every line; brighter on the first line of each commit's block.
        let small = Font::new(font.family.clone(), (font.size * 0.86).round());
        let label = text.shape_line(&a.text, &small);
        let color = if a.first { palette.gutter_text_active } else { palette.gutter_text };
        draw_text(gutter, &label, (b.min_x() + layout.advance * 1.2).round(), row.baseline, color);
        if let Some(area) = area.intersect(&b) {
            self.annotation_rects.push((area, row.line));
        }
    }

    /// The code actions' bulb: a small lamp (its glass a disc, its base a bar) to the left
    /// of the number — the warning's color for a quick fix, muted for the other actions.
    fn paint_bulb(
        &mut self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        row: Row,
        bulb: Bulb,
        palette: &Palette,
    ) {
        let (b, layout) = (f.bounds, &f.layout);
        let size = (layout.line_height * 0.46).round().max(6.0);
        let left = (b.min_x() + layout.annotation + layout.advance * 0.7).round();
        let top = (row.top + (layout.line_height - size * 1.45) / 2.0).round();
        let color = if bulb.fix { palette.warning } else { palette.text_muted };
        gutter.quad(Quad::filled(Rect::new(left, top, size, size), color).rounded(size / 2.0));
        let base_w = (size * 0.5).round().max(2.0);
        let base = Rect::new(
            (left + (size - base_w) / 2.0).round(),
            (top + size * 1.02).round(),
            base_w,
            (size * 0.32).round().max(2.0),
        );
        gutter.quad(Quad::filled(base, color.with_alpha(color.a * 0.8)).rounded(self.scale));
        let hit = Rect::new(
            left - 2.0 * self.scale,
            row.top,
            size + 4.0 * self.scale,
            layout.line_height,
        );
        self.bulb_rect = hit.intersect(&b);
    }

    /// The color written on the line: a swatch to the left of the number.
    pub(super) fn paint_swatch(
        &mut self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        row: Row,
        palette: &Palette,
    ) {
        let swatches = f.deco.swatches;
        let Ok(k) = swatches.binary_search_by_key(&row.line, |(l, _)| *l) else { return };
        let (b, layout) = (f.bounds, &f.layout);
        let swatch = swatches[k].1;
        let size = (layout.line_height * 0.5).round();
        let r = Rect::new(
            (b.min_x() + layout.annotation + layout.advance * 0.7).round(),
            (row.top + (layout.line_height - size) / 2.0).round(),
            size,
            size,
        );
        let radius = (3.0 * self.scale).round();
        let line_w = self.scale.round().max(1.0);
        if swatch.a < 1.0 {
            // Under a transparent color: checkerboard (white / gray).
            gutter.quad(Quad::filled(r, Rgba::WHITE).rounded(radius));
            let half = (size / 2.0).round();
            let grey = CHECKER_GREY;
            gutter.quad(Quad::filled(Rect::new(r.min_x(), r.min_y(), half, half), grey));
            gutter.quad(Quad::filled(
                Rect::new(r.min_x() + half, r.min_y() + half, size - half, size - half),
                grey,
            ));
        }
        gutter.quad(
            Quad::filled(r, swatch).rounded(radius).bordered(line_w, palette.text.with_alpha(0.22)),
        );
        // Only the visible part: a line scrolled above the editor must not take clicks there.
        if let Some(r) = r.intersect(&b) {
            self.swatch_rects.push((r, row.line));
        }
    }

    /// Changed since the last commit: a bar between the number and the fold arrow (a wedge
    /// at the boundary for deleted lines).
    pub(super) fn paint_change_bar(
        &mut self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        row: Row,
        palette: &Palette,
    ) {
        use workbench_model::MarkKind as ChangeKind;
        let (vcs, line) = (f.deco.vcs, row.line as usize);
        let change = vcs.partition_point(|c| c.lines.end.max(c.lines.start + 1) <= line);
        let Some(c) = vcs.get(change).filter(|c| {
            c.lines.contains(&line) || (c.kind == ChangeKind::Deleted && c.lines.start == line)
        }) else {
            return;
        };
        let (b, layout, top) = (f.bounds, &f.layout, row.top);
        let bar_w = (3.0 * self.scale).round().max(2.0);
        let x = (b.min_x() + layout.gutter_width - layout.advance * 2.3).round();
        let (rect, color) = match c.kind {
            ChangeKind::Deleted => {
                let h = (layout.line_height * 0.3).round();
                (Rect::new(x, (top - h / 2.0).round(), bar_w * 2.0, h), palette.vcs_deleted)
            }
            ChangeKind::Added => (Rect::new(x, top, bar_w, layout.line_height), palette.vcs_added),
            ChangeKind::Modified => {
                (Rect::new(x, top, bar_w, layout.line_height), palette.vcs_modified)
            }
        };
        let radius = if c.kind == ChangeKind::Deleted { rect.size.height / 2.0 } else { 1.0 };
        gutter.quad(Quad::filled(rect, color).rounded(radius));
        if let Some(rect) = rect.intersect(&f.bounds) {
            self.vcs_rects.push((rect, change));
        }
    }

    /// A statement a module marks (a console's, a `.http` file's requests), on its first
    /// line, left of the number: a ▶ while it can run, the accent's ring while it runs, a
    /// dot green once done, red when it failed. Clickable (its module decides what it does).
    pub(super) fn paint_statement_mark(
        &mut self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        palette: &Palette,
    ) {
        use ui_components::editor_toolbar::MarkState;
        let line = row.line as usize;
        let marks = f.deco.statements;
        let at = marks.partition_point(|m| m.lines.start < line);
        let Some(mark) = marks.get(at).filter(|m| m.lines.start == line) else { return };
        let (b, layout) = (f.bounds, &f.layout);
        let size = (layout.line_height * 0.42).round().max(4.0);
        // Clear of the current statement's bar (at the gutter's left edge), never touching.
        let (bar_x, bar_w) = current_bar(layout.annotation, self.scale);
        let left = (layout.annotation + layout.advance * 0.7).max(bar_x + bar_w + 4.0 * self.scale);
        let r = Rect::new(
            (b.min_x() + left).round(),
            (row.top + (layout.line_height - size) / 2.0).round(),
            size,
            size,
        );
        let line_w = (1.5 * self.scale).round().max(1.0);
        // A little larger than the dot, to click.
        let pad = (3.0 * self.scale).round();
        let hit = Rect::new(r.min_x() - pad, row.top, size + 2.0 * pad, layout.line_height);
        if let Some(hit) = hit.intersect(&f.bounds) {
            self.statement_rects.push((hit, row.line));
        }
        if mark.state == MarkState::Runnable {
            let font = Font { size: (layout.line_height * 0.6).round(), ..f.ui_font.clone() };
            let shaped = text.shape_line("\u{25B6}", &font);
            let x = (r.min_x() + (size - shaped.width) / 2.0).round();
            ui_kit::draw_text(gutter, &shaped, x, row.baseline, palette.vcs_added);
            return;
        }
        let quad = match mark.state {
            MarkState::Runnable => return,
            MarkState::Running => Quad::filled(r, palette.accent.with_alpha(0.18))
                .rounded(size / 2.0)
                .bordered(line_w, palette.accent),
            MarkState::Done => Quad::filled(r, palette.vcs_added).rounded(size / 2.0),
            MarkState::Failed => Quad::filled(r, palette.danger).rounded(size / 2.0),
        };
        gutter.quad(quad);
    }

    /// The folding arrow: visible (secondary text color), with a background on hover;
    /// collapsed regions use the accent color.
    pub(super) fn paint_fold_arrow(
        &self,
        f: &Frame<'_>,
        gutter: &mut render_api::Layer,
        row: Row,
        palette: &Palette,
    ) {
        let folds = f.deco.folds;
        let Some(fold) = folds.fold_at(row.line) else { return };
        let (b, layout, top) = (f.bounds, &f.layout, row.top);
        let collapsed = folds.is_collapsed(fold);
        let hovered = self.hovered_marker == Some(row.line);
        let size = (layout.line_height * 0.72).round();
        let cx = b.min_x() + layout.gutter_width - layout.advance * 1.2;
        if hovered {
            let bg = Rect::new(
                (cx - size / 2.0).round(),
                (top + (layout.line_height - size) / 2.0).round(),
                size,
                size,
            );
            crate::ds::RowHighlight::new(false, true)
                .hover(crate::ds::HoverFill::Color(palette.tab_hover))
                .radius(FOLD_HOVER_RADIUS)
                .paint(gutter, &crate::ds::Ctx::new(palette, self.scale), bg);
        }
        let color = if collapsed {
            palette.accent
        } else if hovered {
            palette.text
        } else {
            palette.text_muted
        };
        let center = Point::new(cx, top + layout.line_height / 2.0);
        draw_triangle(gutter, center, (layout.line_height * 0.36).round(), !collapsed, color);
    }
}

/// Filled triangle (▼ if `down`, otherwise ▶), drawn from one-pixel strips; the
/// fractional edges are smoothed by the shader, so it looks like a vector shape.
pub(super) fn draw_triangle(
    layer: &mut render_api::Layer,
    center: Point,
    size: f32,
    down: bool,
    color: Rgba,
) {
    let steps = size.max(2.0) as usize;
    let half = size / 2.0;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let extent = size * (1.0 - t);
        let quad = if down {
            // Horizontal rows, narrowing toward the bottom tip.
            Rect::new(
                center.x - extent / 2.0,
                center.y - half * 0.6 + i as f32 * 0.6,
                extent,
                0.6 + 0.01,
            )
        } else {
            // Vertical columns, shortening toward the right tip.
            Rect::new(center.x - half * 0.6 + i as f32 * 0.6, center.y - extent / 2.0, 0.61, extent)
        };
        layer.quad(Quad::filled(quad, color));
    }
}

/// The current statement's bar in the gutter: its left (from the gutter's left) and its
/// width — at the left edge, clear of the statement marks' dots.
fn current_bar(annotation: f32, scale: f32) -> (f32, f32) {
    (annotation + (1.0 * scale).round(), (3.0 * scale).round().max(3.0))
}

/// A fold marker's back under the mouse: its corners (logical pixels).
const FOLD_HOVER_RADIUS: f32 = 4.0;
