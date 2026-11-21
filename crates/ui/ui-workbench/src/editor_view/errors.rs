//! Errors and problems: the marks under the code and in the scrollbar's strip, the preview card of
//! a problem, the tooltip.

use super::*;
use kernel::text::one_line;

impl EditorView {
    /// The error whose mark is under the mouse (offset), for the code preview.
    pub fn hovered_error(&self) -> Option<usize> {
        self.hovered_error.map(|(offset, _)| offset)
    }

    pub(super) fn error_mark_hit(&self, p: Point) -> Option<(usize, f32)> {
        if !self.vbar.contains(p) {
            return None;
        }
        let tolerance = 5.0 * self.scale;
        self.error_marks
            .iter()
            .filter(|(y, _)| (p.y - y).abs() <= tolerance)
            .min_by(|a, b| (p.y - a.0).abs().total_cmp(&(p.y - b.0).abs()))
            .map(|(y, offset)| (*offset, *y))
    }

    /// The mark (error or occurrence) on the scroll strip under point `p` (its offset).
    pub fn error_mark_at(&self, p: Point) -> Option<usize> {
        if !self.vbar.contains(p) {
            return None;
        }
        let tolerance = 4.0 * self.scale;
        self.error_marks
            .iter()
            .chain(&self.occurrence_marks)
            .filter(|(y, _)| (p.y - y).abs() <= tolerance)
            .min_by(|a, b| (p.y - a.0).abs().total_cmp(&(p.y - b.0).abs()))
            .map(|(_, offset)| *offset)
    }

    /// Preview of an error's range, next to its mark on the scroll strip: a card that
    /// fades in, with an arrow toward the mark; the header (message, code in a pill, line) and
    /// a few surrounding lines of code, with syntax colors, the error line marked.
    pub fn paint_error_preview(
        &self,
        doc: &Document,
        spec: ErrorPreviewSpec<'_>,
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, .. } = painter;
        let Some((_, mark_y)) = self.hovered_error else { return };
        let t = self.preview.map_or(1.0, |(_, tween)| tween.value());
        let p = self.measure_preview(doc, spec, text, palette, t);
        let card = self.preview_card(&p, mark_y);
        // The shadow (two soft layers), the card and the arrow toward the mark.
        let layer = scene.push_layer(None);
        p.paint_surface(layer, card, mark_y, palette);
        let layer = scene.push_layer(Some(card));
        p.paint_header(layer, card, text, palette);
        p.paint_code(layer, card, text, palette);
    }

    /// An error's preview, measured: the code lines around it (shaped) and the header.
    pub(super) fn measure_preview<'a>(
        &self,
        doc: &Document,
        spec: ErrorPreviewSpec<'a>,
        text: &mut TextSystem,
        palette: &Palette,
        t: f32,
    ) -> Preview<'a> {
        let ErrorPreviewSpec { error, message, code, line_label, colors } = spec;
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let buffer = doc.buffer();
        let tab = doc.tab_width();
        let error_line = buffer.line_of(error.start.min(buffer.len()));
        let first = error_line.saturating_sub(2);
        let last = (error_line + 2).min(buffer.line_count().saturating_sub(1));

        let code_font = Font::new(self.font_family.clone(), px(palette.code_font_size - 1.0));
        let code_metrics = text.metrics(&code_font);
        let ui = Font::new(palette.ui_font.clone(), px(palette.ui_font_size));
        let ui_metrics = text.metrics(&ui);
        let small = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.5));
        let chip_font = Font::new(self.font_family.clone(), px(palette.code_font_size - 1.5));

        // The content: the code lines (measured) and the header.
        let mut display = String::new();
        let mut rows = Vec::new();
        let mut code_w: f32 = 0.0;
        for line in first..=last {
            let source = buffer.line_text(line);
            expand_tabs(&source, 200, tab, &mut display);
            let shaped = text.shape_line(&display, &code_font);
            code_w = code_w.max(shaped.width);
            let start = buffer.line_start(line);
            rows.push(PreviewRow { line, start, source, display: display.clone(), shaped });
        }
        // The common indentation is trimmed (the code sits right next to the line numbers).
        let indent = rows
            .iter()
            .filter(|r| !r.source.trim().is_empty())
            .map(|r| r.display.len() - r.display.trim_start().len())
            .min()
            .unwrap_or(0);
        let indent_x =
            rows.iter().find(|r| r.display.len() > indent).map_or(0.0, |r| r.shaped.x_at(indent));
        // Shaped as single lines: a newline in a message or a code (a parameter written over
        // two lines) would draw its second part back at the start, over the first.
        let msg = text.shape_line(&one_line(message), &ui);
        let chip_text = code.map(|c| one_line(c).into_owned());
        let chip = chip_text.as_ref().map(|c| text.shape_line(c, &chip_font));
        let label_text = one_line(line_label).into_owned();
        let label = text.shape_line(&label_text, &small);
        let pad = px(14.0);
        let icon = px(18.0);
        let gutter = px(40.0);
        let header_w = pad
            + icon
            + px(10.0)
            + msg.width
            + chip.as_ref().map_or(0.0, |c| c.width + px(18.0))
            + px(16.0)
            + label.width
            + px(16.0)
            + pad;
        let body_w = gutter + (code_w - indent_x) + 2.0 * pad;
        let width =
            header_w.max(body_w).clamp(px(320.0), px(620.0)).min(self.bounds.size.width - px(60.0));
        let header_h = px(42.0);
        let line_h = (code_font.size * 1.55).round();
        let body_h = (last - first + 1) as f32 * line_h + px(14.0);
        let height = header_h + body_h + px(8.0);
        Preview {
            t,
            scale: s,
            error,
            error_line,
            colors,
            tab,
            code_font,
            small,
            code_metrics,
            ui_metrics,
            chip_font,
            message: msg,
            chip_text,
            label_text,
            rows,
            indent_x,
            pad,
            icon,
            gutter,
            header_h,
            line_h,
            body_h,
            width,
            height,
        }
    }

    /// Where an error's preview goes: left of the scroll strip, its header by the mark,
    /// sliding in as it fades in; inside the editor.
    pub(super) fn preview_card(&self, p: &Preview<'_>, mark_y: f32) -> Rect {
        let px = |v: f32| (v * self.scale).round();
        let track = self.vbar.track_rect();
        let x = track.min_x() - px(12.0) - p.width + (1.0 - p.t) * px(10.0);
        let y = (mark_y - p.header_h / 2.0 - px(4.0)).clamp(
            self.bounds.min_y() + px(8.0),
            (self.bounds.max_y() - p.height - px(8.0)).max(self.bounds.min_y()),
        );
        Rect::new(x.round(), y.round(), p.width.round(), p.height.round())
    }

    /// An error's (or a warning's) tooltip, next to the mouse: the icon, the reason and
    /// the code in a pill.
    pub fn paint_error_tooltip(
        &self,
        at: Point,
        tip: ErrorTip<'_>,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let ErrorTip { message, code, warning, weak } = tip;
        let tone = match (weak, warning) {
            (true, _) => palette.text_muted,
            (false, true) => palette.warning,
            (false, false) => palette.danger,
        };
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let ui = Font::new(palette.ui_font.clone(), px(palette.ui_font_size));
        let um = text.metrics(&ui);
        let chip_font = Font::new(self.font_family.clone(), px(palette.code_font_size - 1.5));
        // On one line (see `measure_preview`).
        let msg = text.shape_line(&one_line(message), &ui);
        let chip = code.map(|c| text.shape_line(&one_line(c), &chip_font));
        let icon = px(16.0);
        let h = px(34.0);
        let w = px(12.0)
            + icon
            + px(8.0)
            + msg.width
            + chip.as_ref().map_or(0.0, |c| c.width + px(20.0))
            + px(14.0);
        let x = at.x.min(self.bounds.max_x() - w - px(8.0)).max(self.bounds.min_x() + px(4.0));
        let mut y = at.y + px(20.0);
        if y + h > self.bounds.max_y() {
            y = at.y - h - px(12.0);
        }
        let tip = Rect::new(x.round(), y.round(), w.round(), h);
        let layer = scene.push_layer(None);
        let ctx = crate::ds::Ctx::new(palette, s);
        crate::ds::surface(layer, &ctx, tip, crate::ds::Elevation::Low, crate::ds::radius::MD);
        let mid = tip.min_y() + h / 2.0;
        let bang = Font::new(palette.ui_font.clone(), px(10.0)).with_weight(Font::BOLD);
        let ic =
            error_mark(&bang, tone, TIP_MARK).paint(layer, text, &ctx, tip.min_x() + px(12.0), mid);
        let mut cx = ic.max_x() + px(8.0);
        draw_text(layer, &msg, cx, centered_baseline(tip.min_y(), h, &um), palette.text);
        cx += msg.width + px(8.0);
        if let Some(code) = code {
            code_chip(&one_line(code), &chip_font, tone).paint(layer, text, &ctx, cx, mid);
        }
    }

    /// Marks on the vertical strip (proportional line position): centered pills, larger
    /// on the wide strip. Warnings first, so errors on nearby lines stay on top; the
    /// error under the mouse gets a halo. Occurrences go on the strip's left.
    pub(super) fn paint_strip_marks(
        &mut self,
        f: &Frame<'_>,
        vtrack: Rect,
        wide: f32,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let (doc, deco) = (f.doc, &f.deco);
        let buffer = doc.buffer();
        self.error_marks.clear();
        self.occurrence_marks.clear();
        let lists = [deco.deprecated, deco.errors, deco.occurrences, deco.warnings];
        let key = mark_key(doc.version(), deco.marks_generation, lists);
        let mark_lines = self.mark_lines.get_or_update(key, || {
            lists.map(|list| {
                list.iter().map(|r| buffer.line_of(r.start.min(buffer.len())) as u32).collect()
            })
        });
        let [warn_lines, error_lines, occ_lines, other_lines] = mark_lines;
        let strip = Strip {
            track: vtrack,
            span: self.vbar.inner_span(),
            lines: buffer.line_count().max(1) as f32,
            wide,
            scale: self.scale,
        };
        let marks = scene.push_layer(Some(vtrack));
        let warnings = [(deco.deprecated, &warn_lines[..]), (deco.warnings, &other_lines[..])];
        paint_warning_marks(marks, &strip, warnings, &mut self.error_marks, palette);
        let hovered = self.hovered_error.map(|(offset, _)| offset);
        let errors = (deco.errors, &error_lines[..]);
        paint_error_marks(marks, &strip, errors, hovered, &mut self.error_marks, palette);
        let occurrences = (deco.occurrences, &occ_lines[..]);
        paint_occurrence_marks(marks, &strip, occurrences, &mut self.occurrence_marks, palette);
    }
}

/// An error's preview, measured: the fonts, the header's texts, the code rows (the common
/// indentation trimmed) and the card's sizes, in physical pixels.
pub(super) struct Preview<'a> {
    /// How far it has faded in (0 → 1).
    t: f32,
    scale: f32,
    error: Range<usize>,
    error_line: usize,
    colors: &'a [ColorSpan],
    tab: u32,
    code_font: Font,
    /// The line label's and the line numbers' font.
    small: Font,
    code_metrics: FontMetrics,
    ui_metrics: FontMetrics,
    chip_font: Font,
    message: Arc<ShapedLine>,
    /// The code shown in its chip, and the line's label, on one line.
    chip_text: Option<String>,
    label_text: String,
    rows: Vec<PreviewRow>,
    /// The width of the trimmed indentation.
    indent_x: f32,
    pad: f32,
    icon: f32,
    gutter: f32,
    header_h: f32,
    line_h: f32,
    body_h: f32,
    width: f32,
    height: f32,
}

/// A code line in an error's preview: its index, where it starts, its source and its
/// displayed text (tabs expanded), shaped.
pub(super) struct PreviewRow {
    line: usize,
    start: usize,
    source: String,
    display: String,
    shaped: Arc<ShapedLine>,
}

impl Preview<'_> {
    pub(super) fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// A color, faded in with the preview.
    pub(super) fn fade(&self, c: Rgba) -> Rgba {
        c.with_alpha(c.a * self.t)
    }

    /// The card (with its shadow) and the arrow toward the mark.
    pub(super) fn paint_surface(
        &self,
        layer: &mut render_api::Layer,
        card: Rect,
        mark_y: f32,
        palette: &Palette,
    ) {
        let s = self.scale;
        let line = s.round().max(1.0);
        let radius = self.px(12.0);
        let ctx = crate::ds::Ctx::new(palette, s).with_opacity(self.t);
        crate::ds::surface(layer, &ctx, card, crate::ds::Elevation::Medium, crate::ds::radius::LG);
        let arrow_h = self.px(7.0);
        let ay =
            mark_y.clamp(card.min_y() + radius + arrow_h, card.max_y() - radius - arrow_h).round();
        let mut i = -arrow_h;
        while i <= arrow_h {
            let w = (arrow_h - i.abs()).max(0.0);
            layer.quad(Quad::filled(
                Rect::new(card.max_x() - line, ay + i, w + line, line),
                self.fade(palette.popup_background),
            ));
            i += line;
        }
    }

    /// The header: the icon, the message, the code in a pill, the line on the right.
    pub(super) fn paint_header(
        &self,
        layer: &mut render_api::Layer,
        card: Rect,
        text: &mut TextSystem,
        palette: &Palette,
    ) {
        let px = |v: f32| self.px(v);
        let (pad, icon, header_h) = (self.pad, self.icon, self.header_h);
        let ctx = crate::ds::Ctx::new(palette, self.scale).with_opacity(self.t);
        let mid = card.min_y() + header_h / 2.0;
        let bang = Font::new(palette.ui_font.clone(), px(11.0)).with_weight(Font::BOLD);
        let mark = error_mark(&bang, palette.danger, icon / self.scale);
        let ic = mark.paint(layer, text, &ctx, card.min_x() + pad, mid);
        let mut cx = ic.max_x() + px(10.0);
        draw_text(
            layer,
            &self.message,
            cx,
            centered_baseline(card.min_y(), header_h, &self.ui_metrics),
            self.fade(palette.text),
        );
        cx += self.message.width + px(8.0);
        if let Some(code) = &self.chip_text {
            code_chip(code, &self.chip_font, palette.danger).paint(layer, text, &ctx, cx, mid);
        }
        crate::ds::Badge::new(&self.label_text)
            .font(&self.small)
            .baseline(crate::ds::BadgeBaseline::Metrics)
            .paint_right(layer, text, &ctx, card.max_x() - pad, mid);
    }

    /// The code: a subtle panel inside the card, with line numbers; the error line marked
    /// and underlined.
    pub(super) fn paint_code(
        &self,
        layer: &mut render_api::Layer,
        card: Rect,
        text: &mut TextSystem,
        palette: &Palette,
    ) {
        let px = |v: f32| self.px(v);
        let (line_h, cm) = (self.line_h, &self.code_metrics);
        let panel = Rect::new(
            card.min_x() + px(6.0),
            card.min_y() + self.header_h,
            card.size.width - px(12.0),
            self.body_h,
        );
        layer.quad(Quad::filled(panel, self.fade(palette.text.with_alpha(0.028))).rounded(px(8.0)));
        let x0 = panel.min_x() + self.gutter;
        for (i, row) in self.rows.iter().enumerate() {
            let top = panel.min_y() + px(7.0) + i as f32 * line_h;
            let baseline = (top + (line_h - (cm.ascent + cm.descent)) / 2.0 + cm.ascent).round();
            let is_error = row.line == self.error_line;
            if is_error {
                layer.quad(Quad::filled(
                    Rect::new(panel.min_x(), top, panel.size.width, line_h),
                    self.fade(palette.danger.with_alpha(0.06)),
                ));
                layer.quad(
                    Quad::filled(
                        Rect::new(panel.min_x(), top + px(3.0), px(2.5), line_h - px(6.0)),
                        self.fade(palette.danger),
                    )
                    .rounded(px(1.25)),
                );
            }
            let number = text.shape_line(&(row.line + 1).to_string(), &self.small);
            let color = if is_error { palette.danger } else { palette.gutter_text };
            draw_text(layer, &number, x0 - px(14.0) - number.width, baseline, self.fade(color));
            self.paint_segments(layer, row, (x0, baseline), text, palette);
            if is_error {
                self.paint_squiggle(layer, row, (x0, baseline), palette);
            }
        }
    }

    /// A code row's text, cut where its syntax colors change, each piece in its color;
    /// `at`: where the text starts and its baseline.
    pub(super) fn paint_segments(
        &self,
        layer: &mut render_api::Layer,
        row: &PreviewRow,
        at: (f32, f32),
        text: &mut TextSystem,
        palette: &Palette,
    ) {
        let (x0, baseline) = at;
        let (source, start, colors, tab) = (&row.source, row.start, self.colors, self.tab);
        let end = start + source.len();
        let mut cuts = vec![0, source.len()];
        for c in colors.iter().filter(|c| c.range.end > start && c.range.start < end) {
            cuts.push(c.range.start.saturating_sub(start).min(source.len()));
            cuts.push((c.range.end - start).min(source.len()));
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a >= b || !source.is_char_boundary(a) || !source.is_char_boundary(b) {
                continue;
            }
            let color = colors
                .iter()
                .rev()
                .find(|c| c.range.start <= start + a && c.range.end >= start + b)
                .map_or(palette.code_text, |c| c.color);
            let (da, db) = (source_to_display(source, a, tab), source_to_display(source, b, tab));
            let Some(segment) = row.display.get(da..db) else { continue };
            let shaped = text.shape_line(segment, &self.code_font);
            draw_text(
                layer,
                &shaped,
                (x0 + row.shaped.x_at(da) - self.indent_x).round(),
                baseline,
                self.fade(color),
            );
        }
    }

    /// The error's squiggly underline on its row (as in the editor).
    pub(super) fn paint_squiggle(
        &self,
        layer: &mut render_api::Layer,
        row: &PreviewRow,
        at: (f32, f32),
        palette: &Palette,
    ) {
        let (x0, baseline) = at;
        let (source, start, tab, full) = (&row.source, row.start, self.tab, &row.shaped);
        let end = start + source.len();
        let a = self.error.start.clamp(start, end) - start;
        let b = self.error.end.clamp(start, end) - start;
        let xa = x0 + full.x_at(source_to_display(source, a, tab)) - self.indent_x;
        let xb = (x0 + full.x_at(source_to_display(source, b, tab)) - self.indent_x)
            .max(xa + self.code_metrics.advance);
        let step = self.px(3.0).max(2.0);
        let thick = (1.5 * self.scale).max(1.0);
        let base = baseline + self.px(3.0);
        let mut x = xa;
        let mut up = false;
        while x < xb {
            let w = step.min(xb - x);
            layer.quad(Quad::filled(
                Rect::new(x, if up { base - thick } else { base }, w, thick),
                self.fade(palette.danger),
            ));
            x += step;
            up = !up;
        }
    }
}

/// Both kinds of warnings (deprecated, then the others: ranges and their lines), in line
/// order; marks closer than a pill are merged. Records each mark in `out`.
pub(super) fn paint_warning_marks(
    marks: &mut render_api::Layer,
    strip: &Strip,
    lists: [(&[Range<usize>], &[u32]); 2],
    out: &mut Vec<(f32, usize)>,
    palette: &Palette,
) {
    let (w, s, vtrack) = (strip.wide, strip.scale, strip.track);
    let (warn_w, warn_h) = (((5.0 + 5.0 * w) * s).round(), ((2.0 + w) * s).round().max(2.0));
    let mut last_y = f32::MIN;
    let [(deprecated, warn_lines), (others, other_lines)] = lists;
    let mut warnings: Vec<(&Range<usize>, &u32)> =
        deprecated.iter().zip(warn_lines).chain(others.iter().zip(other_lines)).collect();
    warnings.sort_by_key(|(_, line)| **line);
    for (d, line) in warnings {
        let y = strip.y_of(*line, warn_h);
        if y - last_y < warn_h {
            continue;
        }
        last_y = y;
        let mark = Rect::new(
            (vtrack.min_x() + (vtrack.size.width - warn_w) / 2.0).round(),
            y,
            warn_w,
            warn_h,
        );
        marks.quad(Quad::filled(mark, palette.warning.with_alpha(0.85)).rounded(warn_h / 2.0));
        out.push((y + warn_h / 2.0, d.start));
    }
}

/// The errors (ranges and their lines); the one under the mouse (`hovered`: its offset) gets
/// a halo. Records each mark in `out`.
pub(super) fn paint_error_marks(
    marks: &mut render_api::Layer,
    strip: &Strip,
    errors: (&[Range<usize>], &[u32]),
    hovered: Option<usize>,
    out: &mut Vec<(f32, usize)>,
    palette: &Palette,
) {
    let (w, s, vtrack) = (strip.wide, strip.scale, strip.track);
    let mark_w = ((6.0 + 6.0 * w) * s).round();
    let mark_h = ((3.0 + 1.0 * w) * s).round().max(2.0);
    let mut last_y = f32::MIN;
    for (e, line) in errors.0.iter().zip(errors.1) {
        let y = strip.y_of(*line, mark_h);
        if y - last_y < mark_h {
            continue;
        }
        last_y = y;
        let mark = Rect::new(
            (vtrack.min_x() + (vtrack.size.width - mark_w) / 2.0).round(),
            y,
            mark_w,
            mark_h,
        );
        if hovered == Some(e.start) {
            let halo = (3.0 * s).round();
            let glow = Rect::new(
                mark.min_x() - halo,
                mark.min_y() - halo,
                mark.size.width + 2.0 * halo,
                mark.size.height + 2.0 * halo,
            );
            marks.quad(
                Quad::filled(glow, palette.danger.with_alpha(0.2)).rounded(glow.size.height / 2.0),
            );
        }
        marks.quad(Quad::filled(mark, palette.danger.with_alpha(0.9)).rounded(mark_h / 2.0));
        out.push((y + mark_h / 2.0, e.start));
    }
}

/// Occurrences of the selected word (ranges and their lines): small pills in the accent
/// color, on the left of the strip (errors stay on top, in the center). Records each mark
/// in `out`.
pub(super) fn paint_occurrence_marks(
    marks: &mut render_api::Layer,
    strip: &Strip,
    occurrences: (&[Range<usize>], &[u32]),
    out: &mut Vec<(f32, usize)>,
    palette: &Palette,
) {
    let (w, s, vtrack) = (strip.wide, strip.scale, strip.track);
    let occ_w = ((3.0 + 2.0 * w) * s).round().max(2.0);
    let occ_h = (2.0 * s).round().max(2.0);
    let mut last_y = f32::MIN;
    for (m, line) in occurrences.0.iter().zip(occurrences.1) {
        let y = strip.y_of(*line, occ_h);
        if y - last_y < occ_h + 1.0 {
            continue;
        }
        last_y = y;
        let mark = Rect::new(vtrack.min_x() + (2.0 * s).round(), y, occ_w, occ_h);
        marks.quad(Quad::filled(mark, palette.accent.with_alpha(0.75)).rounded(occ_h / 2.0));
        out.push((y + occ_h / 2.0, m.start));
    }
}

/// The error tooltip's mark (logical pixels).
const TIP_MARK: f32 = 16.0;
/// An error's mark's tint, and its code chip's fill and border (the tone's alphas).
const MARK_FILL: f32 = 0.14;
const CHIP_FILL: f32 = 0.08;
const CHIP_BORDER: f32 = 0.18;
/// A code chip: its height, corners and padding (logical pixels).
const CHIP_H: f32 = 22.0;
const CHIP_RADIUS: f32 = 5.0;
const CHIP_PAD: f32 = 6.0;

/// An error's mark: "!" in `font` on a round tint of `tone`, `side` logical pixels across.
fn error_mark<'f>(font: &'f Font, tone: Rgba, side: f32) -> crate::ds::Badge<'f> {
    crate::ds::Badge::new("!")
        .color(tone)
        .fill_alpha(MARK_FILL)
        .size(crate::ds::BadgeSize::Custom(side))
        .min_width(side)
        .pad(0.0)
        .centered()
        .font(font)
        .baseline(crate::ds::BadgeBaseline::Metrics)
}

/// An error's code (`E0308`) in a chip of `tone`, in the code's `font`.
fn code_chip<'f>(code: &'f str, font: &'f Font, tone: Rgba) -> crate::ds::Badge<'f> {
    crate::ds::Badge::new(code)
        .color(tone)
        .fill_alpha(CHIP_FILL)
        .border(tone.with_alpha(CHIP_BORDER))
        .size(crate::ds::BadgeSize::Custom(CHIP_H))
        .radius(CHIP_RADIUS)
        .pad(CHIP_PAD)
        .font(font)
        .baseline(crate::ds::BadgeBaseline::Metrics)
}
