//! The text: the glyphs with their syntax colors (restyled without reshaping), the folded regions'
//! labels, the carets, what is being composed (IME), the indent guides.

use super::*;

/// The mark at a wrapped row's end (its line goes on below) and at the start of the rows
/// after it, in the gutter.
pub(super) const WRAP_END: &str = "↩";
pub(super) const WRAP_GOES_ON: &str = "↪";

impl EditorView {
    /// The indent guides: a thin line at each indentation level of the visible rows (blank
    /// rows continue their block's), the cursor's block's brighter — in the theme's colors.
    pub(super) fn paint_indent_guides(
        &self,
        f: &Frame<'_>,
        layer: &mut render_api::Layer,
        palette: &Palette,
    ) {
        let (doc, folds, layout, area) = (f.doc, f.deco.folds, &f.layout, f.text_area);
        let tab = doc.tab_width();
        let unit = match doc.indent() {
            document::Indent::Spaces(n) => usize::from(n.max(1)),
            document::Indent::Tabs => tab.max(1) as usize,
        };
        let buffer = doc.buffer();
        let head = doc.selections().primary().head;
        // The cursor's block: its guide stands at the indentation of the line opening it.
        let active = folds.innermost(buffer.line_of(head) as u32).map(|f| {
            let opening = buffer.slice(
                buffer.line_start(f.start_line as usize)..buffer.line_end(f.start_line as usize),
            );
            (f.start_line, f.end_line, indent_width(&opening, tab).unwrap_or(0))
        });
        let thick = self.scale.round().max(1.0);
        for (i, &line) in self.lines.iter().enumerate() {
            let width = match self.indents.get(i).copied().flatten() {
                Some(width) => width,
                None => self.blank_indent(doc, i, tab),
            };
            let top = f.row_top(i);
            // Whole levels only: ` * ` in a doc comment (one space) gets no guide.
            let mut column = 0;
            while column + unit <= width {
                let x = (f.x0 + column as f32 * layout.advance).round();
                if x >= area.min_x() && x < area.max_x() {
                    let in_block =
                        active.is_some_and(|(s, e, c)| c == column && s < line && line < e);
                    let color = if in_block {
                        palette.editor_indent_guide_active
                    } else {
                        palette.editor_indent_guide
                    };
                    layer.quad(Quad::filled(Rect::new(x, top, thick, layout.line_height), color));
                }
                column += unit;
            }
        }
    }

    /// A blank row's guides: the smaller indentation of the lines around it (looking past
    /// the visible rows when needed, a little).
    pub(super) fn blank_indent(&self, doc: &Document, row: usize, tab: u32) -> usize {
        const LOOK: usize = 200;
        let buffer = doc.buffer();
        let nearest = |down: bool| -> usize {
            // Within the visible rows first, then the buffer.
            let mut k = row;
            loop {
                if down {
                    k += 1;
                } else if k == 0 {
                    break;
                } else {
                    k -= 1;
                }
                match self.indents.get(k) {
                    Some(Some(width)) => return *width,
                    Some(None) => continue,
                    None => break,
                }
            }
            let line = self.lines.get(row).copied().unwrap_or(0) as usize;
            let lines: Box<dyn Iterator<Item = usize>> = if down {
                Box::new((line + 1..buffer.line_count()).take(LOOK))
            } else {
                Box::new((0..line).rev().take(LOOK))
            };
            for l in lines {
                let text = buffer.slice(buffer.line_start(l)..buffer.line_end(l));
                if let Some(width) = indent_width(&text, tab) {
                    return width;
                }
            }
            0
        };
        nearest(false).min(nearest(true))
    }

    /// The text, colored glyph by glyph, and the labels of collapsed regions.
    pub(super) fn paint_text(
        &mut self,
        f: &Frame<'_>,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.pills.clear();
        self.lens_rects.clear();
        let content = scene.push_layer(Some(f.text_area));
        for i in 0..self.lines.len() {
            let line = self.lines[i];
            let seg = self.seg(i);
            self.row_base = self.load_row(f.doc, i, f.max_columns);
            self.row_shift = seg.hang as f32 * f.layout.advance;
            let start = f.doc.buffer().line_start(line as usize);
            // What a module hides is masked in the line, then its row's part taken again.
            if !f.deco.concealed.is_empty() {
                workbench_model::concealment::mask(&mut self.line, start, f.deco.concealed);
                self.row_base = self.reload_row_display(f.doc, i, f.max_columns);
            }
            let row = Row { line, top: f.row_top(i), baseline: f.row_baseline(i) };
            let shaped = text.shape_line(&self.display, &f.font);
            self.fill_row_colors(f, i, palette);
            self.push_glyphs(f, content, &shaped, row.baseline, palette);
            self.restyle_glyphs(f, (content, text), &shaped, row.baseline, palette);
            self.paint_inlays(f, content, text, &shaped, row, palette);
            if !seg.last {
                // The line goes on in the next row: the wrap's mark after this one's text.
                self.paint_wrap_mark(f, content, text, row, shaped.width, palette);
                continue;
            }
            let end = self.paint_fold_label(f, content, text, row, shaped.width, palette);
            self.paint_lens(f, content, text, row, end, palette);
        }
        self.row_base = 0;
        self.row_shift = 0.0;
    }

    /// Where the row drawn now starts (the text's start, a wrapped line's later rows under
    /// its text).
    fn row_x0(&self, f: &Frame<'_>) -> f32 {
        f.x0 + self.row_shift
    }

    /// The mark at a wrapped row's end (its line goes on below): muted, after its text.
    fn paint_wrap_mark(
        &self,
        f: &Frame<'_>,
        content: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        width: f32,
        palette: &Palette,
    ) {
        let mark = text.shape_line(WRAP_END, &f.font);
        let x = (self.row_x0(f) + width + f.layout.advance * 0.4).round();
        draw_text(content, &mark, x, row.baseline, palette.text_muted.with_alpha(0.6));
    }

    /// The row's inlay hints: their labels in the room made for them, as small muted chips.
    fn paint_inlays(
        &self,
        f: &Frame<'_>,
        content: &mut render_api::Layer,
        text: &mut TextSystem,
        shaped: &ShapedLine,
        row: Row,
        palette: &Palette,
    ) {
        if self.line_inlays.is_empty() {
            return;
        }
        let tab = f.doc.tab_width();
        let start = f.doc.buffer().line_start(row.line as usize);
        let first = self.inlays.partition_point(|i| i.offset < start);
        let font = Font::new(f.font.family.clone(), (f.font.size * 0.85).round());
        let (pad, gap) = ((3.0 * self.scale).round(), (2.0 * self.scale).round());
        // Inlays at the same byte stand one after another (the later ones nearer the text).
        let mut taken: usize = 0;
        let mut at_byte = usize::MAX;
        for (k, &(at, columns)) in self.line_inlays.iter().enumerate() {
            let Some(inlay) = self.inlays.get(first + k) else { break };
            if at != at_byte {
                at_byte = at;
                taken = 0;
            }
            // The display offset of the room before byte `at` (after its earlier inlays).
            let after = source_to_display_with(&self.line, at, tab, &self.line_inlays);
            let same: usize =
                self.line_inlays.iter().filter(|(o, _)| *o == at).map(|(_, c)| c).sum();
            let from = after - same + taken;
            taken += columns;
            // Another row's (a wrapped line's): not this one.
            let Some(from) = from.checked_sub(self.row_base).filter(|f| *f < self.display.len())
            else {
                continue;
            };
            let (x0, x1) = (shaped.x_at(from), shaped.x_at(from + columns));
            if self.row_x0(f) + x1 < f.text_area.min_x()
                || self.row_x0(f) + x0 > f.text_area.max_x()
            {
                continue;
            }
            let label = text.shape_line(&inlay.label, &font);
            let chip = Rect::new(
                (self.row_x0(f) + x0).round(),
                row.top + 3.0 * self.scale,
                (x1 - x0 - gap).max(label.width + 2.0 * pad),
                f.layout.line_height - 6.0 * self.scale,
            );
            content.quad(Quad::filled(chip, palette.tab_hover).rounded(4.0 * self.scale));
            let x = chip.min_x() + ((chip.size.width - label.width) / 2.0).round();
            draw_text(content, &label, x, row.baseline, palette.text_muted);
        }
    }

    /// The row's lens after its text (and its folded label): muted, clickable.
    fn paint_lens(
        &mut self,
        f: &Frame<'_>,
        content: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        end: f32,
        palette: &Palette,
    ) {
        let lenses = f.deco.lenses;
        let Ok(index) = lenses.binary_search_by_key(&row.line, |l| l.line) else { return };
        let lens = &lenses[index];
        let mut x = (self.row_x0(f) + end + f.layout.advance * 3.0).round();
        let color = palette.text_muted.with_alpha(0.85);
        let dot = text.shape_line(" · ", &f.ui_font);
        for (part, label) in lens.parts.iter().enumerate() {
            if x > f.text_area.max_x() {
                return;
            }
            if part > 0 {
                draw_text(content, &dot, x, row.baseline, color);
                x += dot.width;
            }
            let shaped = text.shape_line(label, &f.ui_font);
            draw_text(content, &shaped, x, row.baseline, color);
            let rect = Rect::new(x, row.top, shaped.width, f.layout.line_height);
            if let Some(rect) = rect.intersect(&f.text_area) {
                self.lens_rects.push((rect, index, part));
            }
            x += shaped.width;
        }
    }

    /// The color of each byte in visible row `i` (in `self.line`), into `self.colors`: its
    /// syntax colors over the text color, unused code dimmed.
    pub(super) fn fill_row_colors(&mut self, f: &Frame<'_>, i: usize, palette: &Palette) {
        let deco = &f.deco;
        let start = self.starts[i];
        let len = self.line.len().min(64 * 1024);
        self.colors.clear();
        self.colors.resize(len, palette.code_text);
        self.faces.clear();
        self.faces.resize(len, 0);
        for &index in &self.buckets[i] {
            let span = &deco.colors[index];
            let a = span.range.start.saturating_sub(start).min(len);
            let z = span.range.end.saturating_sub(start).min(len);
            self.colors[a..z].fill(span.color);
            let face = (u8::from(span.bold) * BOLD) | (u8::from(span.italic) * ITALIC);
            self.faces[a..z].fill(face);
        }
        // Unused code: dimmed (whatever its syntax color).
        let end = start + len;
        let first = deco.unused.partition_point(|u| u.end <= start);
        for u in deco.unused[first..].iter().take_while(|u| u.start < end) {
            let a = u.start.saturating_sub(start).min(len);
            let z = u.end.saturating_sub(start).min(len);
            for color in &mut self.colors[a..z] {
                *color = color.with_alpha(color.a * 0.45);
            }
        }
    }

    /// The row's glyphs (`self.display`, shaped), each in its source byte's color.
    pub(super) fn push_glyphs(
        &mut self,
        f: &Frame<'_>,
        content: &mut render_api::Layer,
        shaped: &ShapedLine,
        baseline: f32,
        palette: &Palette,
    ) {
        let len = self.colors.len();
        // Tabs and inlays shift positions: displayed → source mapping only when present.
        let has_tabs = self.line.contains('\t') || !self.line_inlays.is_empty();
        if has_tabs {
            let tab = f.doc.tab_width();
            build_display_map_with(&self.line, tab, &self.line_inlays, &mut self.disp_to_src);
        }
        let (base, x0) = (self.row_base, self.row_x0(f));
        content.glyphs.extend(shaped.glyphs.iter().map(|g| {
            let src = if has_tabs {
                self.disp_to_src.get(g.start + base).map_or(len, |&s| s as usize)
            } else {
                g.start + base
            };
            GlyphInstance {
                key: g.key,
                origin: Point::new(x0 + g.x, baseline + g.y),
                color: self.colors.get(src).copied().unwrap_or(palette.code_text),
            }
        }));
    }

    /// The glyphs of the row's bold / italic runs (the theme's: comments in italic…), drawn
    /// again in that face over the plain ones' places — the code font's faces are as wide, so
    /// nothing moves. The plain glyphs of those runs are taken out first.
    pub(super) fn restyle_glyphs(
        &mut self,
        f: &Frame<'_>,
        (content, text): (&mut render_api::Layer, &mut TextSystem),
        shaped: &ShapedLine,
        baseline: f32,
        palette: &Palette,
    ) {
        if self.faces.iter().all(|face| *face == 0) {
            return;
        }
        let has_tabs = self.line.contains('\t') || !self.line_inlays.is_empty();
        let (base, row_x0) = (self.row_base, self.row_x0(f));
        let src_of = |display: usize| {
            if has_tabs {
                self.disp_to_src.get(display + base).map_or(usize::MAX, |&s| s as usize)
            } else {
                display + base
            }
        };
        let face_of =
            |g: &text_layout::ShapedGlyph| self.faces.get(src_of(g.start)).copied().unwrap_or(0);
        // The row's glyphs were just pushed: the last `shaped.glyphs.len()` of the layer.
        let first = content.glyphs.len() - shaped.glyphs.len();
        let glyphs = &shaped.glyphs;
        let Restyle { mut runs, mut keep, mut styled } = std::mem::take(&mut self.restyle);
        runs.clear();
        let mut i = 0;
        while i < glyphs.len() {
            let face = face_of(&glyphs[i]);
            let start = i;
            while i < glyphs.len() && face_of(&glyphs[i]) == face {
                i += 1;
            }
            if face != 0 {
                runs.push((face, start, i));
            }
        }
        keep.clear();
        keep.resize(glyphs.len(), true);
        styled.clear();
        for &(face, a, z) in &runs {
            let from = glyphs[a].start;
            let to = glyphs.get(z).map_or(self.display.len(), |g| g.start);
            let Some(slice) = self.display.get(from..to) else { continue };
            let font = match face {
                BOLD => f.font.clone().with_weight(Font::BOLD),
                ITALIC => f.font.clone().with_italic(true),
                _ => f.font.clone().with_weight(Font::BOLD).with_italic(true),
            };
            let run = text.shape_line(slice, &font);
            let x0 = glyphs[a].x;
            for g in &run.glyphs {
                let src = src_of(from + g.start);
                let color = self.colors.get(src).copied().unwrap_or(palette.code_text);
                styled.push(GlyphInstance {
                    key: g.key,
                    origin: Point::new(row_x0 + x0 + g.x, baseline + g.y),
                    color,
                });
            }
            keep[a..z].fill(false);
        }
        // Only this row's glyphs are compacted (the earlier rows' stay as they are: a pass
        // over the whole layer per row would grow with the screen's glyphs).
        let mut write = first;
        for read in first..content.glyphs.len() {
            if keep[read - first] {
                content.glyphs[write] = content.glyphs[read];
                write += 1;
            }
        }
        content.glyphs.truncate(write);
        content.glyphs.append(&mut styled);
        self.restyle = Restyle { runs, keep, styled };
    }

    /// Collapsed region: the label (e.g. "12 chei" or "⋯") after the row's text (`width`
    /// wide), and the closing text.
    pub(super) fn paint_fold_label(
        &mut self,
        f: &Frame<'_>,
        content: &mut render_api::Layer,
        text: &mut TextSystem,
        row: Row,
        width: f32,
        palette: &Palette,
    ) -> f32 {
        let (folds, layout) = (f.deco.folds, &f.layout);
        let Some(fold) = folds.fold_at(row.line) else { return width };
        if !folds.is_collapsed(fold) {
            return width;
        }
        let label = match fold.placeholder {
            Some(languages::FoldCount::Keys(n)) => i18n::tr!("fold.keys", count = n),
            Some(languages::FoldCount::Items(n)) => i18n::tr!("fold.items", count = n),
            None => "⋯".to_owned(),
        };
        // The label is a badge in the row's text: the tabs' hover, a border, the UI's font
        // on the code's baseline.
        let ctx = crate::ds::Ctx::new(palette, self.scale);
        let badge = crate::ds::Badge::new(&label)
            .font(&f.ui_font)
            .baseline(crate::ds::BadgeBaseline::At(row.baseline))
            .color(palette.text_muted)
            .fill(palette.tab_hover)
            .border(palette.border)
            .radius(FOLD_RADIUS)
            .pad(FOLD_PAD)
            .size(crate::ds::BadgeSize::Custom(layout.line_height / self.scale - 2.0 * FOLD_INSET));
        let px = (self.row_x0(f) + width + layout.advance * 0.6).round();
        let mid = row.top + layout.line_height / 2.0;
        let pill = Rect::new(px, mid, badge.width(text, &ctx), 0.0);
        let closing = f.doc.buffer().line_text(fold.end_line as usize);
        let closing = closing.trim();
        let closing = (!closing.is_empty()
            && closing.len() <= 12
            && closing.starts_with(['}', ']', ')', '<']))
        .then(|| {
            (text.shape_line(closing, &f.font), (pill.max_x() + layout.advance * 0.6).round())
        });
        let end = closing.as_ref().map_or(pill.max_x(), |(shaped, x)| x + shaped.width);
        // A selection reaching the region takes it whole (`Folds::widen`): shown on it too.
        let tail = f.doc.buffer().line_end(row.line as usize);
        let selected = f.doc.selections().iter().any(|s| {
            let r = s.range();
            !r.is_empty() && r.start <= tail && tail <= r.end
        });
        if selected {
            let color = if f.focused { palette.selection } else { palette.selection_inactive };
            let x = self.row_x0(f) + width;
            content.quad(Quad::filled(
                Rect::new(x, row.top, (end - x).max(0.0), layout.line_height),
                color,
            ));
        }
        let pill = badge.paint(content, text, &ctx, px, mid);
        self.pills.push((pill, row.line));
        if let Some((shaped, x)) = &closing {
            draw_text(content, shaped, *x, row.baseline, palette.code_text);
        }
        end - self.row_x0(f)
    }

    /// The cursors, as the caret's look says (blinking, glow, glide, trail: `ui_kit::caret`).
    pub(super) fn paint_carets(
        &mut self,
        f: &Frame<'_>,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let (buffer, layout) = (f.doc.buffer(), &f.layout);
        let carets = scene.push_layer(Some(f.text_area));
        // This view's carets (each glides and leaves its trail on its own).
        let id = std::ptr::from_ref(self) as u64;
        for (n, sel) in f.doc.selections().iter().enumerate() {
            let line = buffer.line_of(sel.head);
            let column = sel.head - buffer.line_start(line);
            let Some(i) = self.visible_row_of(line, column) else { continue };
            self.load_line(f.doc, line);
            let seg = self.seg(i);
            let x = self.x_in_row(text, f.doc, seg, column);
            // The character's cell (a block covers it): up to the next one, else a space.
            let next = (column < buffer.line_len(line)).then(|| {
                let after = document::domain::movement::next_grapheme(buffer, sel.head);
                self.x_in_row(text, f.doc, seg, after - buffer.line_start(line))
            });
            let width = next.map_or(layout.advance, |n| (n - x).max(1.0));
            let x = x + seg.hang as f32 * layout.advance;
            let cell = Rect::new((f.x0 + x).round(), f.row_top(i), width, layout.line_height);
            ui_kit::caret::paint(
                carets,
                ui_kit::caret::CaretPaint {
                    id: id ^ ((n as u64) << 48),
                    cell,
                    color: palette.caret,
                    scale: self.scale,
                    shape: None,
                    thickness: None,
                    steady: false,
                },
            );
        }
    }

    /// The IME text being composed, at the primary cursor: over the text, underlined.
    pub(super) fn paint_preedit(
        &mut self,
        f: &Frame<'_>,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let Some(preedit) = self.preedit.take() else { return };
        let (doc, buffer, layout) = (f.doc, f.doc.buffer(), &f.layout);
        let head = doc.selections().primary().head;
        let line = buffer.line_of(head);
        let column = head - buffer.line_start(line);
        if let Some(i) = self.visible_row_of(line, column) {
            self.load_line(doc, line);
            let seg = self.seg(i);
            let x =
                (f.x0 + seg.hang as f32 * layout.advance + self.x_in_row(text, doc, seg, column))
                    .round();
            let shaped = text.shape_line(&preedit, &f.font);
            let top = f.row_top(i);
            let layer = scene.push_layer(Some(f.text_area));
            layer.quad(Quad::filled(
                Rect::new(x, top, shaped.width, layout.line_height),
                palette.editor_background,
            ));
            draw_text(layer, &shaped, x, baseline(layout, top), palette.text);
            let underline = Rect::new(
                x,
                top + layout.line_height - 3.0 * self.scale,
                shaped.width,
                self.scale.round().max(1.0),
            );
            layer.quad(Quad::filled(underline, palette.accent));
        }
        self.preedit = Some(preedit);
    }
}

/// A folded region's label: its corners, the room on each side of its text, and its inset
/// from the row's top and bottom (logical pixels).
const FOLD_RADIUS: f32 = 4.0;
const FOLD_PAD: f32 = 6.0;
const FOLD_INSET: f32 = 3.0;
