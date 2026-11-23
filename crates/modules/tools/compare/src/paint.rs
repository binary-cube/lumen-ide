//! Painting the comparison: the columns and the composed result, the separators, the overview, the
//! toolbar, the steps and the save button.

use super::*;
use ui_components::ds::{ButtonKind, ButtonSpec, SegmentedSpec, button_size};

/// The room a difference's choices leave in its row (logical pixels).
const CHOICES_INSET: f32 = 4.0;

impl Compare {
    /// Difference `change`'s choices, right-aligned on its first row in the result column:
    /// `‹ left`, `right ›`, `both`, `none` — the shared segmented control, the chosen one
    /// raised.
    pub(crate) fn choices(
        &mut self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        (mid, y): (Rect, f32),
        change: usize,
    ) {
        let Pen { layer, text, ds } = pen;
        let pick = self.picks.get(change).copied().unwrap_or_default();
        let options = [
            (format!("‹ {}", t("compare.left")), Pick::Left),
            (format!("{} ›", t("compare.right")), Pick::Right),
            (t("compare.both"), Pick::Both),
            (t("compare.none"), Pick::None),
        ];
        let labels = options.each_ref().map(|(l, _)| l.as_str());
        // None chosen (a conflict waiting): past the last, so none raised.
        let selected = options.iter().position(|(_, v)| *v == pick).unwrap_or(options.len());
        let hovered = options.iter().position(|(_, v)| self.hover == Some(Hit::Pick(change, *v)));
        let spec = SegmentedSpec {
            right: (mid.max_x() - ds.px(8.0)).round(),
            mid: y + self.row_h / 2.0,
            labels: &labels,
            selected,
            hovered,
        };
        // As high as a row lets it, within the small sizes.
        let height = (self.row_h / ds.scale.max(0.01) - CHOICES_INSET)
            .clamp(button_size::SMALL, button_size::COMPACT);
        let look = ds::SegmentedLook { height, ..ds::SegmentedLook::default() };
        let (hits, list) = (&mut self.hits, self.list);
        ds::segmented_styled(layer, text, icons, ds, (spec, look), |i, seg| {
            if let (Some((_, value)), Some(hit)) = (options.get(i), seg.intersect(&list)) {
                hits.push((hit, Hit::Pick(change, *value)));
            }
        });
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
        let wrap = if self.wraps() { self.wrap_columns[which] } else { 0 };
        for (ri, row) in rows.iter().enumerate().skip(grid.first).take(grid.visible) {
            let y = (self.list.min_y() + self.row_top(ri) - self.scroll).round();
            let height = self.row_height(ri);
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
                    height,
                    wrap,
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
                layer.quad(Quad::filled(Rect::new(col.min_x(), y, px(3.0), height), p.accent));
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
    pub(crate) fn paint_compose(&mut self, pen: Pen<'_, '_>, icons: &icons::Icons, mid: Rect) {
        let Pen { layer, text, ds } = pen;
        let hovered = match self.hover {
            Some(Hit::Select(c) | Hit::Pick(c, _)) => Some(c),
            _ => None,
        };
        let mut shown: Vec<usize> = vec![self.current];
        shown.extend(hovered.filter(|&c| c != self.current));
        for change in shown {
            let Some(&row) = self.change_rows.get(change) else { continue };
            let y = (self.list.min_y() + self.row_top(row) - self.scroll).round();
            if y + self.row_h < self.list.min_y() || y > self.list.max_y() {
                continue;
            }
            self.choices(Pen { layer, text, ds }, icons, (mid, y), change);
        }
    }

    /// Right strip: where the differences are across the whole file, and the part in view.
    pub(crate) fn paint_overview(&mut self, overview: &mut Layer, ds: &Ctx<'_>) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        overview.quad(Quad::filled(self.overview, p.text.with_alpha(0.025)));
        // Along the rows' height (wrapped rows taller), as the thumb is.
        let total = self.rows_height().max(self.row_h).max(1.0);
        for (i, &row) in self.change_rows.iter().enumerate() {
            let len = self.rows[row..].iter().take_while(|r| r.change == Some(i)).count().max(1);
            let (top, bottom) = (self.row_top(row), self.row_top(row + len));
            let y = self.overview.min_y() + top / total * self.overview.size.height;
            let h = ((bottom - top) / total * self.overview.size.height).max(px(3.0));
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
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        bar: Rect,
    ) {
        ds::divider(layer, ds, bar.min_x(), bar.max_x(), bar.max_y());
        let mid = bar.min_y() + bar.size.height / 2.0;
        let x = self.paint_title(Pen { layer, text, ds }, bar);
        self.paint_steps(Pen { layer, text, ds }, icons, x, mid);
        // On the right: the mode, then (in "Compose") where to write and "all from…".
        let mut right = bar.max_x() - ds.px(16.0);
        // Composing: the result saved into its file (⌘S), lit once something was chosen
        // (resolving: once no conflict is left).
        if self.merge && self.writable() {
            self.paint_save(Pen { layer, text, ds }, icons, mid, &mut right);
        }
        if self.writable() && self.resolving.is_none() {
            let items =
                [(t("compare.diff"), Hit::Mode(false)), (t("compare.merge"), Hit::Mode(true))];
            self.paint_segmented(Pen { layer, text, ds }, mid, &mut right, &items, self.merge);
        }
        // With a read-only side there is only one file to write to: no choice shown.
        if self.merge && !self.read_only.contains(&true) {
            let items = [
                (
                    format!("{} {}", t("compare.write_to"), t("compare.left")),
                    Hit::Target(Side::Left),
                ),
                (t("compare.right"), Hit::Target(Side::Right)),
            ];
            let right_chosen = self.target() == Side::Right;
            self.paint_segmented(Pen { layer, text, ds }, mid, &mut right, &items, right_chosen);
        }
        if self.resolving.is_some() {
            let writing = self.resolving.as_ref().is_some_and(|r| r.by_hand.is_some());
            let items = [(t("compare.by_hand"), Hit::ByHand, writing)];
            self.paint_actions(Pen { layer, text, ds }, icons, mid, &mut right, &items);
        }
        if self.merge && !self.resolving.as_ref().is_some_and(|r| r.by_hand.is_some()) {
            let items = [
                (t("compare.all_left"), Hit::AllLeft, false),
                (t("compare.all_right"), Hit::AllRight, false),
            ];
            self.paint_actions(Pen { layer, text, ds }, icons, mid, &mut right, &items);
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
    pub(crate) fn paint_steps(
        &mut self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        mut x: f32,
        mid: f32,
    ) {
        let Pen { layer, text, ds } = pen;
        for (label, hit) in [("↑", Hit::Prev), ("↓", Hit::Next)] {
            let r = ds::square_button(ds, (x, mid), button_size::MEDIUM);
            let spec =
                ButtonSpec::new(ButtonKind::Ghost).label(label).hovered(self.hover == Some(hit));
            ds::button(layer, text, icons, ds, r, spec);
            self.hits.push((r, hit));
            x += ds.px(STEP_ADVANCE);
        }
    }

    /// "Save to …  ⌘S", left of `right` (moved past it): lit once something was chosen.
    pub(crate) fn paint_save(
        &mut self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        mid: f32,
        right: &mut f32,
    ) {
        let Pen { layer, text, ds } = pen;
        let path = match (&self.resolving, self.target()) {
            (Some(resolving), _) => &resolving.result,
            (None, Side::Left) => &self.left_path,
            (None, Side::Right) => &self.right_path,
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let label = format!("{}  ⌘S", tr!("compare.save_to", name = name));
        let dirty = self.can_save();
        let spec = ButtonSpec::new(ButtonKind::Primary)
            .label(&label)
            .enabled(dirty)
            .hovered(self.hover == Some(Hit::Save));
        let h = ds.px(button_size::LARGE);
        let w = ds::button_width(text, ds, &spec, h);
        let button = Rect::new(*right - w, mid - h / 2.0, w, h);
        ds::button(layer, text, icons, ds, button, spec);
        if dirty {
            self.hits.push((button, Hit::Save));
        }
        *right = button.min_x() - ds.px(GROUP_GAP);
    }

    /// The shared segmented control ending at `right` (moved past it): its items (label,
    /// what a click does), the second chosen when `second`.
    pub(crate) fn paint_segmented(
        &mut self,
        pen: Pen<'_, '_>,
        mid: f32,
        right: &mut f32,
        items: &[(String, Hit); 2],
        second: bool,
    ) {
        let Pen { layer, text, ds } = pen;
        let labels = [items[0].0.as_str(), items[1].0.as_str()];
        let hovered = items.iter().position(|(_, hit)| self.hover == Some(*hit));
        let spec = SegmentedSpec {
            right: *right,
            mid,
            labels: &labels,
            selected: usize::from(second),
            hovered,
        };
        let width = ds::segmented_width(text, ds, &labels);
        let hits = &mut self.hits;
        ds::segmented_with(layer, text, ds, spec, |i, r| hits.push((r, items[i].1)));
        *right -= width + ds.px(GROUP_GAP);
    }

    /// Soft buttons side by side ending at `right` (moved past them): their label, what a
    /// click does, turned on.
    pub(crate) fn paint_actions(
        &mut self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        mid: f32,
        right: &mut f32,
        items: &[(String, Hit, bool)],
    ) {
        let Pen { layer, text, ds } = pen;
        let h = ds.px(button_size::LARGE);
        let gap = ds.px(ACTION_GAP);
        let mut x = *right;
        for (label, hit, on) in items.iter().rev() {
            let spec = ButtonSpec::new(ButtonKind::Soft)
                .label(label)
                .toggle(*on)
                .hovered(self.hover == Some(*hit));
            let w = ds::button_width(text, ds, &spec, h);
            let r = Rect::new(x - w, mid - h / 2.0, w, h);
            ds::button(layer, text, icons, ds, r, spec);
            self.hits.push((r, *hit));
            x = r.min_x() - gap;
        }
        *right = x + gap - ds.px(GROUP_GAP);
    }
}

/// How far one step button is from the next (logical pixels): its 26 and a 2 apart.
const STEP_ADVANCE: f32 = 28.0;
/// The room between two groups of the toolbar (logical pixels).
const GROUP_GAP: f32 = 12.0;
/// The room between two action buttons of one group (logical pixels).
const ACTION_GAP: f32 = 4.0;

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
        height,
        wrap,
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
        let cell = Rect::new(col.min_x(), y, col.size.width, height);
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
    // Its rows: one, or the parts it wraps into at `wrap` columns.
    let mut parts = Vec::new();
    if wrap > 0 {
        ui_components::text_options::wrap_rows(&source, wrap, 4, &mut parts);
    } else {
        parts.push(0..source.len());
    }
    let max_x = col.max_x() - ds.px(6.0);
    for (k, part) in parts.iter().enumerate() {
        let top = y + k as f32 * row_h;
        let baseline = base + k as f32 * row_h;
        // The changed part of the line (its share of this row), slightly more intense.
        if let Some((range, color)) = &inline {
            let (from, to) = (range.start.max(part.start), range.end.min(part.end));
            if to > from
                && to <= source.len()
                && source.is_char_boundary(from)
                && source.is_char_boundary(to)
            {
                let before = text.shape_line(&source[part.start..from], font).width;
                let width = text.shape_line(&source[from..to], font).width;
                layer.quad(
                    Quad::filled(
                        Rect::new(x0 + before, top + ds.px(2.0), width, row_h - ds.px(4.0)),
                        color.with_alpha(0.22),
                    )
                    .rounded(ds.px(3.0)),
                );
            }
        }
        let line = ui_components::CodeLine {
            source: &source,
            extension,
            font,
            x: x0,
            baseline,
            palette: p,
            max_x,
        };
        if parts.len() == 1 {
            ui_components::draw_code(layer, text, line);
            continue;
        }
        ui_components::draw_code_part(layer, text, line, part.clone());
        // The line goes on below: the wrap's mark after this row's text.
        if k + 1 < parts.len() {
            let width = text.shape_line(&source[part.clone()], font).width;
            let mark = text.shape_line(WRAP_END, font);
            draw_text(
                layer,
                &mark,
                x0 + width + ds.px(4.0),
                baseline,
                p.text_muted.with_alpha(0.6),
            );
        }
    }
}

/// The mark at a wrapped row's end (its line goes on below).
const WRAP_END: &str = "↩";
