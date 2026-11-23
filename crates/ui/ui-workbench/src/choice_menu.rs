//! A list to choose from, in a popover: "Copy" (the ways to copy a file's path / reference),
//! a file's git history, the branches… Each row shows its name and, below it, a detail
//! (monospace, shortened in the middle when long, so both ends stay visible). It uses the
//! hover card's surface (tinted by the theme, distinct from the editor); long lists
//! scroll, by the wheel or the app's scrollbar (its thumb dragged a row at a time). Keys: ↑ ↓, ↵ chooses, esc closes; a click outside closes it (nothing is lost).

use std::time::Duration;

use i18n::t;
use icons::Icons;
use kernel::{Point, Rect};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{KeyHint, draw_icon, draw_text};

use crate::anim::Appear;
use crate::ds::{self, Ctx, Elevation, radius, space};
use crate::palette::Palette;

/// A row: its icon, name, detail (e.g. the value copied), shortcut (as shown) and group
/// (groups are separated by a small gap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub icon: &'static str,
    pub label: String,
    pub value: String,
    pub shortcut: Option<String>,
    pub group: u8,
}

/// At most this many rows are shown (more scroll).
const VISIBLE: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceAction {
    /// Row `i` was chosen (the menu closes).
    Pick(usize),
    /// Inside, on nothing (or on `esc`, see [`ChoiceMenu::over_esc`]).
    Inside,
    Close,
}

#[derive(Debug, Default)]
pub struct ChoiceMenu {
    open: bool,
    options: Vec<Choice>,
    title: String,
    /// What the list is about (e.g. the file's name), under the title.
    subject: String,
    /// The header's icon.
    icon: &'static str,
    /// What ↵ does, in the footer ("copy", "compare").
    action: String,
    /// The first row shown (long lists scroll).
    first: usize,
    /// Where it was asked for (the context menu's position); centered otherwise.
    anchor: Option<Point>,
    selected: usize,
    hover: Option<usize>,
    esc_hover: bool,
    viewport: Rect,
    scale: f32,
    panel: Rect,
    rows: Vec<Rect>,
    esc: Rect,
    appear: Appear,
    /// The list's scrollbar (long lists): the app's.
    bar: ui_components::EdgeScrollbar,
    /// The list's height per row, as last painted (the scrollbar's unit: a row).
    unit: f32,
}

impl ChoiceMenu {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the list: `header` = (title, subject, icon, the ↵ action's name).
    pub fn open(
        &mut self,
        header: (&str, &str, &'static str, &str),
        options: Vec<Choice>,
        anchor: Option<Point>,
    ) {
        if options.is_empty() {
            return;
        }
        let (title, subject, icon, action) = header;
        self.open = true;
        self.title = title.to_owned();
        self.subject = subject.to_owned();
        self.icon = icon;
        self.action = action.to_owned();
        self.first = 0;
        self.bar = ui_components::EdgeScrollbar::default();
        self.options = options;
        self.anchor = anchor;
        self.selected = 0;
        self.hover = None;
        self.appear.start(Duration::from_millis(120));
    }

    pub fn close(&mut self) {
        self.open = false;
        self.options = Vec::new();
        self.rows.clear();
    }

    pub fn is_animating(&self) -> bool {
        self.open && self.appear.animating()
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    pub fn contains(&self, p: Point) -> bool {
        self.open && self.panel.contains(p)
    }

    pub fn step(&mut self, delta: isize) {
        if !self.options.is_empty() {
            self.selected = ui_kit::step_index(self.selected, delta, self.options.len());
            if self.selected < self.first {
                self.first = self.selected;
            } else if self.selected >= self.first + VISIBLE {
                self.first = self.selected + 1 - VISIBLE;
            }
        }
    }

    /// Scrolls the list (physical pixels, positive = down). `true` if it moved.
    pub fn scroll(&mut self, dy: f32) -> bool {
        let rows = (dy / (self.row_height() * self.scale.max(0.5))).round() as isize;
        let max = self.options.len().saturating_sub(VISIBLE) as isize;
        let first = (self.first as isize + rows).clamp(0, max) as usize;
        let moved = first != self.first;
        self.first = first;
        moved
    }

    /// The selected row (Enter).
    pub fn confirm(&self) -> Option<usize> {
        (self.selected < self.options.len()).then_some(self.selected)
    }

    pub fn hit(&self, p: Point) -> Option<ChoiceAction> {
        if !self.open {
            return None;
        }
        if !self.panel.contains(p) || self.esc.contains(p) {
            return Some(ChoiceAction::Close);
        }
        if self.bar.contains(p) {
            return Some(ChoiceAction::Inside);
        }
        Some(match self.rows.iter().position(|r| r.contains(p)) {
            Some(i) => ChoiceAction::Pick(self.first + i),
            None => ChoiceAction::Inside,
        })
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.rows.iter().position(|r| r.contains(p)).map(|i| self.first + i);
        let esc = self.esc.contains(p);
        let bar = self.bar.hover(p);
        let changed = hover != self.hover || esc != self.esc_hover || bar;
        self.hover = hover;
        self.esc_hover = esc;
        if let Some(i) = hover {
            self.selected = i;
        }
        changed
    }

    /// A press on the list's scrollbar: `true` if it took it (a drag follows: the list
    /// follows the thumb, a row at a time).
    pub fn scrollbar_press(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.press(p) else { return false };
        self.scroll_to(offset);
        true
    }

    /// The scrollbar's thumb is held.
    pub fn scrollbar_held(&self) -> bool {
        self.bar.is_dragging()
    }

    /// The held thumb dragged to `p`: `true` if the list moved.
    pub fn scrollbar_drag(&mut self, p: Point) -> bool {
        self.bar.drag(p).is_some_and(|offset| self.scroll_to(offset))
    }

    /// The mouse released: `true` if the scrollbar was held.
    pub fn scrollbar_release(&mut self) -> bool {
        self.bar.release()
    }

    /// The list scrolled to `offset` (the scrollbar's, a row being [`Self::unit`]): the
    /// nearest row first, within the ends. `true` if it moved.
    fn scroll_to(&mut self, offset: f32) -> bool {
        let max = self.options.len().saturating_sub(VISIBLE);
        let first =
            if self.unit > 0.0 { (offset / self.unit).round().max(0.0) as usize } else { 0 };
        let first = first.min(max);
        let moved = first != self.first;
        self.first = first;
        moved
    }

    pub fn over_clickable(&self, p: Point) -> bool {
        self.esc.contains(p) || self.rows.iter().any(|r| r.contains(p))
    }

    pub fn paint(
        &mut self,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.rows.clear();
        if !self.open {
            return;
        }
        let shown = self.appear.value();
        let ctx = Ctx::new(palette, self.scale).with_opacity(shown);
        let px = |v: f32| ctx.px(v);

        let header_h = px(56.0);
        let row_h = px(self.row_height());
        let gap = px(space::SM);
        let footer_h = px(38.0);
        let pad = px(space::SM);
        let end = (self.first + VISIBLE).min(self.options.len());
        let shown_rows = &self.options[self.first..end];
        let groups = shown_rows.windows(2).filter(|w| w[0].group != w[1].group).count();
        let v = self.viewport;
        let width = px(480.0).min(v.size.width - px(32.0));
        let height =
            header_h + pad + shown_rows.len() as f32 * row_h + groups as f32 * gap + pad + footer_h;
        let (x, y) = match self.anchor {
            Some(a) => (a.x, a.y + px(4.0)),
            None => (v.min_x() + (v.size.width - width) / 2.0, v.min_y() + v.size.height * 0.18),
        };
        let x = x.clamp(v.min_x() + px(16.0), (v.max_x() - width - px(16.0)).max(v.min_x()));
        let y = y.min(v.max_y() - height - px(ds::POPOVER_GAP)).max(v.min_y() + px(8.0));
        self.panel =
            Rect::new(x.round(), (y + (1.0 - shown) * px(6.0)).round(), width.round(), height);
        let p = self.panel;
        let frame = Frame { panel: p, header_h, row_h, gap, pad, end };

        let layer = scene.push_layer(None);
        ds::surface_with(layer, &ctx, p, Elevation::High, radius::LG, palette.hover_background);
        let layer = scene.push_layer(Some(p));

        self.paint_header(&frame, icons, text, layer, &ctx);
        let bottom = self.paint_rows(&frame, icons, text, layer, &ctx);

        // Footer: the keys.
        let foot = Rect::new(p.min_x(), p.max_y() - footer_h, p.size.width, footer_h);
        let (choose, close) = (t("keys.choose"), t("keys.close"));
        let hints: &[KeyHint<'_>] =
            &[(&["↑", "↓"], &choose), (&["↵"], &self.action), (&["esc"], &close)];
        ds::footer(layer, text, &ctx, foot, hints);
        self.paint_scrollbar(&frame, bottom, scene, &ctx);
    }

    /// A row's height (logical): two lines when some row has a detail, one otherwise.
    fn row_height(&self) -> f32 {
        if self.options.iter().any(|o| !o.value.is_empty()) { 48.0 } else { 36.0 }
    }

    /// The header: the icon in an accent tile, the title and the file, `esc` on the right.
    fn paint_header(
        &mut self,
        frame: &Frame,
        icons: &Icons,
        text: &mut TextSystem,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) {
        let px = |v: f32| ctx.px(v);
        let palette = ctx.palette;
        let p = frame.panel;
        let title_font = ctx.ui_font(0.5).with_weight(Font::SEMIBOLD);
        let subject_font = ctx.ui_font(-1.0);
        let mid = p.min_y() + frame.header_h / 2.0;
        let tile = px(30.0);
        let tile_rect =
            Rect::new(p.min_x() + px(space::LG), (mid - tile / 2.0).round(), tile, tile);
        layer.quad(
            Quad::filled(tile_rect, ctx.fade(palette.accent.with_alpha(0.14)))
                .rounded(px(radius::SM + 1.0)),
        );
        if let Some(id) = icons.id(self.icon) {
            let size = px(16.0);
            draw_icon(
                layer,
                icons,
                id,
                Rect::new(
                    tile_rect.min_x() + (tile - size) / 2.0,
                    tile_rect.min_y() + (tile - size) / 2.0,
                    size,
                    size,
                ),
                ctx.fade(palette.accent),
            );
        }
        let esc =
            ds::keycap(layer, text, ctx, "esc", p.max_x() - px(space::LG), mid, self.esc_hover);
        self.esc = esc;
        let text_x = tile_rect.max_x() + px(space::MD);
        let room = esc.min_x() - px(space::MD) - text_x;
        let title = text.shape_line(&self.title, &title_font);
        let title_top = mid - px(17.0);
        draw_text(
            layer,
            &title,
            text_x,
            ds::optical_baseline(title_top, px(18.0), &title_font),
            ctx.fade(palette.text),
        );
        let subject = ds::ellipsize_middle(text, &self.subject, &subject_font, room);
        let shaped = text.shape_line(&subject, &subject_font);
        draw_text(
            layer,
            &shaped,
            text_x,
            ds::optical_baseline(mid + px(1.0), px(16.0), &subject_font),
            ctx.fade(palette.text_muted),
        );
        ds::divider(layer, ctx, p.min_x(), p.max_x(), p.min_y() + frame.header_h);
    }

    /// The rows shown: the name above, the detail below; returns the list's bottom.
    fn paint_rows(
        &mut self,
        frame: &Frame,
        icons: &Icons,
        text: &mut TextSystem,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) -> f32 {
        let px = |v: f32| ctx.px(v);
        let palette = ctx.palette;
        let (p, row_h, pad) = (frame.panel, frame.row_h, frame.pad);
        let label_font = ctx.ui_font(0.0);
        let value_font = ctx.code_font(-1.5);
        let mut top = p.min_y() + frame.header_h + pad;
        for (i, option) in self.options.iter().enumerate().take(frame.end).skip(self.first) {
            if i > self.first && self.options[i - 1].group != option.group {
                top += frame.gap;
            }
            let rect = Rect::new(p.min_x() + pad, top, p.size.width - pad * 2.0, row_h);
            let selected = i == self.selected;
            ds::RowHighlight::new(selected, false)
                .selected_alpha(CHOSEN_ALPHA)
                .paint(layer, ctx, rect);
            let inner = rect.min_x() + px(space::MD);
            if let Some(id) = icons.id(option.icon) {
                let size = px(16.0);
                let color = if selected { palette.accent } else { palette.text_muted };
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(inner, (rect.min_y() + (row_h - size) / 2.0).round(), size, size),
                    ctx.fade(color),
                );
            }
            let text_x = inner + px(28.0);
            let mut right = rect.max_x() - px(space::MD);
            if let Some(key) = &option.shortcut {
                let cap =
                    ds::keycap(layer, text, ctx, key, right, rect.min_y() + row_h / 2.0, false);
                right = cap.min_x() - px(space::SM);
            }
            let label = ds::ellipsize(text, &option.label, &label_font, right - text_x);
            let label = text.shape_line(&label, &label_font);
            // With no detail the name sits in the row's middle, level with the icon and key.
            let label_top = if option.value.is_empty() {
                rect.min_y() + (row_h - px(18.0)) / 2.0
            } else {
                rect.min_y() + px(6.0)
            };
            draw_text(
                layer,
                &label,
                text_x,
                ds::optical_baseline(label_top, px(18.0), &label_font),
                ctx.fade(palette.text),
            );
            let value = ds::ellipsize_middle(text, &option.value, &value_font, right - text_x);
            let shaped = text.shape_line(&value, &value_font);
            draw_text(
                layer,
                &shaped,
                text_x,
                ds::optical_baseline(rect.min_y() + px(25.0), px(16.0), &value_font),
                ctx.fade(if selected { palette.code_text } else { palette.text_muted }),
            );
            self.rows.push(rect);
            top += row_h;
        }
        top
    }

    /// The app's scrollbar when the list is longer than what's shown (the list ends at
    /// `bottom`): a row of the list is one of its units, whatever the gaps between groups.
    fn paint_scrollbar(&mut self, frame: &Frame, bottom: f32, scene: &mut Scene, ctx: &Ctx<'_>) {
        let count = self.options.len();
        if count <= VISIBLE {
            self.bar = ui_components::EdgeScrollbar::default();
            return;
        }
        let p = frame.panel;
        let list_top = p.min_y() + frame.header_h + frame.pad;
        let list =
            Rect::new(p.min_x(), list_top, p.size.width - self.scale.round(), bottom - list_top);
        self.unit = list.size.height / VISIBLE as f32;
        let content = self.unit * count as f32;
        let offset = self.unit * self.first as f32;
        self.bar.paint_in(scene, ctx, list, content, offset);
    }
}

/// The panel's geometry, shared by the pieces of [`ChoiceMenu::paint`].
struct Frame {
    panel: Rect,
    header_h: f32,
    row_h: f32,
    /// Between groups.
    gap: f32,
    /// Around the list.
    pad: f32,
    /// The end (exclusive) of the rows shown.
    end: usize,
}

/// The chosen option's back (the accent's alpha).
const CHOSEN_ALPHA: f32 = 0.12;

#[cfg(test)]
#[path = "../tests/unit/choice_menu.rs"]
mod tests;
