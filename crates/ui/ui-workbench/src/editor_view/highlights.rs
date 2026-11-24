//! What is drawn behind the text of the visible rows: the current lines, the colors put in
//! buckets per row, the selections, squiggles, strike-throughs, the link, the occurrences,
//! the search matches and the finding pointed at, the ruler.

use super::*;

impl EditorView {
    /// The current line, for each cursor without a selection; the lines a module highlights.
    pub(super) fn paint_current_lines(&self, f: &Frame<'_>, scene: &mut Scene, palette: &Palette) {
        let (b, buffer) = (f.bounds, f.doc.buffer());
        let background = scene.push_layer(Some(b));
        if let Some((first, end)) = f.deco.highlight {
            for (i, &line) in self.lines.iter().enumerate() {
                if (first..end).contains(&(line as usize)) {
                    background.quad(Quad::filled(
                        Rect::new(b.min_x(), f.row_top(i), b.size.width, f.layout.line_height),
                        palette.accent.with_alpha(0.08),
                    ));
                }
            }
        }
        if let Some((from, to, color)) = f.deco.focus {
            let (first, last) = (buffer.line_of(from), buffer.line_of(to));
            for (i, &line) in self.lines.iter().enumerate() {
                if (first..=last).contains(&(line as usize)) {
                    background.quad(Quad::filled(
                        Rect::new(b.min_x(), f.row_top(i), b.size.width, f.layout.line_height),
                        color.with_alpha(0.07),
                    ));
                }
            }
        }
        // Every row of the line (a wrapped one's too): only the cursors on the rows shown,
        // each line's rows found in the sorted lines.
        let (Some(&top), Some(&bottom)) = (self.lines.first(), self.lines.last()) else { return };
        let shown = buffer.line_start(top as usize)..=buffer.line_end(bottom as usize);
        for (_, sel) in near(f.doc.selections().as_slice(), shown).filter(|(_, s)| s.is_empty()) {
            let line = buffer.line_of(sel.head) as u32;
            let first = self.lines.partition_point(|l| *l < line);
            let last = self.lines.partition_point(|l| *l <= line);
            for i in first..last {
                background.quad(Quad::filled(
                    Rect::new(b.min_x(), f.row_top(i), b.size.width, f.layout.line_height),
                    palette.current_line,
                ));
            }
        }
    }

    /// Distributes the colors over the visible rows once (keeping the order): `self.buckets`.
    pub(super) fn bucket_colors(&mut self, f: &Frame<'_>) {
        let buffer = f.doc.buffer();
        self.starts.clear();
        self.starts.extend(self.lines.iter().map(|&l| buffer.line_start(l as usize)));
        self.buckets.resize_with(self.lines.len(), Vec::new);
        self.buckets.iter_mut().for_each(Vec::clear);
        for (index, span) in f.deco.colors.iter().enumerate() {
            // The first row of the line the span starts on (a wrapped line's rows share its
            // start).
            let last = self.starts.partition_point(|&s| s <= span.range.start).saturating_sub(1);
            let line_start = self.starts.get(last).copied().unwrap_or(0);
            let first = self.starts.partition_point(|&s| s < line_start);
            for (i, start) in self.starts.iter().enumerate().skip(first) {
                if *start >= span.range.end && i > first {
                    break;
                }
                self.buckets[i].push(index);
            }
        }
    }

    /// What is drawn behind the visible rows' text (selections, squiggles, strike-throughs,
    /// the link, occurrences, search matches), collected row by row and drawn together;
    /// notes each row's indentation for the guides. Returns the widest row's width.
    pub(super) fn paint_highlights(
        &mut self,
        f: &Frame<'_>,
        text: &mut TextSystem,
        layer: &mut render_api::Layer,
        palette: &Palette,
    ) -> f32 {
        let (buffer, tab, deco) = (f.doc.buffer(), f.doc.tab_width(), &f.deco);
        let selection_color =
            if f.focused { palette.selection } else { palette.selection_inactive };
        let mut widest: f32 = 0.0;
        self.link_rect = None;
        let mut rects = std::mem::take(&mut self.rects);
        rects.clear();
        let mut focus: Vec<(Rect, Rgba)> = Vec::new();
        self.indents.clear();
        for i in 0..self.lines.len() {
            let line = self.lines[i] as usize;
            let base = self.load_row(f.doc, i, f.max_columns);
            let seg = self.seg(i);
            // A wrapped line's later rows: no guides (their text starts at the left).
            self.indents.push(if seg.first { indent_width(&self.line, tab) } else { Some(0) });
            let shaped = text.shape_line(&self.display, &f.font);
            widest = widest.max(shaped.width);
            let line_start = buffer.line_start(line);
            let (start, end) = if seg.first && seg.last {
                (line_start, buffer.line_end(line))
            } else {
                let to = if seg.last { buffer.line_end(line) } else { line_start + seg.to };
                (line_start + seg.from, to)
            };
            let inlays = &self.line_inlays;
            let x_of = |offset: usize| {
                let display = source_to_display_with(&self.line, offset - line_start, tab, inlays);
                shaped.x_at(display.saturating_sub(base))
            };
            let shift = seg.hang as f32 * f.layout.advance;
            let row = RowSpan {
                start,
                end,
                continues: !seg.last,
                top: f.row_top(i),
                x0: f.x0 + shift,
                width: shaped.width,
                layout: &f.layout,
                scale: f.scale,
                x_of: &x_of,
            };
            push_selections(&mut rects, &row, f.doc, selection_color, deco.current_match);
            push_squiggles(&mut rects, &row, deco, palette);
            push_strikes(&mut rects, &row, deco.deprecated, palette);
            if let Some(rect) = push_link(&mut rects, &row, deco.link, palette) {
                self.link_rect = Some(rect);
            }
            push_matches(&mut rects, &row, deco, palette);
            if let Some((from, to, color)) = deco.focus
                && from <= end
                && to >= start
            {
                let (a, z) = (x_of(from.max(start)), x_of(to.min(end)));
                let r = Rect::new(
                    f.x0 + shift + a - self.scale,
                    row.top,
                    (z - a).max(2.0) + 2.0 * self.scale,
                    f.layout.line_height,
                );
                focus.push((r, color));
            }
        }
        for &(rect, color) in &rects {
            layer.quad(Quad::filled(rect, color).rounded(2.0 * self.scale));
        }
        // A finding's place: tinted in its color and outlined, over the other backgrounds.
        for &(rect, color) in &focus {
            layer.quad(
                Quad::filled(rect, color.with_alpha(0.22))
                    .rounded(3.0 * self.scale)
                    .bordered(self.scale.round().max(1.0), color.with_alpha(0.9)),
            );
        }
        self.rects = rects;
        widest
    }

    /// The vertical guide at the ruler's column, when it is in view.
    pub(super) fn paint_ruler(
        &self,
        f: &Frame<'_>,
        layer: &mut render_api::Layer,
        palette: &Palette,
    ) {
        let Some(column) = self.ruler else { return };
        let (b, text_area) = (f.bounds, f.text_area);
        let x = (f.x0 + column as f32 * f.layout.advance).round();
        if x > text_area.min_x() && x < text_area.max_x() {
            let ctx = crate::ds::Ctx::new(palette, self.scale);
            crate::ds::divider_vertical(layer, &ctx, x, b.min_y(), b.max_y());
        }
    }
}

/// A visible row while its highlights are collected: its byte range, its top, where its
/// text starts and how wide it is, the x (from `x0`) of a byte offset in it.
struct RowSpan<'a> {
    start: usize,
    end: usize,
    /// Its line goes on in the next row (wrapped): what starts at `end` is that row's.
    continues: bool,
    top: f32,
    x0: f32,
    width: f32,
    layout: &'a Layout,
    scale: f32,
    x_of: &'a dyn Fn(usize) -> f32,
}

/// The selections crossing the row — but the one that is the current search match (Find
/// selects it): its outline shows it, and the text in it stays as readable as anywhere.
fn push_selections(
    rects: &mut Vec<(Rect, Rgba)>,
    row: &RowSpan<'_>,
    doc: &Document,
    color: Rgba,
    current_match: Option<&Range<usize>>,
) {
    let (start, end, layout, x_of) = (row.start, row.end, row.layout, row.x_of);
    for (_, sel) in near(doc.selections().as_slice(), start..=end).filter(|(_, s)| !s.is_empty()) {
        let r = sel.range();
        let after = if row.continues { r.start >= end } else { r.start > end };
        if r.end < start || after || current_match == Some(&r) {
            continue;
        }
        let a = x_of(r.start.max(start));
        // A selection running past the line end also includes `\n`: extend it a bit.
        let z = if r.end > end { row.width + layout.advance * 0.6 } else { x_of(r.end) };
        if z > a {
            rects.push((Rect::new(row.x0 + a, row.top, z - a, layout.line_height), color));
        }
    }
}

/// Errors (red) and warnings (yellow): squiggly underline (small zigzag segments), at least
/// one character.
fn push_squiggles(
    rects: &mut Vec<(Rect, Rgba)>,
    row: &RowSpan<'_>,
    deco: &Decorations<'_>,
    palette: &Palette,
) {
    let (start, end, layout, x_of) = (row.start, row.end, row.layout, row.x_of);
    for (list, color) in [(deco.warnings, palette.warning), (deco.errors, palette.danger)] {
        let first = list.partition_point(|e| e.end < start);
        for e in list[first..].iter().take_while(|e| e.start <= end) {
            let a = x_of(e.start.clamp(start, end));
            let z = x_of(e.end.clamp(start, end)).max(a + layout.advance);
            let step = (3.0 * row.scale).round().max(2.0);
            let thick = (1.5 * row.scale).max(1.0);
            let base = row.top + layout.line_height - 2.5 * row.scale;
            let mut x = a;
            let mut up = false;
            while x < z {
                let w = step.min(z - x);
                let y = if up { base - thick } else { base };
                rects.push((Rect::new(row.x0 + x, y, w, thick), color));
                x += step;
                up = !up;
            }
        }
    }
}

/// Deprecated: struck through.
fn push_strikes(
    rects: &mut Vec<(Rect, Rgba)>,
    row: &RowSpan<'_>,
    deprecated: &[Range<usize>],
    palette: &Palette,
) {
    let (start, end, layout, x_of) = (row.start, row.end, row.layout, row.x_of);
    let first = deprecated.partition_point(|d| d.end < start);
    for d in deprecated[first..].iter().take_while(|d| d.start <= end) {
        let a = x_of(d.start.clamp(start, end));
        let z = x_of(d.end.clamp(start, end)).max(a + layout.advance);
        let thick = row.scale.round().max(1.0);
        let y = (row.top + layout.line_height / 2.0).round();
        rects.push((Rect::new(row.x0 + a, y, z - a, thick), palette.warning));
    }
}

/// The link (⌘ + hover), when it is on the row: a background and an underline. Returns
/// where it is.
fn push_link(
    rects: &mut Vec<(Rect, Rgba)>,
    row: &RowSpan<'_>,
    link: Option<&Range<usize>>,
    palette: &Palette,
) -> Option<Rect> {
    let link = link.filter(|l| l.start >= row.start && l.end <= row.end)?;
    let (layout, x_of, top) = (row.layout, row.x_of, row.top);
    let (a, z) = (x_of(link.start), x_of(link.end));
    let rect = Rect::new(row.x0 + a, top, (z - a).max(layout.advance), layout.line_height);
    rects.push((rect, palette.accent.with_alpha(0.12)));
    let thick = row.scale.round().max(1.0);
    let base = (top + layout.line_height - 3.0 * row.scale).round();
    rects.push((Rect::new(rect.min_x(), base, rect.size.width, thick), palette.accent));
    Some(rect)
}

/// Occurrences of the selected word: subtle background (below selection and search);
/// then the search matches, the current one stronger.
fn push_matches(
    rects: &mut Vec<(Rect, Rgba)>,
    row: &RowSpan<'_>,
    deco: &Decorations<'_>,
    palette: &Palette,
) {
    let (start, end, layout, x_of) = (row.start, row.end, row.layout, row.x_of);
    let first_occ = deco.occurrences.partition_point(|m| m.end < start);
    for m in deco.occurrences[first_occ..].iter().take_while(|m| m.start <= end) {
        let (a, z) = (x_of(m.start.max(start)), x_of(m.end.min(end)));
        let rect = Rect::new(row.x0 + a, row.top + 1.0, (z - a).max(2.0), layout.line_height - 2.0);
        rects.push((rect, palette.accent.with_alpha(0.13)));
    }
    let first_match = deco.matches.partition_point(|m| m.end < start);
    for m in deco.matches[first_match..].iter().take_while(|m| m.start <= end) {
        let (a, z) = (x_of(m.start.max(start)), x_of(m.end.min(end)));
        let rect = Rect::new(row.x0 + a, row.top, (z - a).max(2.0), layout.line_height);
        if deco.current_match == Some(m) {
            push_current_match(rects, rect, palette.search_match_current, row.scale);
        } else {
            rects.push((rect, palette.search_match));
        }
    }
}

/// The current search match: a faint fill (the text over it reads as anywhere else, in
/// every theme, whatever its color — a comment's too) and a solid outline in the match's color (where it is, at a
/// glance).
fn push_current_match(rects: &mut Vec<(Rect, Rgba)>, rect: Rect, color: Rgba, scale: f32) {
    let line = scale.round().max(1.0) * 1.5;
    let line = line.round();
    rects.push((rect, color.with_alpha(color.a.min(0.12))));
    let edge = color.with_alpha(1.0);
    let (x, y, w, h) = (rect.min_x(), rect.min_y(), rect.size.width, rect.size.height);
    rects.push((Rect::new(x, y, w, line), edge));
    rects.push((Rect::new(x, y + h - line, w, line), edge));
    rects.push((Rect::new(x, y, line, h), edge));
    rects.push((Rect::new(x + w - line, y, line, h), edge));
}

/// The selections touching `bytes` (ending at or after its start, starting at or before its
/// end), with their places among all: selections are sorted and don't overlap, so both their
/// starts and their ends only grow — found by a binary search, never all of them walked (a
/// row is drawn with ten thousand cursors after "Select All Occurrences").
pub(super) fn near(
    all: &[document::Selection],
    bytes: std::ops::RangeInclusive<usize>,
) -> impl Iterator<Item = (usize, &document::Selection)> {
    let (from, to) = (*bytes.start(), *bytes.end());
    let first = all.partition_point(|s| s.range().end < from);
    all.iter().enumerate().skip(first).take_while(move |(_, s)| s.range().start <= to)
}
