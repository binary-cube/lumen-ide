//! The completion list below the cursor: each suggestion with a kind badge (class,
//! method, property…), the name with matched letters highlighted, and the detail on the right.
//! A longer list scrolls (the wheel, the keys) with a scrollbar hidden at rest.

use std::time::Instant;

use code_intelligence::{CompletionItem, CompletionKind, Visibility};
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::EdgeScrollbar;
use ui_kit::{KeyHint, centered_baseline, draw_text, paint_key_hints};

use crate::palette::Palette;

/// How many rows are visible at once.
const VISIBLE: usize = 10;

#[derive(Debug, Default)]
pub struct CompletionPopup {
    open: bool,
    items: Vec<CompletionItem>,
    /// What the user typed (for highlighting the matched letters).
    query: String,
    selected: usize,
    first: usize,
    /// The cursor's bottom-left corner (physical pixels): the list appears below it.
    anchor: Point,
    caret_height: f32,
    viewport: Rect,
    scale: f32,
    rect: Rect,
    row_h: f32,
    /// The list's scrollbar, hidden at rest (over the rows' right edge: the layout stays).
    bar: EdgeScrollbar,
    /// What the wheel scrolled that doesn't make a whole row yet (physical pixels).
    wheel: f32,
}

impl CompletionPopup {
    /// The suggestions shown, best first.
    pub fn items(&self) -> &[CompletionItem] {
        &self.items
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(
        &mut self,
        items: Vec<CompletionItem>,
        query: String,
        caret: Rect,
        viewport: Rect,
        scale: f32,
    ) {
        if items.is_empty() {
            self.close();
            return;
        }
        // Keep the selection on the same suggestion if the list was only filtered.
        let previous = self.selected().map(|i| i.label.clone());
        if !self.open {
            self.bar = EdgeScrollbar::auto_hiding();
            self.wheel = 0.0;
        }
        self.selected = previous.and_then(|l| items.iter().position(|i| i.label == l)).unwrap_or(0);
        self.items = items;
        self.query = query;
        self.first = 0;
        self.anchor = Point::new(caret.min_x(), caret.max_y());
        self.caret_height = caret.size.height;
        self.viewport = viewport;
        self.scale = scale;
        self.open = true;
        self.keep_visible();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.items.clear();
    }

    pub fn selected(&self) -> Option<&CompletionItem> {
        self.items.get(self.selected).filter(|_| self.open)
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        self.selected = ui_kit::step_index(self.selected, delta, self.items.len());
        self.keep_visible();
    }

    fn keep_visible(&mut self) {
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + VISIBLE {
            self.first = self.selected + 1 - VISIBLE;
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        self.open && self.rect.contains(p)
    }

    /// Where it's drawn, while open.
    pub fn bounds(&self) -> Option<Rect> {
        self.open.then_some(self.rect)
    }

    /// The row under point `p` (also selects it).
    pub fn click(&mut self, p: Point) -> Option<&CompletionItem> {
        if !self.contains(p) || self.row_h <= 0.0 {
            return None;
        }
        let row = ((p.y - self.rect.min_y() - (4.0 * self.scale).round()) / self.row_h)
            .floor()
            .max(0.0) as usize;
        if row >= self.items.len().min(VISIBLE) {
            return None;
        }
        let index = self.first + row;
        (index < self.items.len()).then(|| {
            self.selected = index;
            &self.items[index]
        })
    }

    pub fn scroll(&mut self, rows: isize) {
        let max = self.items.len().saturating_sub(VISIBLE) as isize;
        self.first = (self.first as isize + rows).clamp(0, max) as usize;
    }

    /// The wheel over the list (physical pixels, positive = down): whole rows at a time,
    /// the rest kept for the next turn. `true` if the list moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        if !self.open || self.row_h <= 0.0 {
            return false;
        }
        self.wheel += dy;
        let rows = (self.wheel / self.row_h).trunc();
        if rows == 0.0 {
            return false;
        }
        self.wheel -= rows * self.row_h;
        let before = self.first;
        self.scroll(rows as isize);
        if self.first == before {
            self.wheel = 0.0;
        }
        self.first != before
    }

    /// The list's top at `offset` pixels (from its scrollbar): the nearest row.
    fn scroll_to(&mut self, offset: f32) -> bool {
        let max = self.items.len().saturating_sub(VISIBLE);
        let first = ((offset / self.row_h.max(1.0)).round().max(0.0) as usize).min(max);
        let moved = first != self.first;
        self.first = first;
        moved
    }

    /// The mouse moved (anywhere: leaving it, its scrollbar narrows). `true`: redraw.
    pub fn hover(&mut self, p: Point) -> bool {
        self.open && self.bar.hover(p)
    }

    /// A press on the list's scrollbar: `true` if it took it (a drag follows).
    pub fn scrollbar_press(&mut self, p: Point) -> bool {
        if !self.contains(p) {
            return false;
        }
        let Some(offset) = self.bar.press(p) else { return false };
        self.scroll_to(offset);
        true
    }

    pub fn scrollbar_held(&self) -> bool {
        self.open && self.bar.is_dragging()
    }

    /// The held scrollbar's thumb dragged to `p`: `true` if the list moved.
    pub fn scrollbar_drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        self.scroll_to(offset)
    }

    /// The mouse released: `true` if the scrollbar was held.
    pub fn scrollbar_release(&mut self) -> bool {
        self.bar.release()
    }

    /// When its scrollbar next needs a frame (fading in or out, its hold ending); `None`
    /// at rest.
    pub fn frame_deadline(&self) -> Option<Instant> {
        self.open.then(|| self.bar.next_frame()).flatten()
    }

    pub fn paint(&mut self, text: &mut TextSystem, scene: &mut Scene, palette: &Palette) {
        if !self.open {
            return;
        }
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let line = s.round().max(1.0);
        let font = Font::new(palette.code_font.clone(), px(palette.code_font_size));
        let small = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.0));
        let badge_font = Font::new(palette.ui_font.clone(), px(10.0)).with_weight(Font::BOLD);
        let metrics = text.metrics(&font);
        let small_metrics = text.metrics(&small);
        self.row_h = px(26.0);
        let pad = px(4.0);
        let rows = self.items.len().min(VISIBLE);

        // Width: the longest label + the detail, between 280 and 560 logical pixels.
        let widest = self.items.iter().take(40).map(|i| {
            let label = text.shape_line(&i.label, &font).width;
            let detail = i
                .detail
                .as_deref()
                .map_or(0.0, |d| text.shape_line(d, &small).width.min(px(220.0)));
            label + detail
        });
        let content = widest.fold(0.0_f32, f32::max);
        let width = (content + px(70.0))
            .clamp(px(280.0), px(560.0))
            .min(self.viewport.size.width - px(16.0));
        // The footer with the navigation keys.
        let footer_h = px(28.0);
        let height = rows as f32 * self.row_h + 2.0 * pad + footer_h;
        let mut x = self.anchor.x - px(30.0);
        x = x.min(self.viewport.max_x() - width - px(8.0)).max(self.viewport.min_x() + px(4.0));
        // Below the cursor; if it doesn't fit, above.
        let below = self.anchor.y + px(2.0);
        let y = if below + height > self.viewport.max_y() {
            self.anchor.y - self.caret_height - height - px(2.0)
        } else {
            below
        };
        self.rect = Rect::new(x.round(), y.round(), width.round(), height);
        let r = self.rect;

        let layer = scene.push_layer(None);
        let ctx = crate::ds::Ctx::new(palette, s);
        crate::ds::surface(layer, &ctx, r, crate::ds::Elevation::Medium, 8.0);

        let layer = scene.push_layer(Some(r));
        let foot = Rect::new(r.min_x(), r.max_y() - footer_h, r.size.width, footer_h);
        layer.quad(Quad::filled(
            Rect::new(foot.min_x(), foot.min_y(), foot.size.width, line),
            palette.border,
        ));
        let (choose, insert, close) =
            (i18n::t("keys.choose"), i18n::t("keys.insert"), i18n::t("keys.close"));
        let hints: &[KeyHint<'_>] =
            &[(&["↑", "↓"], &choose), (&["↵"], &insert), (&["esc"], &close)];
        let row = Rect::new(
            foot.min_x() + px(10.0),
            foot.min_y() + line,
            foot.size.width - px(20.0),
            footer_h - line,
        );
        paint_key_hints(layer, text, hints, row, &palette.key_hints(s));
        for (row, item) in self.items.iter().enumerate().skip(self.first).take(VISIBLE) {
            let top = r.min_y() + pad + (row - self.first) as f32 * self.row_h;
            let row_rect = Rect::new(r.min_x() + pad, top, r.size.width - 2.0 * pad, self.row_h);
            if row == self.selected {
                layer.quad(Quad::filled(row_rect, palette.selection).rounded(px(5.0)));
            }
            // The kind badge.
            let (letter, color) = badge(item.kind, palette);
            let size = px(16.0);
            let badge_rect =
                Rect::new(row_rect.min_x() + px(6.0), top + (self.row_h - size) / 2.0, size, size);
            layer.quad(Quad::filled(badge_rect, color.with_alpha(0.22)).rounded(px(4.0)));
            let shaped = text.shape_line(letter, &badge_font);
            let bm = text.metrics(&badge_font);
            draw_text(
                layer,
                &shaped,
                (badge_rect.min_x() + (size - shaped.width) / 2.0).round(),
                centered_baseline(badge_rect.min_y(), size, &bm),
                color,
            );

            // Member visibility: a small dot over the badge corner (green public,
            // yellow protected, red private).
            if let Some(visibility) = item.visibility {
                let color = match visibility {
                    Visibility::Public => palette.terminal_ansi[10],
                    Visibility::Protected => palette.terminal_ansi[11],
                    Visibility::Private => palette.terminal_ansi[9],
                };
                let dot = px(7.0);
                let r = Rect::new(
                    badge_rect.max_x() - dot * 0.6,
                    badge_rect.max_y() - dot * 0.6,
                    dot,
                    dot,
                );
                layer.quad(
                    Quad::filled(r, color)
                        .rounded(dot / 2.0)
                        .bordered(px(1.0).max(1.0), palette.popup_background),
                );
            }
            // The label, with the matched letters accented.
            let baseline = centered_baseline(top, self.row_h, &metrics);
            let mut x = badge_rect.max_x() + px(8.0);
            let matched = matched_chars(&self.query, &item.label);
            for (i, ch) in item.label.char_indices() {
                let glyph = &item.label[i..i + ch.len_utf8()];
                let shaped = text.shape_line(glyph, &font);
                let color = if matched.contains(&i) { palette.accent } else { palette.text };
                draw_text(layer, &shaped, x.round(), baseline, color);
                x += shaped.width;
            }
            // The detail, right-aligned inside the row: cut with "…" where it doesn't fit.
            let room = row_rect.max_x() - px(8.0) - (x + px(16.0));
            if let Some(detail) = item.detail.as_deref().filter(|_| room > px(24.0)) {
                let detail = crate::ds::ellipsize(text, detail, &small, room);
                let shaped = text.shape_line(&detail, &small);
                let dx = row_rect.max_x() - px(8.0) - shaped.width;
                let sb = centered_baseline(top, self.row_h, &small_metrics);
                draw_text(layer, &shaped, dx.round(), sb, palette.text_muted);
            }
        }
        // The scrollbar along the rows' right edge (nothing when they all fit).
        let list = Rect::new(r.min_x(), r.min_y() + pad, r.size.width, rows as f32 * self.row_h);
        let content = self.items.len() as f32 * self.row_h;
        let offset = self.first as f32 * self.row_h;
        self.bar.paint(scene, palette, s, list, content, offset);
    }
}

/// The badge letter and color for a suggestion kind.
pub(crate) fn badge(kind: CompletionKind, p: &Palette) -> (&'static str, Rgba) {
    match kind {
        CompletionKind::Class => ("C", p.terminal_ansi[12]),
        CompletionKind::Interface => ("I", p.terminal_ansi[14]),
        CompletionKind::Method => ("m", p.terminal_ansi[13]),
        CompletionKind::Function => ("f", p.terminal_ansi[13]),
        CompletionKind::Property => ("p", p.terminal_ansi[11]),
        CompletionKind::Constant => ("K", p.terminal_ansi[9]),
        CompletionKind::Variable => ("$", p.terminal_ansi[10]),
        CompletionKind::Keyword => ("k", p.text_muted),
        CompletionKind::Tag => ("<>", p.terminal_ansi[12]),
        CompletionKind::Attribute => ("a", p.terminal_ansi[11]),
        CompletionKind::Value => ("v", p.terminal_ansi[10]),
        CompletionKind::Selector => (".", p.terminal_ansi[14]),
        CompletionKind::Table => ("T", p.terminal_ansi[12]),
        CompletionKind::View => ("V", p.terminal_ansi[14]),
        CompletionKind::Column => ("c", p.terminal_ansi[11]),
        CompletionKind::Schema => ("S", p.terminal_ansi[13]),
        CompletionKind::Snippet => ("s", p.accent),
        CompletionKind::Namespace => ("N", p.terminal_ansi[14]),
    }
}

/// Positions (bytes) in `label` matching `query` as a subsequence (for highlighting).
fn matched_chars(query: &str, label: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut chars = label.char_indices();
    for q in query.chars().flat_map(char::to_lowercase) {
        for (i, c) in chars.by_ref() {
            if c.to_lowercase().next() == Some(q) {
                out.push(i);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/completion_popup.rs"]
mod tests;
