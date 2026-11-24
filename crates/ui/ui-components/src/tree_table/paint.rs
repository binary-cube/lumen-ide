//! The table painted: the header and its edges, the rows in view (the tree's guides, its
//! chevrons, the marks, the signs), each cell of its kind (text, a number with its unit, a
//! bar, a badge, an icon), the scrollbars.

use super::*;

/// A pill among a cell's words: its height, the room on each side of its label, and its
/// fill's alpha (of its ink; of the text when neutral).
const PILL_H: f32 = 17.0;
pub(super) const PILL_PAD: f32 = 6.0;
const PILL_FILL: f32 = 0.16;
const PILL_NEUTRAL_FILL: f32 = 0.07;
/// A cell's bar: its thickness, its widest, the least room it shows in, the gap before its
/// label, its track's alpha and the least a fill shows (logical pixels).
const BAR_H: f32 = 6.0;
const BAR_MAX: f32 = 90.0;
const BAR_MIN_ROOM: f32 = 16.0;
const BAR_GAP: f32 = 8.0;
const BAR_TRACK: f32 = 0.07;
const BAR_MIN_FILL: f32 = 2.0;

/// Where a cell is drawn.
#[derive(Clone, Copy)]
struct CellPlace {
    left: f32,
    room: f32,
    mid: f32,
    top: f32,
    height: f32,
    align: Align,
}

struct Fonts<'a> {
    normal: &'a Font,
    strong: &'a Font,
    code: &'a Font,
    small: &'a Font,
}

/// A pill's text color: a neutral one muted, others in their own color.
fn pill_color(ink: Ink, p: &Palette) -> Rgba {
    match ink {
        Ink::Tone(Tone::Neutral) => p.text_muted,
        ink => ink.color(p),
    }
}

/// A cell's bar's track before its label (starting at `label_x`): as wide as there's room,
/// at most [`BAR_MAX`]; none in too little room.
fn bar_track(ds: &ds::Ctx<'_>, at: &CellPlace, label_x: f32) -> Option<Rect> {
    let px = |v: f32| ds.px(v);
    let bar_w = (label_x - px(BAR_GAP) - at.left).min(px(BAR_MAX));
    if bar_w <= px(BAR_MIN_ROOM) {
        return None;
    }
    let h = px(BAR_H);
    Some(Rect::new(at.left.round(), (at.mid - h / 2.0).round(), bar_w.round(), h))
}

fn paint_cell(
    pen: &mut crate::LayerPainter<'_>,
    at: CellPlace,
    cell: &TableCell,
    fonts: &Fonts<'_>,
    (marked, numbers): (bool, &mut NumberText),
) {
    let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let base_of = |font: &Font| ds::optical_baseline(at.top, at.height, font);
    match &cell.value {
        CellValue::Empty => {}
        CellValue::Text(spans) => {
            // Each piece after the one before, the last ones cut to the room: measured
            // first (how many fit, how wide), then drawn — nothing kept between the two.
            let look = |i: usize, style: SpanStyle| match style {
                SpanStyle::Normal if marked && i == 0 => (fonts.strong, p.text),
                SpanStyle::Normal => (fonts.normal, p.text),
                SpanStyle::Strong => (fonts.strong, p.text),
                SpanStyle::Muted => (fonts.normal, p.text_muted),
                SpanStyle::Code => (fonts.code, p.text_muted),
                SpanStyle::Ink(ink) => (fonts.strong, ink.color(p)),
                SpanStyle::Pill(ink) => (fonts.small, pill_color(ink, p)),
            };
            // A piece's gap before it, its pill's padding, and the room it may take.
            let place = |i: usize, style: SpanStyle, used: f32| {
                let gap = if i > 0 { px(6.0) } else { 0.0 };
                let pad = if matches!(style, SpanStyle::Pill(_)) { px(12.0) } else { 0.0 };
                (gap, pad, at.room - used - gap - pad)
            };
            let (mut used, mut count) = (0.0, 0);
            for (i, span) in spans.iter().enumerate() {
                let (gap, pad, left) = place(i, span.style, used);
                if left < px(12.0) {
                    break;
                }
                let (font, _) = look(i, span.style);
                used += gap + ds::ellipsized_width(text, &span.text, font, left) + pad;
                count = i + 1;
            }
            let mut x = match at.align {
                Align::Left => at.left,
                Align::Right => at.left + at.room - used,
            };
            let mut drawn = 0.0;
            for (i, span) in spans.iter().take(count).enumerate() {
                let (gap, _, left) = place(i, span.style, drawn);
                let (font, color) = look(i, span.style);
                x += gap;
                let width = ds::ellipsized_width(text, &span.text, font, left);
                if let SpanStyle::Pill(ink) = span.style {
                    let fill = match ink {
                        Ink::Tone(Tone::Neutral) => p.text.with_alpha(PILL_NEUTRAL_FILL),
                        ink => ink.color(p).with_alpha(PILL_FILL),
                    };
                    // Its label as it is, a copy cut short only when it doesn't fit.
                    let cut;
                    let label = if text.shape_line(&span.text, font).width <= left + 0.5 {
                        span.text.as_str()
                    } else {
                        cut = ds::ellipsize(text, &span.text, font, left);
                        cut.as_str()
                    };
                    ds::Badge::new(label)
                        .font(font)
                        .color(color)
                        .fill(fill)
                        .size(ds::BadgeSize::Custom(PILL_H))
                        .pad(PILL_PAD)
                        .baseline(ds::BadgeBaseline::At(base_of(font)))
                        .paint(layer, text, ds, x, at.mid);
                    x += width + px(2.0 * PILL_PAD);
                } else {
                    let base = base_of(font);
                    ds::draw_ellipsized(
                        layer,
                        text,
                        &span.text,
                        font,
                        (x.round(), base),
                        left,
                        color,
                    );
                    x += width;
                }
                drawn += gap
                    + width
                    + if matches!(span.style, SpanStyle::Pill(_)) { px(12.0) } else { 0.0 };
            }
        }
        CellValue::Number { value, decimals, unit } => {
            let shaped = text.shape_line(numbers.write(*value, *decimals), fonts.normal);
            let unit_shaped = unit.as_ref().map(|u| text.shape_line(u, fonts.normal));
            let unit_w = unit_shaped.as_ref().map_or(0.0, |u| u.width + px(4.0));
            let w = shaped.width + unit_w;
            let x = match at.align {
                Align::Left => at.left,
                Align::Right => at.left + at.room - w,
            };
            let base = base_of(fonts.normal);
            draw_text(layer, &shaped, x.round(), base, p.text);
            if let Some(u) = unit_shaped {
                draw_text(layer, &u, (x + shaped.width + px(4.0)).round(), base, p.text_muted);
            }
        }
        CellValue::Bar { fraction, ink, label } => {
            let shaped = text.shape_line(label, fonts.normal);
            let label_x = at.left + at.room - shaped.width;
            draw_text(layer, &shaped, label_x.round(), base_of(fonts.normal), p.text);
            if let Some(track) = bar_track(ds, &at, label_x) {
                ds::ProgressBar::of(*fraction)
                    .color(ink.color(p))
                    .track_alpha(BAR_TRACK)
                    .min_fill(BAR_MIN_FILL)
                    .snapped()
                    .paint_bar(layer, ds, track);
            }
        }
        CellValue::Split { a, b, label } => {
            let shaped = text.shape_line(label, fonts.normal);
            let label_x = at.left + at.room - shaped.width;
            draw_text(layer, &shaped, label_x.round(), base_of(fonts.normal), p.text);
            if let Some(track) = bar_track(ds, &at, label_x) {
                let parts = [
                    ds::ProgressSegment::new(*a, p.accent),
                    ds::ProgressSegment::new(*b, p.warning),
                ];
                ds::ProgressBar::split(&parts)
                    .track_alpha(BAR_TRACK)
                    .snapped()
                    .paint_bar(layer, ds, track);
            }
        }
        CellValue::Badge { label, tone } => {
            let (x, align_right) = match at.align {
                Align::Left => (at.left, false),
                Align::Right => (at.left + at.room, true),
            };
            ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec { label, x, mid: at.mid, align_right, tone: *tone },
            );
        }
        CellValue::Icon { name, tone } => {
            if let Some(id) = icons.id(name) {
                let s = px(14.0);
                let x = match at.align {
                    Align::Left => at.left,
                    Align::Right => at.left + at.room - s,
                };
                let ir = Rect::new(x.round(), (at.mid - s / 2.0).round(), s, s);
                draw_icon(layer, icons, id, ir, tone.color(p));
            }
        }
    }
}

/// The texts a number is written in ([`number_text`]'s), kept by whoever writes many.
#[derive(Debug, Default)]
pub(super) struct NumberText {
    shown: String,
    digits: String,
}

impl NumberText {
    /// `value` written as [`number_text`] writes it, in the kept texts.
    fn write(&mut self, value: f64, decimals: u8) -> &str {
        use std::fmt::Write;
        let (shown, digits) = (&mut self.shown, &mut self.digits);
        shown.clear();
        if !value.is_finite() {
            let _ = write!(shown, "{value}");
            return shown;
        }
        digits.clear();
        let _ = write!(digits, "{:.*}", decimals as usize, value.abs());
        let (int, frac) = match digits.split_once('.') {
            Some((i, f)) => (i, f.trim_end_matches('0')),
            None => (digits.as_str(), ""),
        };
        if value < 0.0 && (int != "0" || !frac.is_empty()) {
            shown.push('-');
        }
        for (i, c) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 {
                shown.push('\u{2009}');
            }
            shown.push(c);
        }
        if !frac.is_empty() {
            shown.push('.');
            shown.push_str(frac);
        }
        shown
    }
}

/// `value` with its thousands grouped by a thin space and at most `decimals` decimals (the
/// trailing zeros dropped): `12 340`, `0.42`, `1 204.5`.
pub fn number_text(value: f64, decimals: u8) -> String {
    NumberText::default().write(value, decimals).to_owned()
}

impl TreeTable {
    /// Draws it in `bounds`: the header, the rows in view, the scrollbar.
    pub fn paint(&mut self, ctx: TablePaint<'_>) {
        let TablePaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.scale = scale;
        self.row_h = px(ROW_H).round();
        self.gutter = if self.rows.iter().any(|r| r.sign.is_some()) { px(GUTTER) } else { 0.0 };
        self.header = Rect::new(b.min_x(), b.min_y(), b.size.width, px(HEADER_H).round());
        self.body = Rect::new(
            b.min_x(),
            self.header.max_y(),
            b.size.width,
            (b.size.height - self.header.size.height).max(0.0),
        );
        let base = scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background));
        self.lay_columns(b.min_x() + self.gutter, b.size.width - self.gutter);
        let shown = self.shown_rows().len();
        if self.reveal {
            self.reveal = false;
            self.bring_into_view();
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(shown));
        self.hscroll = self.hscroll.clamp(0.0, self.max_hscroll());
        let font = ds.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let code = ds.code_font(-1.0);
        let small = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
        // The header.
        {
            let layer = scene.push_layer(Some(self.header));
            crate::table::header_band(layer, &ds, self.header);
            let title_font = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
            for (i, c) in self.columns.iter().enumerate() {
                let (x, w) = self.xs[i];
                let x = x - self.hscroll;
                let r = Rect::new(x, self.header.min_y(), w, self.header.size.height);
                let _told = crate::table::column_header(&c.title, r, Some(i));
                if matches!(self.hover, Some(Hit::Header(h)) if h == i) && c.tip.is_some() {
                    crate::table::header_hover(layer, &ds, r);
                }
                let room = (w - px(16.0)).max(0.0);
                let tx = match c.align {
                    Align::Left => x + px(if i == 0 { 10.0 } else { 8.0 }),
                    Align::Right => {
                        x + w - px(8.0) - ds::ellipsized_width(text, &c.title, &title_font, room)
                    }
                };
                let base = ds::optical_baseline(r.min_y(), r.size.height, &title_font);
                let at = (tx.round(), base);
                ds::draw_ellipsized(layer, text, &c.title, &title_font, at, room, p.text_muted);
                if i + 1 < self.columns.len() {
                    let dragged = matches!(self.hover, Some(Hit::Edge(e)) if e == i + 1)
                        || self.resizing.is_some_and(|(e, ..)| e == i + 1);
                    crate::table::header_edge(layer, &ds, x + w - ds.line() / 2.0, r, dragged);
                }
            }
            crate::table::header_rule(layer, &ds, self.header);
        }
        if self.rows.is_empty() {
            let area = Rect::new(
                self.body.min_x(),
                self.body.min_y(),
                self.body.size.width,
                (self.body.size.height * 0.8).max(0.0),
            );
            let base = scene.push_layer(Some(self.body));
            ds::empty_state(base, text, &ds, area, &self.empty, None);
            return;
        }
        // The rows in view.
        let range = TreeList::new(self.row_h, shown).visible(self.scroll, self.body.size.height);
        // The rows in view read from the rows shown (made once per change), never copied.
        self.shown_rows();
        let layer = scene.push_layer(Some(self.body));
        let hovered_row = match self.hover {
            Some(Hit::Row { row, .. }) => Some(row),
            _ => None,
        };
        let mut numbers = std::mem::take(&mut self.numbers);
        for slot in range {
            let Some(&row) = self.shown.value().and_then(|v| v.get(slot)) else { continue };
            let y = (self.body.min_y() + slot as f32 * self.row_h - self.scroll).round();
            let full = Rect::new(b.min_x(), y, b.size.width, self.row_h);
            let r = &self.rows[row];
            // To a screen reader: the row as a tree's item named by its first cell (made only
            // while a tree is), its other cells read under it.
            let name = match r.cells.first().map(|c| &c.value) {
                Some(CellValue::Text(spans)) if ui_kit::access::recording() => {
                    spans.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
                }
                _ => String::new(),
            };
            let _row = tree::item(tree::Item {
                name: &name,
                bounds: full,
                level: self.depth(row),
                expanded: self.children[row].then(|| self.open[row]),
                selected: self.selected == Some(row),
            });
            if let Some(ink) = r.tint {
                layer.quad(Quad::filled(full, ink.color(p).with_alpha(0.08)));
            }
            if self.selected == Some(row) {
                layer.quad(Quad::filled(full, p.tree_selection));
            } else if hovered_row == Some(row) {
                layer.quad(Quad::filled(full, p.tree_hover));
            }
            if r.marked {
                layer.quad(Quad::filled(
                    Rect::new(b.min_x(), y, px(2.0).round(), self.row_h),
                    p.accent,
                ));
            }
            let mid = y + self.row_h / 2.0;
            if let Some((icon, ink)) = r.sign
                && let Some(id) = icons.id(icon)
            {
                let s = px(13.0);
                let ir = Rect::new(
                    (b.min_x() + (self.gutter - s) / 2.0).round(),
                    (mid - s / 2.0).round(),
                    s,
                    s,
                );
                draw_icon(layer, icons, id, ir, ink.color(p));
            }
            // Folded: what it hides said after its text.
            let folded = match &r.folded_note {
                Some(note) if self.children[row] && !self.open[row] => {
                    r.cells.first().map(|first| {
                        let mut cell = first.clone();
                        if let CellValue::Text(spans) = &mut cell.value {
                            spans.push(Span::new(
                                note.clone(),
                                SpanStyle::Pill(Ink::Tone(Tone::Neutral)),
                            ));
                        }
                        cell
                    })
                }
                _ => None,
            };
            for (column, cell) in r.cells.iter().enumerate().take(self.xs.len()) {
                let cell = match (&folded, column) {
                    (Some(f), 0) => f,
                    _ => cell,
                };
                let (x, w) = self.xs[column];
                let x = x - self.hscroll;
                let pad = px(8.0);
                let mut left = x + pad;
                let right = x + w - pad;
                if column == 0 {
                    let depth = self.depth(row);
                    let x_of = |level: usize| x + px(6.0 + INDENT * level as f32 + 7.0);
                    tree::guides(layer, 0..depth, x_of, (y, self.row_h), ds.line(), p.indent_guide);
                    left = x + px(6.0 + INDENT * depth as f32);
                    if self.children[row] {
                        let s = px(14.0);
                        let at = Rect::new(left.round(), (mid - s / 2.0).round(), s, s);
                        tree::chevron(layer, icons, at, self.open[row], p.text_muted);
                    }
                    left += px(18.0);
                    if let Some(icon) = r.icon
                        && let Some(id) = icons.id(icon)
                    {
                        let s = px(14.0);
                        let ir = Rect::new(left.round(), (mid - s / 2.0).round(), s, s);
                        let color = if r.marked { p.accent } else { p.text_muted };
                        draw_icon(layer, icons, id, ir, color);
                        left += s + px(6.0);
                    }
                }
                if let Some(ink) = cell.outline {
                    let inset = px(3.0).round();
                    let o = Rect::new(
                        (x + inset).round(),
                        y + inset,
                        (w - 2.0 * inset).max(0.0).round(),
                        self.row_h - 2.0 * inset,
                    );
                    layer.quad(
                        Quad::filled(o, ink.color(p).with_alpha(0.06))
                            .rounded(px(4.0))
                            .bordered(ds.line().max(px(1.0)), ink.color(p).with_alpha(0.85)),
                    );
                }
                if let Some((icon, ink)) = cell.lead
                    && let Some(id) = icons.id(icon)
                {
                    let s = px(13.0);
                    let ir = Rect::new(left.round(), (mid - s / 2.0).round(), s, s);
                    draw_icon(layer, icons, id, ir, ink.color(p));
                    left += s + px(5.0);
                }
                let room = (right - left).max(0.0);
                let align = self.columns[column].align;
                let cell_mid = mid;
                let fonts = Fonts { normal: &font, strong: &strong, code: &code, small: &small };
                paint_cell(
                    &mut crate::LayerPainter::new(layer, text, icons, ds),
                    CellPlace { left, room, mid: cell_mid, top: y, height: self.row_h, align },
                    cell,
                    &fonts,
                    (r.marked && column == 0, &mut numbers),
                );
            }
        }
        self.numbers = numbers;
        let content = self.content_height(shown);
        self.bar.paint(scene, p, scale, self.body, content, self.scroll);
    }
}
