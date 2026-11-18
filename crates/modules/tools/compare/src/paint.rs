//! Painting the comparison: the columns and the composed result, the separators, the overview, the
//! toolbar, the steps and the save button.

use super::*;

impl Compare {
    /// Difference `change`'s choices, right-aligned on its first row in the result column:
    /// `‹ left`, `right ›`, `both`, `none` — the chosen one filled.
    pub(crate) fn choices(&mut self, pen: Pen<'_, '_>, mid: Rect, y: f32, change: usize) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let pick = self.picks.get(change).copied().unwrap_or_default();
        let small = ds.ui_font(-2.0);
        let options = [
            (format!("‹ {}", t("compare.left")), Pick::Left),
            (format!("{} ›", t("compare.right")), Pick::Right),
            (t("compare.both"), Pick::Both),
            (t("compare.none"), Pick::None),
        ];
        let shaped: Vec<_> = options.iter().map(|(l, _)| text.shape_line(l, &small)).collect();
        let pad = px(9.0);
        let total: f32 = shaped.iter().map(|s| s.width + pad * 2.0).sum::<f32>() + px(4.0);
        let h = (self.row_h - px(4.0)).max(px(16.0));
        let container = Rect::new(
            (mid.max_x() - px(8.0) - total).round(),
            (y + (self.row_h - h) / 2.0).round(),
            total.round(),
            h,
        );
        let current = change == self.current;
        layer.quad(
            Quad::filled(container, p.popup_background)
                .rounded(h / 2.0)
                .bordered(ds.line(), if current { p.accent.with_alpha(0.55) } else { p.border }),
        );
        let mut x = container.min_x() + px(2.0);
        for ((_, value), shaped) in options.iter().zip(&shaped) {
            let w = shaped.width + pad * 2.0;
            let chip = Rect::new(x, container.min_y() + px(2.0), w, h - px(4.0));
            let on = pick == *value;
            let hovered = self.hover == Some(Hit::Pick(change, *value));
            let fill = if on {
                p.accent
            } else if hovered {
                p.text.with_alpha(0.10)
            } else {
                Rgba::TRANSPARENT
            };
            layer.quad(Quad::filled(chip, fill).rounded(chip.size.height / 2.0));
            draw_text(
                layer,
                shaped,
                (chip.min_x() + pad).round(),
                ds::optical_baseline(chip.min_y(), chip.size.height, &small),
                if on { Rgba::WHITE } else { p.text_muted },
            );
            if let Some(hit) = chip.intersect(&self.list) {
                self.hits.push((hit, Hit::Pick(change, *value)));
            }
            x += w;
        }
    }

    /// The columns: left | (result |) right, with a gutter between them (a splitter).
    pub(crate) fn columns(&mut self, ds: &Ctx<'_>) -> Columns {
        let gutter = ds.px(30.0);
        let columns = if self.merge { 3 } else { 2 };
        let columns_w = (self.list.size.width - gutter * (columns - 1) as f32).max(0.0);
        self.gutter = gutter;
        self.columns_w = columns_w;
        let ends = self.ends();
        let left_w = (ends[0] * columns_w).floor();
        let left = Rect::new(self.list.min_x(), self.list.min_y(), left_w, self.list.size.height);
        let (mid, right) = if self.merge {
            let mid = Rect::new(
                left.max_x() + gutter,
                self.list.min_y(),
                ((ends[1] - ends[0]) * columns_w).floor(),
                self.list.size.height,
            );
            (
                Some(mid),
                Rect::new(
                    mid.max_x() + gutter,
                    self.list.min_y(),
                    self.list.max_x() - mid.max_x() - gutter,
                    self.list.size.height,
                ),
            )
        } else {
            (
                None,
                Rect::new(
                    left.max_x() + gutter,
                    self.list.min_y(),
                    self.list.max_x() - left.max_x() - gutter,
                    self.list.size.height,
                ),
            )
        };
        Columns { left, mid, right, gutter }
    }

    /// A column's visible rows (`which`: 0 left, 1 result, 2 right): each cell tinted by its
    /// difference, the current one marked by a border.
    pub(crate) fn paint_column(
        &self,
        pen: Pen<'_, '_>,
        grid: &Grid,
        rows: &[Row],
        which: usize,
        col: Rect,
        pass: Pass,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let green = p.terminal_ansi[2];
        let red = p.danger;
        for (ri, row) in rows.iter().enumerate().skip(grid.first).take(grid.visible) {
            let y = (self.list.min_y() + ri as f32 * self.row_h - self.scroll).round();
            let base = ds::optical_baseline(y, self.row_h, &grid.font);
            let changed = row.change.is_some();
            let is_current = row.change == Some(self.current);
            let left_text = row.left.map(|i| self.left[i].as_str());
            let right_text = row.right.map(|i| self.right[i].as_str());
            let inline = match (changed, left_text, right_text) {
                (true, Some(l), Some(r)) => Some(inline_change(l, r)),
                _ => None,
            };
            let neutral = p.text.with_alpha(0.03);
            let (number, source, tint, inline, number_w) = match which {
                0 => {
                    let tint = changed.then(|| match (row.left, row.right) {
                        (Some(_), Some(_)) => p.accent.with_alpha(0.08),
                        (Some(_), None) => red.with_alpha(0.10),
                        _ => neutral,
                    });
                    (row.left, left_text, tint, inline.map(|i| (i.0, red)), grid.number_w)
                }
                2 => {
                    let tint = changed.then(|| match (row.left, row.right) {
                        (Some(_), Some(_)) => p.accent.with_alpha(0.08),
                        (None, Some(_)) => green.with_alpha(0.12),
                        _ => neutral,
                    });
                    (row.right, right_text, tint, inline.map(|i| (i.1, green)), grid.number_w)
                }
                _ => {
                    // A conflict still to choose: in the danger color.
                    let open = row
                        .change
                        .is_some_and(|c| self.resolving.as_ref().is_some_and(|r| r.open(c)));
                    let color = if open { red } else { p.accent };
                    let tint =
                        changed.then(|| color.with_alpha(if is_current { 0.14 } else { 0.08 }));
                    (None, row.result.as_deref(), tint, None, px(12.0))
                }
            };
            paint_cell(
                Pen { layer, text, ds },
                CellSpec {
                    pass,
                    hscroll: self.across[which].offset.round(),
                    extension: self.extension(),
                    col,
                    y,
                    row_h: self.row_h,
                    number,
                    source,
                    tint,
                    inline,
                    number_w,
                    font: &grid.font,
                    number_font: &grid.number_font,
                    base,
                },
            );
            // Border of the current difference.
            if pass == Pass::Back && is_current && changed {
                layer.quad(Quad::filled(Rect::new(col.min_x(), y, px(3.0), self.row_h), p.accent));
            }
        }
    }

    /// The separators between the columns (under the buttons that sit on them); the second
    /// only while composing.
    pub(crate) fn paint_splitters(
        &mut self,
        scene: &mut render_api::Scene,
        columns: &Columns,
        p: &Palette,
        scale: f32,
    ) {
        let gutters = [columns.mid.unwrap_or(columns.right), columns.right];
        let style = SplitterStyle::from_palette(p);
        for (i, splitter) in self.splitters.iter_mut().enumerate() {
            let shown = i == 0 || self.merge;
            let area = if shown {
                Rect::new(
                    gutters[i].min_x() - columns.gutter,
                    self.list.min_y(),
                    columns.gutter,
                    self.list.size.height,
                )
            } else {
                Rect::default()
            };
            splitter.set_style(style.clone());
            splitter.set_bounds(area, scale);
            if shown {
                splitter.paint(scene);
            }
        }
    }

    /// The choices of the current difference and of the one under the mouse, in the result
    /// column `mid` (those scrolled out of sight skipped).
    pub(crate) fn paint_compose(&mut self, pen: Pen<'_, '_>, mid: Rect) {
        let Pen { layer, text, ds } = pen;
        let hovered = match self.hover {
            Some(Hit::Select(c) | Hit::Pick(c, _)) => Some(c),
            _ => None,
        };
        let mut shown: Vec<usize> = vec![self.current];
        shown.extend(hovered.filter(|&c| c != self.current));
        for change in shown {
            let Some(&row) = self.change_rows.get(change) else { continue };
            let y = (self.list.min_y() + row as f32 * self.row_h - self.scroll).round();
            if y + self.row_h < self.list.min_y() || y > self.list.max_y() {
                continue;
            }
            self.choices(Pen { layer, text, ds }, mid, y, change);
        }
    }

    /// Right strip: where the differences are across the whole file, and the part in view.
    pub(crate) fn paint_overview(&mut self, overview: &mut Layer, ds: &Ctx<'_>) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        overview.quad(Quad::filled(self.overview, p.text.with_alpha(0.025)));
        let total = self.rows.len().max(1) as f32;
        for (i, &row) in self.change_rows.iter().enumerate() {
            let len = self.rows[row..].iter().take_while(|r| r.change == Some(i)).count().max(1);
            let y = self.overview.min_y() + row as f32 / total * self.overview.size.height;
            let h = (len as f32 / total * self.overview.size.height).max(px(3.0));
            let color = if i == self.current { p.accent } else { p.accent.with_alpha(0.45) };
            overview.quad(
                Quad::filled(
                    Rect::new(
                        self.overview.min_x() + px(3.0),
                        y,
                        self.overview.size.width - px(6.0),
                        h,
                    ),
                    color,
                )
                .rounded(px(2.0)),
            );
        }
        overview.quad(Quad::filled(self.overview_thumb(), p.text.with_alpha(0.06)));
        self.hits.push((self.overview, Hit::Overview));
    }

    pub(crate) fn paint_toolbar(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        bar: Rect,
    ) {
        ds::divider(layer, ds, bar.min_x(), bar.max_x(), bar.max_y());
        let mid = bar.min_y() + bar.size.height / 2.0;
        let x = self.paint_title(Pen { layer, text, ds }, bar);
        self.paint_steps(Pen { layer, text, ds }, x, mid);
        // On the right: the mode, then (in "Compose") where to write and "all from…".
        let mut right = bar.max_x() - ds.px(16.0);
        // Composing: the result saved into its file (⌘S), lit once something was chosen
        // (resolving: once no conflict is left).
        if self.merge && self.writable() {
            self.paint_save(Pen { layer, text, ds }, mid, &mut right);
        }
        if self.writable() && self.resolving.is_none() {
            let items = [
                (t("compare.diff"), Hit::Mode(false), !self.merge),
                (t("compare.merge"), Hit::Mode(true), self.merge),
            ];
            self.paint_segmented(Pen { layer, text, ds }, mid, &mut right, &items);
        }
        // With a read-only side there is only one file to write to: no choice shown.
        if self.merge && !self.read_only.contains(&true) {
            let items = [
                (
                    format!("{} {}", t("compare.write_to"), t("compare.left")),
                    Hit::Target(Side::Left),
                    self.target() == Side::Left,
                ),
                (t("compare.right"), Hit::Target(Side::Right), self.target() == Side::Right),
            ];
            self.paint_segmented(Pen { layer, text, ds }, mid, &mut right, &items);
        }
        if self.resolving.is_some() {
            let writing = self.resolving.as_ref().is_some_and(|r| r.by_hand.is_some());
            let items = [(t("compare.by_hand"), Hit::ByHand, writing)];
            self.paint_segmented(Pen { layer, text, ds }, mid, &mut right, &items);
        }
        if self.merge && !self.resolving.as_ref().is_some_and(|r| r.by_hand.is_some()) {
            let items = [
                (t("compare.all_left"), Hit::AllLeft, false),
                (t("compare.all_right"), Hit::AllRight, false),
            ];
            self.paint_segmented(Pen { layer, text, ds }, mid, &mut right, &items);
        }
    }

    /// `a.php ↔ b.php · 3 differences`, from the bar's left; returns where it ends.
    pub(crate) fn paint_title(&self, pen: Pen<'_, '_>, bar: Rect) -> f32 {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let mut x = bar.min_x() + px(16.0);
        for (i, path) in [&self.left_path, &self.right_path].into_iter().enumerate() {
            if i == 1 {
                let arrow = text.shape_line("↔", &font);
                draw_text(
                    layer,
                    &arrow,
                    x,
                    ds::optical_baseline(bar.min_y(), bar.size.height, &font),
                    p.text_muted,
                );
                x += arrow.width + px(8.0);
            }
            let label = self.labels[i].clone().unwrap_or_else(|| file_name(path));
            let name = text.shape_line(&label, &strong);
            draw_text(
                layer,
                &name,
                x,
                ds::optical_baseline(bar.min_y(), bar.size.height, &strong),
                p.text,
            );
            x += name.width + px(8.0);
        }
        let summary = match &self.resolving {
            Some(resolving) => resolving_status(resolving),
            None => tr!("compare.changes", count = self.changes() as i64),
        };
        let count = text.shape_line(&format!("· {summary}"), &font);
        draw_text(
            layer,
            &count,
            x,
            ds::optical_baseline(bar.min_y(), bar.size.height, &font),
            p.text_muted,
        );
        x + count.width + px(12.0)
    }

    /// Previous / next difference, from `x`.
    pub(crate) fn paint_steps(&mut self, pen: Pen<'_, '_>, mut x: f32, mid: f32) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        for (label, hit) in [("↑", Hit::Prev), ("↓", Hit::Next)] {
            let r = Rect::new(x, mid - px(13.0), px(26.0), px(26.0));
            if self.hover == Some(hit) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.07)).rounded(px(6.0)));
            }
            let shaped = text.shape_line(label, &font);
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
                ds::optical_baseline(r.min_y(), r.size.height, &font),
                p.text,
            );
            self.hits.push((r, hit));
            x += px(28.0);
        }
    }

    /// "Save to …  ⌘S", left of `right` (moved past it): lit once something was chosen.
    pub(crate) fn paint_save(&mut self, pen: Pen<'_, '_>, mid: f32, right: &mut f32) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let path = match (&self.resolving, self.target()) {
            (Some(resolving), _) => &resolving.result,
            (None, Side::Left) => &self.left_path,
            (None, Side::Right) => &self.right_path,
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let label = format!("{}  ⌘S", tr!("compare.save_to", name = name));
        let small = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        let shaped = text.shape_line(&label, &small);
        let h = px(28.0);
        let button =
            Rect::new(*right - shaped.width - px(24.0), mid - h / 2.0, shaped.width + px(24.0), h);
        let dirty = self.can_save();
        let (fill, color) = if dirty {
            let fill =
                if self.hover == Some(Hit::Save) { p.accent.with_alpha(0.85) } else { p.accent };
            (fill, Rgba::WHITE)
        } else {
            (p.text.with_alpha(0.05), p.text_muted)
        };
        layer.quad(Quad::filled(button, fill).rounded(px(8.0)));
        draw_text(
            layer,
            &shaped,
            (button.min_x() + px(12.0)).round(),
            ds::optical_baseline(button.min_y(), h, &small),
            color,
        );
        if dirty {
            self.hits.push((button, Hit::Save));
        }
        *right = button.min_x() - px(12.0);
    }

    /// A segmented control ending at `right` (moved past it): its items (label, what a click
    /// does, chosen).
    pub(crate) fn paint_segmented(
        &mut self,
        pen: Pen<'_, '_>,
        mid: f32,
        right: &mut f32,
        items: &[(String, Hit, bool)],
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let small = ds.ui_font(-1.0);
        let widths: Vec<f32> =
            items.iter().map(|(l, _, _)| text.shape_line(l, &small).width + px(20.0)).collect();
        let total: f32 = widths.iter().sum::<f32>() + px(4.0);
        let h = px(28.0);
        let seg = Rect::new(*right - total, mid - h / 2.0, total, h);
        layer.quad(Quad::filled(seg, p.text.with_alpha(0.05)).rounded(px(8.0)));
        let mut sx = seg.min_x() + px(2.0);
        for ((label, hit, on), w) in items.iter().zip(&widths) {
            let r = Rect::new(sx, seg.min_y() + px(2.0), *w, h - px(4.0));
            if *on {
                layer.quad(
                    Quad::filled(r, p.popup_background)
                        .rounded(px(6.0))
                        .bordered(ds.line(), p.border),
                );
            } else if self.hover == Some(*hit) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.05)).rounded(px(6.0)));
            }
            let shaped = text.shape_line(label, &small);
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
                ds::optical_baseline(r.min_y(), r.size.height, &small),
                if *on { p.text } else { p.text_muted },
            );
            self.hits.push((r, *hit));
            sx += w;
        }
        *right = seg.min_x() - px(12.0);
    }
}

/// A cell: line number, background (difference), colored text and the changed part.
/// `range` of line `raw` where it is once the line's tabs are four spaces (each tab before a
/// place: three bytes more); `None`: not a range of the line.
pub(crate) fn tab_shifted(
    raw: &str,
    range: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    if range.end > raw.len()
        || !raw.is_char_boundary(range.start)
        || !raw.is_char_boundary(range.end)
    {
        return None;
    }
    let shown = |at: usize| at + 3 * raw.as_bytes()[..at].iter().filter(|b| **b == b'\t').count();
    Some(shown(range.start)..shown(range.end))
}

pub(crate) fn paint_cell(pen: Pen<'_, '_>, spec: CellSpec<'_>) {
    let Pen { layer, text, ds } = pen;
    let CellSpec {
        pass,
        hscroll,
        extension,
        col,
        y,
        row_h,
        number,
        source,
        tint,
        inline,
        number_w,
        font,
        number_font,
        base,
    } = spec;
    let p: &Palette = ds.palette;
    if pass == Pass::Back {
        let cell = Rect::new(col.min_x(), y, col.size.width, row_h);
        if let Some(tint) = tint {
            layer.quad(Quad::filled(cell, tint));
        }
        if let Some(n) = number {
            let shaped = text.shape_line(&(n + 1).to_string(), number_font);
            draw_text(
                layer,
                &shaped,
                col.min_x() + number_w - ds.px(8.0) - shaped.width,
                base,
                p.gutter_text,
            );
        }
        // Missing row (alignment): subtle hatching.
        if source.is_none() && tint.is_some() {
            let step = ds.px(8.0);
            let mut x = col.min_x() + number_w;
            while x < col.max_x() {
                layer.quad(Quad::filled(
                    Rect::new(x, y + row_h / 2.0, ds.px(4.0), ds.line()),
                    p.text.with_alpha(0.10),
                ));
                x += step;
            }
        }
        return;
    }
    let Some(source) = source else { return };
    // Scrolled across (its layer clips it after the line numbers).
    let x0 = col.min_x() + number_w - hscroll;
    let inline = inline.and_then(|(range, color)| Some((tab_shifted(source, range)?, color)));
    let source = source.replace('\t', "    ");
    // The changed part of the line, slightly more intense.
    if let Some((range, color)) = inline
        && range.end > range.start
        && range.end <= source.len()
        && source.is_char_boundary(range.start)
        && source.is_char_boundary(range.end)
    {
        let before = text.shape_line(&source[..range.start], font).width;
        let width = text.shape_line(&source[range], font).width;
        layer.quad(
            Quad::filled(
                Rect::new(x0 + before, y + ds.px(2.0), width, row_h - ds.px(4.0)),
                color.with_alpha(0.22),
            )
            .rounded(ds.px(3.0)),
        );
    }
    ui_components::draw_code(
        layer,
        text,
        ui_components::CodeLine {
            source: &source,
            extension,
            font,
            x: x0,
            baseline: base,
            palette: p,
            max_x: col.max_x() - ds.px(6.0),
        },
    );
}
