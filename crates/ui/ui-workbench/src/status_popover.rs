//! The status bar's small windows: what an item of the bar holds, shown over it while the
//! mouse is on the item or on the window (the file's language, encoding, line endings,
//! indentation; what the app costs, as the last minute's graphs). Rows of a label and a
//! value (a click runs the row's command, when it has one) or of a label, a graph and a
//! value. One can be pinned: it stays open, whatever the mouse does, until unpinned.
//! A row may lead with an icon. A window of graphs keeps room for its figures (and never
//! narrows while open), so values that change don't make it jump.
//!
//! It floats: drawn over everything, never taking the keys, blocking nothing; only the
//! mouse on its own area reaches it. The rows are given at each paint (made from what the
//! app already holds: nothing read for them).

use std::time::Duration;

use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::TextSystem;
use ui_components::chart::{self, Plot};
use ui_kit::{draw_icon, draw_text};

use crate::anim::Appear;
use crate::ds::{self, Ctx, Elevation, radius};
use crate::palette::Palette;

/// A row of the window.
#[derive(Debug, Clone)]
pub enum PopoverRow {
    /// A label and its value; a click runs `action`, when it has one.
    Value { label: String, value: String, action: Option<&'static str>, icon: Option<&'static str> },
    /// A label, the values over time as a graph (on a scale up to `max`), the value now.
    Series {
        label: String,
        plot: Plot,
        max: f32,
        color: Rgba,
        value: String,
        icon: Option<&'static str>,
    },
}

impl PopoverRow {
    fn icon(&self) -> Option<&'static str> {
        match self {
            Self::Value { icon, .. } | Self::Series { icon, .. } => *icon,
        }
    }
}

/// A pinned window's pill (the resources kept in a corner): a value in its color, and
/// maybe a second one after a bar (the RAM: the app's, then the total the system shows).
#[derive(Debug, Clone)]
pub struct Pill {
    pub label: String,
    pub value: String,
    pub color: Rgba,
    pub more: Option<String>,
}

/// What a click on the window hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopoverHit {
    /// A row with a command: run it.
    Run(&'static str),
    /// The pin: pinned, or unpinned.
    Pin,
    /// Elsewhere on it.
    Inside,
}

/// Logical sizes.
const ROW_H: f32 = 30.0;
const SERIES_H: f32 = 30.0;
const PAD: f32 = 8.0;
const WIDTH_MIN: f32 = 240.0;
const GRAPH_W: f32 = 110.0;
const HEADER_H: f32 = 26.0;
const ICON: f32 = 15.0;
const ICON_GAP: f32 = 8.0;
/// The figures a window of graphs keeps room for (in the code font: digits as wide).
const FIGURES_ROOM: &str = "99 · 9999 MB";
/// The room each pill's figure keeps (so the pills don't move as values change).
const PILL_ROOM: &str = "9999 MB";

/// An item's window (see the module's doc).
#[derive(Debug, Default)]
pub struct StatusPopover {
    /// The item it's open for (its command in the bar) and where that item is.
    open: Option<(&'static str, Rect)>,
    pinned: bool,
    appear: Appear,
    panel: Rect,
    /// The rows with a command (from the last paint), and the pin.
    rows: Vec<(Rect, &'static str)>,
    pin: Option<Rect>,
    /// What it drew, as shapes (rect, corners' radius; physical): its window, or its pills
    /// and pin — what must show over a page of the system's web engine.
    shapes: Vec<(Rect, f32)>,
    hovered: Option<&'static str>,
    hover_pin: bool,
    /// The values' column at its widest since it opened (physical): it never narrows.
    value_room: f32,
}

impl StatusPopover {
    /// Opened (or moved) over the item of command `owner`, at `anchor`; another item's
    /// window has its own pin (the one before kept none of it).
    pub fn open(&mut self, owner: &'static str, anchor: Rect) {
        if self.open.is_none_or(|(o, _)| o != owner) {
            self.appear.start(Duration::from_millis(90));
            self.hovered = None;
            self.pinned = false;
            self.value_room = 0.0;
        }
        self.open = Some((owner, anchor));
    }

    /// The pin under `p` (its area, for its tooltip).
    pub fn pin_at(&self, p: Point) -> Option<Rect> {
        self.pin.filter(|r| self.is_open() && r.contains(p))
    }

    /// Closed (unpinned with it).
    pub fn close(&mut self) {
        self.open = None;
        self.pinned = false;
        self.rows.clear();
        self.pin = None;
        self.shapes.clear();
        self.panel = Rect::default();
        self.value_room = 0.0;
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The item it's open for.
    pub fn owner(&self) -> Option<&'static str> {
        self.open.map(|(o, _)| o)
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    /// Whether `p` is on it (as last drawn).
    pub fn contains(&self, p: Point) -> bool {
        self.is_open() && self.panel.contains(p)
    }

    /// What it shows, as shapes (rect, corners' radius; physical): its window, or each
    /// pill and the pin.
    pub fn shapes(&self) -> &[(Rect, f32)] {
        if self.is_open() { &self.shapes } else { &[] }
    }

    /// Its area while it shows.
    pub fn bounds(&self) -> Option<Rect> {
        (self.is_open() && !self.panel.is_empty()).then_some(self.panel)
    }

    /// Whether the mouse at `p` keeps it open: on it, on its item, or on the way between
    /// them — the gap over the bar, as wide as the item, reaching the window's edge when the
    /// window stands left of it (kept off the side's buttons: the way goes up and left, beside
    /// the window, to its pin). A pinned one always is.
    pub fn keeps(&self, p: Point) -> bool {
        match self.open {
            Some(_) if self.pinned => true,
            Some((_, anchor)) => {
                let panel = self.panel;
                let left = anchor.min_x().min(panel.max_x() - anchor.size.width);
                let way = Rect::new(
                    left,
                    panel.min_y(),
                    (anchor.max_x() - left).max(0.0),
                    (anchor.min_y() - panel.min_y()).max(0.0),
                );
                panel.contains(p) || anchor.contains(p) || way.contains(p)
            }
            None => false,
        }
    }

    pub fn hit(&self, p: Point) -> Option<PopoverHit> {
        if !self.contains(p) {
            return None;
        }
        if self.pin.is_some_and(|r| r.contains(p)) {
            return Some(PopoverHit::Pin);
        }
        Some(match self.rows.iter().find(|(r, _)| r.contains(p)) {
            Some((_, action)) => PopoverHit::Run(action),
            None => PopoverHit::Inside,
        })
    }

    /// The mouse at `p`: `true` if what's lit changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let row = self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, a)| *a);
        let pin = self.pin.is_some_and(|r| r.contains(p));
        let changed = row != self.hovered || pin != self.hover_pin;
        (self.hovered, self.hover_pin) = (row, pin);
        changed
    }

    /// Over something it runs (the hand cursor).
    pub fn over_button(&self) -> bool {
        self.hovered.is_some() || self.hover_pin
    }

    /// Drawn as pills side by side, ending at `corner` (its bottom-right, physical), the pin
    /// last (a click unpins): a pinned window made small, in a corner of the editor.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_pills(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &Icons,
        palette: &Palette,
        scale: f32,
        corner: Point,
        pills: &[Pill],
    ) {
        self.rows.clear();
        self.pin = None;
        self.shapes.clear();
        if self.open.is_none() || pills.is_empty() {
            self.panel = Rect::default();
            return;
        }
        let ctx = Ctx { opacity: self.appear.value(), ..Ctx::new(palette, scale) };
        let px = |v: f32| (v * scale).round();
        let font = ctx.ui_font(-1.0);
        let mono =
            text_layout::Font::new(palette.code_font.clone(), px(palette.ui_font_size - 1.5));
        let h = px(24.0);
        let gap = px(4.0);
        let dot = px(7.0);
        let room = text.shape_line(PILL_ROOM, &mono).width;
        let figure = |text: &mut TextSystem, v: &str| text.shape_line(v, &mono).width.max(room);
        // The bar between a pill's two values, and the space on each side of it.
        let split = px(8.0);
        // Their widths first: they end at the corner, the pin last.
        let widths: Vec<f32> = pills
            .iter()
            .map(|p| {
                let more = p.more.as_deref().map_or(0.0, |m| split * 2.0 + figure(text, m));
                px(10.0)
                    + dot
                    + px(6.0)
                    + text.shape_line(&p.label, &font).width
                    + px(6.0)
                    + figure(text, &p.value)
                    + more
                    + px(10.0)
            })
            .collect();
        let pin_w = h;
        let total = widths.iter().sum::<f32>() + gap * widths.len() as f32 + pin_w;
        let top = (corner.y - h).round();
        let mut x = (corner.x - total).round();
        self.panel = Rect::new(x, top, total.round(), h);
        let layer = scene.push_layer(None);
        let face = |layer: &mut render_api::Layer, r: Rect| {
            ds::surface_with(layer, &ctx, r, Elevation::Low, h / 2.0, palette.popup_background);
        };
        for (pill, w) in pills.iter().zip(&widths) {
            let r = Rect::new(x, top, w.round(), h);
            face(layer, r);
            self.shapes.push((r, h / 2.0));
            let mut cx = r.min_x() + px(10.0);
            let d = Rect::new(cx, (top + (h - dot) / 2.0).round(), dot, dot);
            layer.quad(Quad::filled(d, ctx.fade(pill.color)).rounded(dot / 2.0));
            cx += dot + px(6.0);
            let label = text.shape_line(&pill.label, &font);
            let base = ds::optical_baseline(top, h, &font);
            draw_text(layer, &label, cx.round(), base, ctx.fade(palette.text_muted));
            cx += label.width + px(6.0);
            // The figures right-aligned in their room: the digits stay put.
            let value = text.shape_line(&pill.value, &mono);
            let width = figure(text, &pill.value);
            let at = cx + width - value.width;
            draw_text(layer, &value, at.round(), base, ctx.fade(palette.text));
            cx += width;
            if let Some(more) = &pill.more {
                let bar =
                    Rect::new((cx + split).round(), top + px(6.0), px(1.0).max(1.0), h - px(12.0));
                layer.quad(Quad::filled(bar, ctx.fade(palette.text_muted.with_alpha(0.4))));
                cx += split * 2.0;
                let value = text.shape_line(more, &mono);
                let width = figure(text, more);
                let at = cx + width - value.width;
                draw_text(layer, &value, at.round(), base, ctx.fade(palette.text_muted));
            }
            x += w + gap;
        }
        // The pin, lit (it's pinned): a click lets go.
        let pin = Rect::new(x.round(), top, pin_w, h);
        face(layer, pin);
        self.shapes.push((pin, h / 2.0));
        let lit = palette.accent.with_alpha(if self.hover_pin { 0.26 } else { 0.18 });
        layer.quad(Quad::filled(pin, ctx.fade(lit)).rounded(h / 2.0));
        if let Some(id) = icons.id("pin") {
            let s = px(13.0);
            let at = Rect::new(
                (pin.min_x() + (pin_w - s) / 2.0).round(),
                (top + (h - s) / 2.0).round(),
                s,
                s,
            );
            draw_icon(layer, icons, id, at, ctx.fade(palette.accent));
        }
        self.pin = Some(pin);
    }

    pub fn is_animating(&self) -> bool {
        self.is_open() && self.appear.animating()
    }

    /// Draws it with `rows` above its item, its right edge on the item's, `gap` (physical)
    /// over the bar's top `bar_top`, within `viewport`; `title` and a pin when `pinnable`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &Icons,
        palette: &Palette,
        scale: f32,
        viewport: Rect,
        bar_top: f32,
        rows: &[PopoverRow],
        title: Option<&str>,
        pinnable: bool,
    ) {
        self.rows.clear();
        self.pin = None;
        self.shapes.clear();
        let Some((_, anchor)) = self.open else { return };
        if rows.is_empty() {
            self.panel = Rect::default();
            return;
        }
        let ctx = Ctx { opacity: self.appear.value(), ..Ctx::new(palette, scale) };
        let px = |v: f32| (v * scale).round();
        let font = ctx.ui_font(-0.5);
        let mono =
            text_layout::Font::new(palette.code_font.clone(), px(palette.ui_font_size - 1.0));
        // A window of graphs: every value in the code font (digits as wide: they don't
        // jump), with room kept for the figures.
        let series = rows.iter().any(|r| matches!(r, PopoverRow::Series { .. }));
        let value_font = if series { &mono } else { &font };
        // Icons: every label moved by their slot, so the labels stay aligned.
        let icon_slot =
            if rows.iter().any(|r| r.icon().is_some()) { px(ICON) + px(ICON_GAP) } else { 0.0 };
        // The width: the widest label and value, or a graph's row.
        let mut label_w: f32 = 0.0;
        let mut value_w: f32 =
            if series { text.shape_line(FIGURES_ROOM, &mono).width } else { 0.0 };
        for row in rows {
            let (label, value) = match row {
                PopoverRow::Value { label, value, .. }
                | PopoverRow::Series { label, value, .. } => (label, value),
            };
            label_w = label_w.max(text.shape_line(label, &font).width);
            value_w = value_w.max(text.shape_line(value, value_font).width);
        }
        label_w += icon_slot;
        value_w = value_w.max(self.value_room);
        self.value_room = value_w;
        let inner = if series {
            label_w + px(16.0) + px(GRAPH_W) + px(16.0) + value_w
        } else {
            label_w + px(32.0) + value_w
        };
        let width = (inner + px(PAD) * 2.0 + px(16.0)).max(px(WIDTH_MIN));
        let header = if title.is_some() || pinnable { px(HEADER_H) } else { 0.0 };
        let rows_h: f32 = rows
            .iter()
            .map(|r| match r {
                PopoverRow::Value { .. } => px(ROW_H),
                PopoverRow::Series { .. } => px(SERIES_H),
            })
            .sum();
        let height = header + rows_h + px(PAD) * 2.0;
        let x = (anchor.max_x() - width)
            .min(viewport.max_x() - width - px(8.0))
            .max(viewport.min_x() + px(8.0));
        let y = (bar_top - px(ds::POPOVER_GAP) - height).max(viewport.min_y() + px(8.0));
        let panel = Rect::new(x.round(), y.round(), width.round(), height.round());
        self.panel = panel;
        self.shapes.push((panel, px(radius::MD)));

        let layer = scene.push_layer(None);
        ds::surface_with(
            layer,
            &ctx,
            panel,
            Elevation::Medium,
            radius::MD,
            palette.popup_background,
        );
        let layer = scene.push_layer(Some(panel));
        let left = panel.min_x() + px(PAD) + px(8.0);
        let right = panel.max_x() - px(PAD) - px(8.0);
        let mut top = panel.min_y() + px(PAD);
        if header > 0.0 {
            if let Some(title) = title {
                let bold = ctx.ui_font(-0.5).with_weight(text_layout::Font::SEMIBOLD);
                let shaped = text.shape_line(title, &bold);
                let baseline = ds::optical_baseline(top, header, &bold);
                draw_text(layer, &shaped, left, baseline, ctx.fade(palette.text));
            }
            if pinnable && let Some(id) = icons.id("pin") {
                let size = px(15.0);
                let button = Rect::new(
                    right - px(22.0) + px(3.5),
                    top + (header - px(22.0)) / 2.0,
                    px(22.0),
                    px(22.0),
                );
                // Pinned: lit in the accent (as a pressed button), plain otherwise.
                if self.pinned {
                    let lit = palette.accent.with_alpha(if self.hover_pin { 0.26 } else { 0.18 });
                    layer.quad(Quad::filled(button, ctx.fade(lit)).rounded(px(radius::SM)));
                } else if self.hover_pin {
                    layer.quad(
                        Quad::filled(button, ctx.fade(palette.tab_hover)).rounded(px(radius::SM)),
                    );
                }
                let color = if self.pinned { palette.accent } else { palette.text_muted };
                let at = Rect::new(
                    (button.min_x() + (button.size.width - size) / 2.0).round(),
                    (button.min_y() + (button.size.height - size) / 2.0).round(),
                    size,
                    size,
                );
                draw_icon(layer, icons, id, at, ctx.fade(color));
                self.pin = Some(button);
            }
            top += header;
        }
        let icon_at = |top: f32, h: f32| {
            let size = px(ICON);
            Rect::new(left, (top + (h - size) / 2.0).round(), size, size)
        };
        for row in rows {
            match row {
                PopoverRow::Value { label, value, action, icon } => {
                    let h = px(ROW_H);
                    let r = Rect::new(
                        panel.min_x() + px(PAD),
                        top,
                        panel.size.width - px(PAD) * 2.0,
                        h,
                    );
                    if let Some(action) = action {
                        if self.hovered == Some(*action) {
                            layer.quad(
                                Quad::filled(r, ctx.fade(palette.accent.with_alpha(0.14)))
                                    .rounded(px(radius::SM)),
                            );
                        }
                        self.rows.push((r, action));
                    }
                    if let Some(id) = icon.and_then(|i| icons.id(i)) {
                        draw_icon(layer, icons, id, icon_at(top, h), ctx.fade(palette.text_muted));
                    }
                    let shaped = text.shape_line(label, &font);
                    let baseline = ds::optical_baseline(top, h, &font);
                    let x = left + icon_slot;
                    draw_text(layer, &shaped, x, baseline, ctx.fade(palette.text_muted));
                    let shaped = text.shape_line(value, value_font);
                    let color =
                        if action.is_some() { palette.text } else { palette.text.with_alpha(0.85) };
                    draw_text(layer, &shaped, right - shaped.width, baseline, ctx.fade(color));
                    top += h;
                }
                PopoverRow::Series { label, plot, max, color, value, icon } => {
                    let h = px(SERIES_H);
                    // Its icon in its graph's color: the two read together.
                    if let Some(id) = icon.and_then(|i| icons.id(i)) {
                        draw_icon(layer, icons, id, icon_at(top, h), ctx.fade(*color));
                    }
                    let shaped = text.shape_line(label, &font);
                    let baseline = ds::optical_baseline(top, h, &font);
                    let x = left + icon_slot;
                    draw_text(layer, &shaped, x, baseline, ctx.fade(palette.text_muted));
                    let shaped_value = text.shape_line(value, &mono);
                    draw_text(
                        layer,
                        &shaped_value,
                        right - shaped_value.width,
                        baseline,
                        ctx.fade(palette.text),
                    );
                    let graph = Rect::new(
                        left + label_w + px(16.0),
                        top + px(8.0),
                        (right - value_w - px(16.0)) - (left + label_w + px(16.0)),
                        h - px(16.0),
                    );
                    chart::sparkline(layer, &ctx, graph, plot, *color, chart::Style::Steps, *max);
                    top += h;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/status_popover.rs"]
mod tests;
