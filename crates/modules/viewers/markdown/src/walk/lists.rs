//! Lists and quotes: an item's marker (bullets, numbers, tasks), a quote opened and closed, a
//! callout's look.

use super::*;

impl Walk<'_> {
    /// Text of a list item without a paragraph (a "tight" list), if there is any.
    pub(super) fn flush_item(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let x0 = self.indent();
        if let Some(marker) = self.item_marker.take() {
            self.marker(marker, x0);
        }
        let color = self.text_color();
        self.flow_body(x0, color);
        self.b.y += self.b.px(2.0);
    }

    pub(super) fn item_start(&mut self) {
        self.flush_item();
        let nested = self.lists.len() > 1;
        let marker = match self.lists.last_mut().and_then(|l| l.next.as_mut()) {
            Some(n) => {
                let m = format!("{n}.");
                *n += 1;
                m
            }
            None => if nested { "◦" } else { "•" }.to_owned(),
        };
        self.item_marker = Some(Marker::Text(marker));
    }

    /// A list item's bullet, number or checkbox, to the left of its text.
    pub(super) fn marker(&mut self, marker: Marker, x0: f32) {
        let depth = self.lists.len();
        let b = &mut self.b;
        let size = if self.small { (b.body * 0.88).round() } else { b.body };
        let font = Font::new(b.palette.ui_font.clone(), size);
        let metrics = b.text.metrics(&font);
        let line_h = (size * 1.6).round();
        let baseline =
            b.y + ((line_h - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent).round();
        match marker {
            Marker::Text(marker) => {
                let width = b.text.shape_line(&marker, &font).width;
                let color = if marker.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    b.palette.text_muted
                } else {
                    b.palette.accent
                };
                let x = x0 - width - b.px(8.0);
                // Copied with the item, indented as deep as its list.
                let copy = format!("{}{marker} ", "  ".repeat(depth.saturating_sub(1)));
                b.blocks += 1;
                let sep = b.take_sep(Sep::Line);
                // Up to the item's text: one box with it when selected.
                b.run(Rect::new(x, b.y, x0 - x, line_h), String::new(), None, copy, sep);
                b.sep = Some(Sep::None);
                b.items.push(Item::Text { x, baseline, text: marker, font, color });
            }
            Marker::Task(done, range) => {
                let side = b.px(15.0);
                let rect = Rect::new(
                    x0 - side - b.px(7.0),
                    (b.y + (line_h - side) / 2.0).round(),
                    side,
                    side,
                );
                let radius = b.px(4.0);
                if done {
                    b.items.push(Item::Rect { rect, color: b.palette.accent, radius });
                    let inset = b.px(2.0);
                    let icon = Rect::new(
                        rect.min_x() + inset,
                        rect.min_y() + inset,
                        side - 2.0 * inset,
                        side - 2.0 * inset,
                    );
                    let color = b.palette.editor_background;
                    b.items.push(Item::Icon { rect: icon, name: "check", color });
                } else {
                    let width = b.px(1.5).max(1.0);
                    let color = b.palette.text_muted;
                    b.items.push(Item::Border { rect, color, width, radius });
                }
                let pad = b.px(4.0);
                let area = Rect::new(
                    rect.min_x() - pad,
                    rect.min_y() - pad,
                    side + 2.0 * pad,
                    side + 2.0 * pad,
                );
                b.hits.push((area, Hit::Task(range)));
                let copy = format!(
                    "{}[{}] ",
                    "  ".repeat(depth.saturating_sub(1)),
                    if done { "x" } else { " " }
                );
                b.blocks += 1;
                let sep = b.take_sep(Sep::Line);
                let to_text = Rect::new(rect.min_x(), b.y, x0 - rect.min_x(), line_h);
                b.run(to_text, String::new(), None, copy, sep);
                b.sep = Some(Sep::None);
            }
        }
    }

    pub(super) fn quote_start(&mut self, kind: Option<BlockQuoteKind>, range: Range<usize>) {
        self.flush_paragraph(false);
        let callout = match kind {
            Some(kind) => Some((Alert::from_gfm(kind), None, None)),
            None => callout(&self.source[range]),
        };
        let start = self.b.y;
        let Some((alert, title, fold)) = callout else {
            self.quotes.push(Quote { start, alert: None });
            return;
        };
        self.callout_line = kind.is_none();
        self.quotes.push(Quote { start, alert: Some(alert) });
        // The title: the icon and the type (or the callout's own title), in its color.
        let x0 = self.indent();
        let b = &mut self.b;
        let color = alert.color(b.palette);
        let size = b.body;
        let line_h = (size * 1.6).round();
        let icon_side = b.px(16.0);
        let icon = Rect::new(x0, (b.y + (line_h - icon_side) / 2.0).round(), icon_side, icon_side);
        b.items.push(Item::Icon { rect: icon, name: alert.icon(), color });
        let title = title.unwrap_or_else(|| alert.title());
        let fold = fold.map(|open| {
            let index = self.folds;
            self.folds += 1;
            (index, open != self.toggled.contains(&index))
        });
        let mut label = title;
        if let Some((_, open)) = fold {
            label.push_str(if open { "  ▾" } else { "  ▸" });
        }
        let spans = [Span::text(label, Style { bold: true, ..Style::default() }, None)];
        let top = b.y;
        b.flow(
            &spans,
            Run {
                x0: x0 + icon_side + b.px(8.0),
                x1: b.right,
                size,
                weight: Font::SEMIBOLD,
                color,
                line: 1.6,
                align: Align::Left,
            },
        );
        if let Some((index, open)) = fold {
            b.hits.push((Rect::new(x0, top, b.right - x0, b.y - top), Hit::Fold(index)));
            if !open {
                self.callout_line = false;
                self.skip = Some(Skip::Block { depth: 0 });
            }
        }
        self.b.y += self.b.px(4.0);
    }

    pub(super) fn quote_end(&mut self) {
        self.flush_paragraph(false);
        self.callout_line = false;
        let x = self.indent() - self.b.px(18.0);
        let gap = self.gap();
        let Some(quote) = self.quotes.pop() else { return };
        let b = &mut self.b;
        let bottom = (b.y - gap + b.px(2.0)).max(quote.start + b.px(8.0));
        let color = match quote.alert {
            Some(alert) => alert.color(b.palette),
            None => b.palette.accent.with_alpha(0.55),
        };
        b.items.push(Item::Rect {
            rect: Rect::new(x.max(b.left), quote.start, b.px(3.0), bottom - quote.start),
            color,
            radius: b.px(1.5),
        });
    }
}
