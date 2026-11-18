//! The bottom status bar: the current file, the cursor position, the language, the
//! indentation, the encoding, the line terminator and the window's resource usage. Each
//! item can have an icon.
//!
//! When the window is too narrow for all of them, the items on the right go by their
//! [`Priority`] (the spare ones first, then the normal ones; the essential ones never), the
//! farthest from the corner first, an item [`joined`](StatusItem::joined) to the one before
//! it with it; a `…` takes their place, and a click on it ([`MORE`]) lists them
//! ([`StatusBar::hidden`]). The file's path on the left keeps [`LEFT_MIN`] at least.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{centered_baseline, draw_icon, draw_text};

use crate::palette::Palette;

/// The bar height, in logical pixels.
const HEIGHT: f32 = 32.0;
/// What the left side (the file's path) keeps at least while the right one is cut
/// (logical pixels).
pub const LEFT_MIN: f32 = 160.0;
/// The command of the `…` standing for the items that don't fit (the app lists them).
pub const MORE: &str = "status.more";

/// How much an item matters when the bar is too narrow for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    /// Goes first (the zoom, the language level).
    Spare,
    #[default]
    Normal,
    /// Never goes (the padlock, the errors, the cursor's place, work going on).
    Essential,
}

/// A bar item: icon (optional, with its own color) and text.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItem {
    pub icon: Option<(IconId, Option<Rgba>)>,
    pub text: String,
    /// The command issued on click (the item looks like a button on hover).
    pub action: Option<&'static str>,
    /// The width reserved for the text: that of this template (e.g. `"100,0%"`). Frequently
    /// changing values (CPU, memory, zoom) no longer shift the rest of the bar, nor themselves.
    pub reserve: Option<&'static str>,
    /// Starts a group: a thin separator is drawn before it.
    pub group: bool,
    /// A path: cut in the middle (its file name kept), at most [`crate::ds::PATH_MAX`] wide.
    pub path: bool,
    /// What its tooltip says (a module's indicator: why it shows).
    pub tooltip: Option<String>,
    /// How much it matters when they don't all fit.
    pub priority: Priority,
    /// Goes and stays with the item before it (the zoom's `−`, `100%`, `+`).
    pub joined: bool,
    /// As a row of the `…`'s list: its label (else its text, else its command's) and its
    /// icon's name.
    pub menu_label: Option<String>,
    pub menu_icon: Option<&'static str>,
    /// Drawn as small bars instead of its text (what the app costs): each a height
    /// (0–1) and a color, gliding to a new height when it changes.
    pub meter: Vec<(f32, Rgba)>,
}

impl StatusItem {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            icon: None,
            text: text.into(),
            action: None,
            reserve: None,
            group: false,
            path: false,
            tooltip: None,
            priority: Priority::Normal,
            joined: false,
            menu_label: None,
            menu_icon: None,
            meter: Vec::new(),
        }
    }

    /// Small bars of `heights` (0–1) in their colors (see [`Self::meter`]).
    pub fn meter(bars: Vec<(f32, Rgba)>) -> Self {
        Self { meter: bars, ..Self::text("") }
    }

    pub fn with_icon(icon: Option<IconId>, text: impl Into<String>) -> Self {
        Self { icon: icon.map(|i| (i, None)), ..Self::text(text) }
    }

    /// With the set's icon `name` (also its row's, under `…`).
    pub fn named(icons: &Icons, name: &'static str, text: impl Into<String>) -> Self {
        Self { menu_icon: Some(name), ..Self::with_icon(icons.id(name), text) }
    }

    /// It never goes ([`Priority::Essential`]).
    pub fn essential(mut self) -> Self {
        self.priority = Priority::Essential;
        self
    }

    /// It goes first ([`Priority::Spare`]).
    pub fn spare(mut self) -> Self {
        self.priority = Priority::Spare;
        self
    }

    /// It goes and stays with the item before it.
    pub fn joined(mut self) -> Self {
        self.joined = true;
        self
    }

    /// Its label as a row under `…`.
    pub fn menu_label(mut self, label: impl Into<String>) -> Self {
        self.menu_label = Some(label.into());
        self
    }

    /// Its tooltip says `tooltip`.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// The text is a path (see [`Self::path`]).
    pub fn as_path(mut self) -> Self {
        self.path = true;
        self
    }

    /// The item is clickable: `action` is the app command.
    pub fn clickable(mut self, action: &'static str) -> Self {
        self.action = Some(action);
        self
    }

    /// Reserves the width of the `template` template (see `reserve`).
    pub fn reserved(mut self, template: &'static str) -> Self {
        self.reserve = Some(template);
        self
    }

    /// Starts a group (a separator before it).
    pub fn grouped(mut self) -> Self {
        self.group = true;
        self
    }

    pub fn colored(mut self, color: Rgba) -> Self {
        if let Some((_, c)) = &mut self.icon {
            *c = Some(color);
        }
        self
    }
}

#[derive(Debug, Default)]
pub struct StatusBar {
    bounds: Rect,
    scale: f32,
    /// The areas of the clickable items (from the last paint).
    actions: Vec<(Rect, &'static str)>,
    hover: Option<&'static str>,
    /// A text cut short (from the last paint): where, and all of it (its tooltip).
    cut: Option<(Rect, String)>,
    /// The items with a tooltip of their own (from the last paint).
    tips: Vec<(Rect, String)>,
    /// The items on the right that didn't fit (from the last paint), in their order.
    hidden: Vec<StatusItem>,
    /// The meter's bars gliding: from these heights to those, since then.
    meter_from: Vec<f32>,
    meter_to: Vec<f32>,
    meter_since: Option<std::time::Instant>,
    /// Its window isn't the one in front: the bars jump to their heights (a glide is a dozen
    /// whole frames, at every sample, in every window).
    still: bool,
}

/// How long a meter's bar takes to reach its new height.
const METER_GLIDE: std::time::Duration = std::time::Duration::from_millis(220);

/// A meter's bars at `t` (0–1) of their glide from `from` to `to`, eased.
fn glide(from: &[f32], to: &[f32], t: f32) -> Vec<f32> {
    let k = t.clamp(0.0, 1.0);
    let k = k * k * (3.0 - 2.0 * k);
    to.iter()
        .enumerate()
        .map(|(i, b)| {
            let a = from.get(i).copied().unwrap_or(*b);
            a + (b - a) * k
        })
        .collect()
}

/// Which of `items` (their widths, gaps included) are shown in `room`: all if they fit;
/// else the spare ones go first, then the normal ones, the farthest from the corner (the
/// first) first, a joined item with the one before it, until they fit with `more` (the
/// `…`'s width) beside them. The essential ones always stay.
pub fn fit(items: &[StatusItem], widths: &[f32], room: f32, more: f32) -> Vec<bool> {
    let mut shown = vec![true; items.len()];
    let total = |shown: &[bool]| -> f32 {
        widths.iter().zip(shown).filter(|(_, s)| **s).map(|(w, _)| *w).sum()
    };
    if total(&shown) <= room {
        return shown;
    }
    // The clusters: an item and those joined after it.
    let mut clusters: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match clusters.last_mut() {
            Some(last) if item.joined => last.end = i + 1,
            _ => clusters.push(i..i + 1),
        }
    }
    let priority = |c: &std::ops::Range<usize>| {
        items[c.clone()].iter().map(|i| i.priority).max().unwrap_or_default()
    };
    for level in [Priority::Spare, Priority::Normal] {
        for cluster in clusters.iter().filter(|c| priority(c) == level) {
            if total(&shown) + more <= room {
                return shown;
            }
            for k in cluster.clone() {
                shown[k] = false;
            }
        }
    }
    shown
}

impl StatusBar {
    /// The text cut short under `p`: all of it (a tooltip).
    pub fn tooltip_at(&self, p: Point) -> Option<(Rect, String)> {
        self.cut.iter().chain(&self.tips).find(|(r, _)| r.contains(p)).cloned()
    }

    pub fn height(scale: f32) -> f32 {
        (HEIGHT * scale).round()
    }

    /// The clickable item under `p` and its area (for the tooltip).
    pub fn hit_rect(&self, p: Point) -> Option<(Rect, &'static str)> {
        self.actions.iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// The area of the item with action `action` (e.g. a popover's anchor).
    pub fn action_rect(&self, action: &str) -> Option<Rect> {
        self.actions.iter().find(|(_, a)| *a == action).map(|(r, _)| *r)
    }

    /// The command of the clickable item under `p`.
    pub fn hit(&self, p: Point) -> Option<&'static str> {
        self.actions.iter().find(|(r, _)| r.contains(p)).map(|(_, a)| *a)
    }

    /// The items on the right that didn't fit at the last paint (the `…`'s list).
    pub fn hidden(&self) -> &[StatusItem] {
        &self.hidden
    }

    /// When the next frame is due for the meter's bars (only while they glide).
    pub fn next_frame(&self) -> Option<std::time::Instant> {
        let since = self.meter_since?;
        let now = std::time::Instant::now();
        (now < since + METER_GLIDE).then(|| now + std::time::Duration::from_millis(16))
    }

    /// The meter's bars as drawn at `now`, their glide started anew when `target` changed.
    fn meter_at(&mut self, target: &[f32], now: std::time::Instant) -> Vec<f32> {
        let shown = |bar: &Self| match bar.meter_since {
            Some(since) => glide(
                &bar.meter_from,
                &bar.meter_to,
                now.saturating_duration_since(since).as_secs_f32() / METER_GLIDE.as_secs_f32(),
            ),
            None => bar.meter_to.clone(),
        };
        if self.meter_to != target && self.still {
            self.meter_to = target.to_vec();
            self.meter_since = None;
        } else if self.meter_to != target {
            let from = if self.meter_to.is_empty() { target.to_vec() } else { shown(self) };
            self.meter_from = from;
            self.meter_to = target.to_vec();
            self.meter_since = Some(now);
        } else if self.meter_since.is_some_and(|s| now >= s + METER_GLIDE) {
            self.meter_since = None;
        }
        shown(self)
    }

    /// Its window in front or not: the meter's bars glide only in the window in front.
    pub fn set_still(&mut self, still: bool) {
        self.still = still;
        if still {
            self.meter_since = None;
        }
    }

    /// `true` if the item under the mouse changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    /// `left` is aligned left, `right` right (in the given order).
    pub fn paint(
        &mut self,
        left: &[StatusItem],
        right_items: &[StatusItem],
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let (bounds, s) = (self.bounds, self.scale);
        let px = |v: f32| (v * s).round();
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 0.5));
        let metrics = text.metrics(&font);
        let baseline = centered_baseline(bounds.min_y(), bounds.size.height, &metrics);
        let padding = px(14.0);
        let gap = px(18.0);
        let icon_size = px(14.0);
        let icon_gap = px(6.0);
        let icon_top = (bounds.min_y() + (bounds.size.height - icon_size) / 2.0).round();

        // The meter's bars as they glide now (one meter in the bar).
        let target: Vec<f32> = right_items
            .iter()
            .find(|i| !i.meter.is_empty())
            .map(|i| i.meter.iter().map(|(h, _)| h.clamp(0.0, 1.0)).collect())
            .unwrap_or_default();
        let meter_shown = self.meter_at(&target, std::time::Instant::now());
        // The status bar sits on the window background, below the card, without a border.
        let layer = scene.push_layer(Some(bounds));
        layer.quad(Quad::filled(bounds, palette.window_background));

        let text_width = |item: &StatusItem, text: &mut TextSystem| {
            let w = text.shape_line(&item.text, &font).width;
            item.reserve.map_or(w, |r| w.max(text.shape_line(r, &font).width))
        };
        let (bar_w, bar_gap, bar_h) = (px(5.0), px(3.0), px(13.0));
        let item_width = |item: &StatusItem, text: &mut TextSystem| {
            if !item.meter.is_empty() {
                let n = item.meter.len() as f32;
                return n * bar_w + (n - 1.0) * bar_gap;
            }
            // An icon alone (the padlock): no gap after it.
            let icon = match (item.icon.is_some(), item.text.is_empty()) {
                (true, true) => icon_size,
                (true, false) => icon_size + icon_gap,
                (false, _) => 0.0,
            };
            icon + text_width(item, text)
        };
        self.actions.clear();
        self.tips.clear();
        let actions = &mut self.actions;
        let tips = &mut self.tips;
        let hover = self.hover;
        let pill_h = px(20.0);
        let pill_top = (bounds.min_y() + (bounds.size.height - pill_h) / 2.0).round();
        let mut draw = |x: f32, item: &StatusItem, text: &mut TextSystem| -> f32 {
            let mut x = x;
            if let Some(action) = item.action {
                let w = item_width(item, text);
                let pill = Rect::new(x - px(6.0), pill_top, w + px(12.0), pill_h);
                if hover == Some(action) {
                    layer.quad(Quad::filled(pill, palette.tab_hover).rounded(px(5.0)));
                }
                actions.push((pill, action));
            }
            if let Some(tip) = &item.tooltip {
                let w = item_width(item, text);
                tips.push((Rect::new(x - px(6.0), pill_top, w + px(12.0), pill_h), tip.clone()));
            }
            if !item.meter.is_empty() {
                let bottom = (bounds.min_y() + (bounds.size.height + bar_h) / 2.0).round();
                for (i, (_, color)) in item.meter.iter().enumerate() {
                    let left = x + i as f32 * (bar_w + bar_gap);
                    let track = Rect::new(left, bottom - bar_h, bar_w, bar_h);
                    layer.quad(Quad::filled(track, color.with_alpha(0.16)).rounded(px(1.5)));
                    let h = meter_shown.get(i).copied().unwrap_or(0.0);
                    let h = (h * bar_h).max(px(2.0)).round();
                    let bar = Rect::new(left, bottom - h, bar_w, h);
                    layer.quad(Quad::filled(bar, *color).rounded(px(1.5)));
                }
                return x + item_width(item, text);
            }
            if let Some((icon, color)) = item.icon {
                let rect = Rect::new(x, icon_top, icon_size, icon_size);
                draw_icon(layer, icons, icon, rect, color.unwrap_or(palette.text_muted));
                x += icon_size + icon_gap;
            }
            let shaped = text.shape_line(&item.text, &font);
            let slot = text_width(item, text);
            // In a reserved slot, a value next to its icon starts right after it (digits of
            // different widths don't shift it); a lone value (the zoom) is centered.
            let tx = if item.icon.is_some() { x } else { x + (slot - shaped.width) / 2.0 };
            draw_text(layer, &shaped, tx.round(), baseline, palette.text_muted);
            x + slot
        };

        // The right items that fit, the left keeping its least; the others under `…`.
        let left_width: f32 = left.iter().map(|item| item_width(item, text) + gap).sum();
        let left_keeps = left_width.min(px(LEFT_MIN));
        let room = (bounds.size.width - 2.0 * padding - left_keeps).max(0.0);
        let widths: Vec<f32> =
            right_items.iter().map(|item| item_width(item, text) + gap).collect();
        let more_item = StatusItem::text("…").clickable(MORE);
        let more_w = item_width(&more_item, text) + gap;
        let shown = fit(right_items, &widths, room, more_w);
        self.hidden =
            right_items.iter().zip(&shown).filter(|(_, s)| !**s).map(|(i, _)| i.clone()).collect();
        // A group's separator stays: on the first item shown after its start.
        let mut right: Vec<StatusItem> = Vec::with_capacity(right_items.len() + 1);
        let mut group = false;
        for (item, shown) in right_items.iter().zip(&shown) {
            group |= item.group;
            if *shown {
                right.push(StatusItem { group, ..item.clone() });
                group = false;
            }
        }
        if !self.hidden.is_empty() {
            right.insert(0, more_item);
        }
        let right_width: f32 = right.iter().map(|item| item_width(item, text) + gap).sum();
        // The left items end before the right ones; a text that doesn't fit is shortened
        // from its start (a path keeps its file name).
        let limit = bounds.max_x() - padding - right_width;
        let mut x = bounds.min_x() + padding;
        self.cut = None;
        for item in left {
            let room = limit - x;
            if room <= 0.0 {
                break;
            }
            // A path: cut in the middle to its room (at most its widest), the file name kept.
            if item.path {
                let icon = if item.icon.is_some() { icon_size + icon_gap } else { 0.0 };
                let max = (room.min(px(crate::ds::PATH_MAX)) - icon).max(0.0);
                let mut shown = item.clone();
                shown.text = crate::ds::ellipsize_path(text, &item.text, &font, max);
                let end = draw(x, &shown, text);
                if shown.text != item.text {
                    self.cut = Some((
                        Rect::new(x, bounds.min_y(), end - x, bounds.size.height),
                        item.text.clone(),
                    ));
                }
                x = end + gap;
                continue;
            }
            if item_width(item, text) <= room {
                x = draw(x, item, text) + gap;
                continue;
            }
            // The fewest leading characters to drop (binary search: few shapings per frame).
            let chars: Vec<char> = item.text.chars().collect();
            let mut shortened = item.clone();
            shortened.reserve = None;
            let mut with_skip = |skip: usize, text: &mut TextSystem| {
                shortened.text =
                    std::iter::once('…').chain(chars[skip..].iter().copied()).collect();
                item_width(&shortened, text) <= room
            };
            let (mut lo, mut hi) = (1, chars.len());
            while lo < hi {
                let mid = (lo + hi) / 2;
                if with_skip(mid, text) {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            if lo < chars.len() && with_skip(lo, text) {
                draw(x, &shortened, text);
            }
            break;
        }
        let mut x = bounds.max_x() - padding;
        let line = px(1.0).max(1.0);
        let line_h = px(14.0);
        let line_top = (bounds.min_y() + (bounds.size.height - line_h) / 2.0).round();
        let mut separators = Vec::new();
        for (k, item) in right.iter().enumerate().rev() {
            x -= item_width(item, text);
            draw(x.round(), item, text);
            x -= gap;
            // A group's separator, in the middle of the gap before it (not before the first).
            if item.group && k > 0 {
                separators.push(Rect::new((x + gap / 2.0).round(), line_top, line, line_h));
            }
        }
        for rect in separators {
            layer.quad(Quad::filled(rect, palette.border));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/status_bar.rs"]
mod tests;
