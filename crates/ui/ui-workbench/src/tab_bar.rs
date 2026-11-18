//! The tab bar above the editor: navigation buttons (back / forward) and
//! the tabs, with horizontal scrolling when they don't fit.
//!
//! Tabs are compact and flat, on the editor background: the active one has accented
//! text and an accent line below it (over the bar's line); the one under the cursor
//! strengthens its text and gets a subtle line. A file outside the project is tinted
//! faintly in the theme's warning color, with a line of it along the tab's top and its ↗.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{centered_baseline, draw_icon, draw_text};

use crate::Painter;
use crate::palette::Palette;

/// The bar height, in logical pixels.
const HEIGHT: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabTitle<'a> {
    pub title: &'a str,
    pub dirty: bool,
    /// Pinned: stays first and isn't closed with the group (pin icon).
    pub pinned: bool,
    /// The file icon and its color.
    pub icon: Option<(IconId, Rgba)>,
    /// The view's color (a data source's): a dot on the icon's corner.
    pub accent: Option<Rgba>,
    /// A short mark after the title, in its color (`TX`: a console's transaction open).
    pub badge: Option<(&'a str, Rgba)>,
    /// Locked (read-only): a small padlock after the title.
    pub locked: bool,
    /// A file outside the open project (dropped in, opened from elsewhere): a small ↗ after
    /// the title, the tab tinted, a line along its top.
    pub outside: bool,
}

/// The mark of a file outside the project (↗).
const OUTSIDE_MARK: &str = "arrow-up-right";

impl TabTitle<'_> {
    /// The small muted icons after the title, in order.
    fn marks(&self) -> impl Iterator<Item = &'static str> {
        [(self.locked, "lock"), (self.outside, OUTSIDE_MARK)]
            .into_iter()
            .filter_map(|(on, icon)| on.then_some(icon))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHit {
    Activate(usize),
    Close(usize),
    Back,
    Forward,
    /// The hidden-tab indicators (left / right): scroll the strip.
    MoreLeft,
    MoreRight,
    /// The editor area all the window (the side and bottom windows hidden), or back.
    Maximize,
}

/// The state of the navigation buttons (enabled / disabled).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Navigation {
    pub can_back: bool,
    pub can_forward: bool,
    /// The editor area takes the window (its button says it goes back).
    pub maximized: bool,
}

#[derive(Debug, Default)]
pub struct TabBar {
    bounds: Rect,
    scale: f32,
    hover: Option<usize>,
    /// Horizontal scroll of the tabs, in physical pixels.
    scroll_x: f32,
    /// The total width of the tabs (from the last paint), for limiting the scroll.
    content_width: f32,
    /// The area where the tabs sit (after the navigation buttons).
    strip: Rect,
    /// The area of the pinned tabs (on the left, not scrolled).
    pinned_area: Rect,
    /// (tab area, close button area), in screen coordinates.
    rects: Vec<(Rect, Rect)>,
    back: Rect,
    forward: Rect,
    /// The button at its right end: the editor area all the window, or back.
    maximize: Rect,
    /// The buttons of the hidden-tab indicators (empty when nothing is hidden).
    more_left: Rect,
    more_right: Rect,
    /// The active tab must be scrolled into view on the next paint.
    reveal_active: bool,
    /// The indicator under the cursor: `Some(true)` = left.
    hover_more: Option<bool>,
}

impl TabBar {
    pub fn height(scale: f32) -> f32 {
        (HEIGHT * scale).round()
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    pub fn contains(&self, p: Point) -> bool {
        self.bounds.contains(p)
    }

    /// Requests bringing the active tab into the visible area (e.g. after opening / switching).
    pub fn reveal_active(&mut self) {
        self.reveal_active = true;
    }

    /// Horizontal scroll (wheel or trackpad over the bar). `true` if it moved.
    pub fn scroll_by(&mut self, delta: f32) -> bool {
        let before = self.scroll_x;
        let max = (self.content_width - self.strip.size.width).max(0.0);
        self.scroll_x = (self.scroll_x + delta).clamp(0.0, max).round();
        self.scroll_x != before
    }

    /// Updates the tab under the cursor. Returns `true` if it changed (a redraw is needed).
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self
            .strip
            .contains(p)
            .then(|| self.rects.iter().position(|(r, _)| r.contains(p)))
            .flatten();
        let changed = hover != self.hover;
        self.hover = hover;
        let more = if self.more_left.contains(p) {
            Some(true)
        } else if self.more_right.contains(p) {
            Some(false)
        } else {
            None
        };
        let more_changed = more != self.hover_more;
        self.hover_more = more;
        changed || more_changed
    }

    /// The button under `p` (navigation, close tab) and its area (for the tooltip).
    pub fn hit_rect(&self, p: Point) -> Option<(Rect, TabHit)> {
        let hit = self.hit(p)?;
        let rect = match hit {
            TabHit::Back => self.back,
            TabHit::Forward => self.forward,
            TabHit::Maximize => self.maximize,
            TabHit::Close(i) => self.rects.get(i)?.1,
            TabHit::Activate(_) | TabHit::MoreLeft | TabHit::MoreRight => return None,
        };
        Some((rect, hit))
    }

    pub fn hit(&self, p: Point) -> Option<TabHit> {
        if self.maximize.contains(p) {
            return Some(TabHit::Maximize);
        }
        if self.back.contains(p) {
            return Some(TabHit::Back);
        }
        if self.forward.contains(p) {
            return Some(TabHit::Forward);
        }
        if self.more_left.contains(p) {
            return Some(TabHit::MoreLeft);
        }
        if self.more_right.contains(p) {
            return Some(TabHit::MoreRight);
        }
        if !self.strip.contains(p) && !self.pinned_area.contains(p) {
            return None;
        }
        let index = self.rects.iter().position(|(r, _)| r.contains(p))?;
        let close = self.rects[index].1;
        // The close click area is slightly larger than the icon.
        let pad = 4.0 * self.scale;
        let target = Rect::new(
            close.min_x() - pad,
            close.min_y() - pad,
            close.size.width + 2.0 * pad,
            close.size.height + 2.0 * pad,
        );
        Some(if target.contains(p) { TabHit::Close(index) } else { TabHit::Activate(index) })
    }

    /// Where a tab dropped at `p` (dragged from elsewhere) would go: the insertion index and
    /// the vertical line that shows it. `None` if `p` isn't on the bar.
    pub fn drop_target(&self, p: Point) -> Option<(usize, Rect)> {
        if !self.bounds.contains(p) {
            return None;
        }
        let index = self
            .rects
            .iter()
            .position(|(r, _)| p.x < r.min_x() + r.size.width / 2.0)
            .unwrap_or(self.rects.len());
        let x = match self.rects.get(index) {
            Some((r, _)) => r.min_x(),
            None => self.rects.last().map_or(self.strip.min_x(), |(r, _)| r.max_x()),
        };
        let w = (2.0 * self.scale).round().max(1.0);
        let pad = (8.0 * self.scale).round();
        let marker = Rect::new(
            (x - w / 2.0).round(),
            self.bounds.min_y() + pad,
            w,
            (self.bounds.size.height - 2.0 * pad).max(0.0),
        );
        Some((index, marker))
    }

    /// The area of tab `i` (from the last paint).
    pub fn tab_rect(&self, i: usize) -> Option<Rect> {
        self.rects.get(i).map(|(r, _)| *r)
    }

    /// Scrolls the strip by a "page" (as much as fits, minus a bit), left or right.
    pub fn page(&mut self, right: bool) -> bool {
        let page = (self.strip.size.width * 0.7).max(1.0);
        self.scroll_by(if right { page } else { -page })
    }

    /// The hidden-tab indicators: at the strip's edge, a translucent fade ("glass")
    /// in the bar color and a round, semi-transparent button with the arrow toward the tabs.
    fn paint_overflow(&mut self, icons: &Icons, scene: &mut Scene, palette: &Palette) {
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let line = s.round().max(1.0);
        let strip = self.strip;
        let max = (self.content_width - strip.size.width).max(0.0);
        let left = self.scroll_x > 0.5;
        let right = self.scroll_x < max - 0.5;
        self.more_left = Rect::default();
        self.more_right = Rect::default();
        if !left && !right {
            return;
        }
        let layer = scene.push_layer(Some(strip));
        let fade = px(72.0);
        let height = strip.size.height - line;
        let button = px(28.0);
        let top = (strip.min_y() + (height - button) / 2.0).round();
        for (show, from_left) in [(left, true), (right, false)] {
            if !show {
                continue;
            }
            // The fade ("glass"): nearly opaque at the edge, then smoothly fading toward the
            // inside, in fine strips.
            const STEPS: usize = 24;
            let step = fade / STEPS as f32;
            for i in 0..STEPS {
                let t = (i as f32 + 0.5) / STEPS as f32;
                let alpha = 0.97 * (1.0 - t * t * (3.0 - 2.0 * t));
                let x = if from_left {
                    strip.min_x() + i as f32 * step
                } else {
                    strip.max_x() - (i + 1) as f32 * step
                };
                layer.quad(Quad::filled(
                    Rect::new(x.floor(), strip.min_y(), step.ceil() + 1.0, height),
                    palette.tab_bar_background.with_alpha(alpha),
                ));
            }
            // The arrow, without a background; the click area is an invisible square around it.
            let x =
                if from_left { strip.min_x() + px(4.0) } else { strip.max_x() - px(4.0) - button };
            let rect = Rect::new(x, top, button, button);
            let hovered = self.hover_more == Some(from_left);
            if let Some(id) = icons.id(if from_left { "chevron-left" } else { "chevron-right" }) {
                let size = px(16.0);
                let r = Rect::new(
                    rect.min_x() + (button - size) / 2.0,
                    rect.min_y() + (button - size) / 2.0,
                    size,
                    size,
                );
                let color = if hovered { palette.accent } else { palette.text };
                draw_icon(layer, icons, id, r, color);
            }
            if from_left {
                self.more_left = rect;
            } else {
                self.more_right = rect;
            }
        }
    }

    pub fn paint(
        &mut self,
        tabs: &[TabTitle<'_>],
        active: usize,
        navigation: Navigation,
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, icons } = painter;
        let look = TabLook::new(self.bounds, self.scale, text, palette, icons);
        let (b, line) = (self.bounds, look.line);
        let px = |v: f32| look.px(v);
        let layer = scene.push_layer(Some(b));
        self.paint_bar(layer, &look, navigation);

        // The tab widths, so we know how far it can scroll and where the active tab is.
        let mut widths = Vec::with_capacity(tabs.len());
        for tab in tabs {
            let label = text.shape_line(tab.title, &look.strong);
            let label_w = label.width.min(look.title_max());
            let icon_space = if tab.icon.is_some() { look.icon + look.gap * 0.75 } else { 0.0 };
            let badge = tab.badge.map_or(0.0, |(b, _)| look.badge_width(text, b) + look.gap * 0.75);
            let marks = tab.marks().count() as f32 * look.mark_width();
            widths.push(
                (look.padding
                    + icon_space
                    + label_w
                    + marks
                    + badge
                    + look.gap
                    + look.close_size
                    + look.padding * 0.75)
                    .round(),
            );
        }
        // The pinned tabs (always first) stay put on the left; the rest scroll next to them.
        let pinned_n = tabs.iter().take_while(|t| t.pinned).count();
        let full = self.strip;
        self.split_strip(&widths, pinned_n, active);
        let xs = self.tab_positions(&widths, pinned_n, full.min_x());

        self.rects.clear();
        let mut rects = vec![(Rect::default(), Rect::default()); tabs.len()];
        // First the scrolled ones (clipped to their area), then the pinned ones, on top.
        for pinned_pass in [false, true] {
            let clip = if pinned_pass { self.pinned_area } else { self.strip };
            if pinned_pass && pinned_n > 0 && self.scroll_x > 0.5 {
                // A thin line between the pinned ones and the tabs scrolled underneath them.
                let edge = Rect::new(
                    self.strip.min_x() - line,
                    b.min_y() + px(8.0),
                    line,
                    b.size.height - px(16.0),
                );
                scene.push_layer(None).quad(Quad::filled(edge, palette.card_border));
            }
            let layer = scene.push_layer(Some(clip));
            for (i, tab) in tabs.iter().enumerate() {
                if (i < pinned_n) != pinned_pass {
                    continue;
                }
                let slot = look.slot(xs[i], widths[i], i == active, self.hover == Some(i));
                look.paint_tab(layer, text, tab, slot);
                rects[i] = (slot.rect, slot.close);
            }
        }
        self.rects = rects;
        self.paint_overflow(icons, scene, palette);
    }

    /// The bar background (rounded at the top, following the card's corners) and bottom
    /// line; the navigation buttons, separated from the tabs by a vertical line. Sets the
    /// tabs' strip (after them).
    fn paint_bar(
        &mut self,
        layer: &mut render_api::Layer,
        look: &TabLook<'_>,
        navigation: Navigation,
    ) {
        let (b, line, palette, icons) = (self.bounds, look.line, look.palette, look.icons);
        let px = |v: f32| look.px(v);
        let card_radius = px(9.0);
        layer.quad(
            Quad::filled(
                Rect::new(b.min_x(), b.min_y(), b.size.width, b.size.height + card_radius),
                palette.tab_active_background,
            )
            .rounded(card_radius),
        );
        layer.quad(Quad::filled(
            Rect::new(b.min_x(), b.max_y() - line, b.size.width, line),
            palette.card_border,
        ));
        let button = px(28.0);
        let nav_top = (b.min_y() + (b.size.height - button) / 2.0).round();
        self.back = Rect::new(b.min_x() + px(8.0), nav_top, button, button);
        self.forward = Rect::new(self.back.max_x() + px(2.0), nav_top, button, button);
        for (rect, name, enabled) in [
            (self.back, "arrow-left", navigation.can_back),
            (self.forward, "arrow-right", navigation.can_forward),
        ] {
            if let Some(id) = icons.id(name) {
                let color =
                    if enabled { palette.text_muted } else { palette.text_muted.with_alpha(0.35) };
                let size = px(16.0);
                let r = Rect::new(
                    rect.min_x() + (rect.size.width - size) / 2.0,
                    rect.min_y() + (rect.size.height - size) / 2.0,
                    size,
                    size,
                );
                draw_icon(layer, icons, id, r, color);
            }
        }
        let divider = self.forward.max_x() + px(8.0);
        layer.quad(Quad::filled(
            Rect::new(divider, b.min_y(), line, b.size.height - line),
            palette.card_border,
        ));
        // At the right end, after a line: the editor area all the window, or back (as the
        // bottom window's own button).
        self.maximize = Rect::new(b.max_x() - px(8.0) - button, nav_top, button, button);
        let right_divider = self.maximize.min_x() - px(8.0);
        layer.quad(Quad::filled(
            Rect::new(right_divider, b.min_y(), line, b.size.height - line),
            palette.card_border,
        ));
        let icon = if navigation.maximized { "minimize-2" } else { "maximize-2" };
        if let Some(id) = icons.id(icon) {
            let size = px(15.0);
            let r = Rect::new(
                (self.maximize.min_x() + (button - size) / 2.0).round(),
                (self.maximize.min_y() + (button - size) / 2.0).round(),
                size,
                size,
            );
            draw_icon(layer, icons, id, r, palette.text_muted);
        }
        // The right margin: the tabs don't touch the line before the button.
        let right_margin = px(4.0);
        self.strip = Rect::new(
            divider + line,
            b.min_y(),
            (right_divider - divider - line - right_margin).max(0.0),
            // Also includes the bottom line: the active tab covers it.
            b.size.height,
        );
    }

    /// Splits the strip into the pinned tabs' area and the scrolled one; how far the tabs
    /// can scroll, the active one brought into view when asked.
    fn split_strip(&mut self, widths: &[f32], pinned_n: usize, active: usize) {
        let px = |v: f32| (v * self.scale).round();
        let pinned_w: f32 = widths[..pinned_n].iter().sum();
        let full = self.strip;
        self.pinned_area = Rect::new(full.min_x(), full.min_y(), pinned_w, full.size.height);
        self.strip = Rect::new(
            full.min_x() + pinned_w,
            full.min_y(),
            (full.size.width - pinned_w).max(0.0),
            full.size.height,
        );
        self.content_width = widths[pinned_n..].iter().sum::<f32>() + px(8.0);
        if self.reveal_active && active < pinned_n {
            self.reveal_active = false;
        }
        if self.reveal_active && active < widths.len() {
            let start: f32 = widths[pinned_n..active].iter().sum();
            let end = start + widths[active];
            let view = self.strip.size.width;
            if start < self.scroll_x {
                self.scroll_x = start;
            } else if end > self.scroll_x + view {
                self.scroll_x = end - view + px(8.0);
            }
            self.reveal_active = false;
        }
        self.scroll_x =
            self.scroll_x.clamp(0.0, (self.content_width - self.strip.size.width).max(0.0)).round();
    }

    /// The tabs' left edges: the pinned ones from `start` (the bar's), the rest after them,
    /// scrolled.
    fn tab_positions(&self, widths: &[f32], pinned_n: usize, start: f32) -> Vec<f32> {
        let mut xs = Vec::with_capacity(widths.len());
        let mut x = start;
        for (i, w) in widths.iter().enumerate() {
            if i == pinned_n {
                x = self.strip.min_x() - self.scroll_x;
            }
            xs.push(x);
            x += w;
        }
        xs
    }
}

/// What one paint of the tabs draws with: the theme, the icons, the fonts, the labels'
/// baseline and the sizes (physical pixels).
struct TabLook<'a> {
    palette: &'a Palette,
    icons: &'a Icons,
    bounds: Rect,
    scale: f32,
    line: f32,
    font: Font,
    /// The active tab's label (and the widths, so it doesn't move when activated).
    strong: Font,
    baseline: f32,
    padding: f32,
    icon: f32,
    close_size: f32,
    gap: f32,
}

/// A tab's place and state: its area, its close button's, active, under the cursor.
#[derive(Clone, Copy)]
struct TabSlot {
    rect: Rect,
    close: Rect,
    active: bool,
    hovered: bool,
}

impl<'a> TabLook<'a> {
    fn new(
        bounds: Rect,
        scale: f32,
        text: &mut TextSystem,
        palette: &'a Palette,
        icons: &'a Icons,
    ) -> Self {
        let px = |v: f32| (v * scale).round();
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 0.5));
        let strong = font.clone().with_weight(500);
        let metrics = text.metrics(&font);
        // The tabs occupy the strip below the top margin (the active tab starts from there).
        let baseline = centered_baseline(bounds.min_y(), bounds.size.height, &metrics);
        Self {
            palette,
            icons,
            bounds,
            scale,
            line: scale.round().max(1.0),
            font,
            strong,
            baseline,
            padding: px(12.0),
            icon: px(15.0),
            close_size: px(15.0),
            gap: px(7.0),
        }
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    fn badge_font(&self) -> Font {
        Font::new(self.palette.ui_font.clone(), self.px(self.palette.ui_font_size - 3.0))
            .with_weight(Font::BOLD)
    }

    /// The widest a tab's title gets (physical pixels): longer, it's cut in the middle.
    fn title_max(&self) -> f32 {
        self.px(crate::ds::NAME_MAX)
    }

    /// A mark's size (the padlock, ↗), and its room after the title (with the gap before).
    fn mark_size(&self) -> f32 {
        self.px(11.0)
    }

    fn mark_width(&self) -> f32 {
        self.mark_size() + (self.gap * 0.5).round()
    }

    /// A badge's width: its letters and a little room on each side.
    fn badge_width(&self, text: &mut TextSystem, badge: &str) -> f32 {
        (text.shape_line(badge, &self.badge_font()).width + self.px(10.0)).round()
    }

    /// The slot of a tab starting at `x`, `width` wide.
    fn slot(&self, x: f32, width: f32, active: bool, hovered: bool) -> TabSlot {
        let (b, close_size) = (self.bounds, self.close_size);
        let (tab_top, tab_h) = (b.min_y(), b.size.height);
        let rect = Rect::new(x, b.min_y(), width, b.size.height - self.line);
        let close = Rect::new(
            rect.max_x() - self.padding * 0.75 - close_size,
            tab_top + (tab_h - close_size) / 2.0,
            close_size,
            close_size,
        );
        TabSlot { rect, close, active, hovered }
    }

    /// A file outside the project: the tab tinted faintly in the theme's warning color, a
    /// line of it along its top (stronger when active or under the cursor) — apart from the
    /// active tab's line below.
    fn paint_outside(&self, layer: &mut render_api::Layer, rect: Rect, lit: bool) {
        let (color, b) = (self.palette.warning, self.bounds);
        let tab = Rect::new(rect.min_x(), b.min_y(), rect.size.width, b.size.height);
        layer.quad(Quad::filled(tab, color.with_alpha(if lit { 0.08 } else { 0.05 })));
        let line = self.px(2.0);
        let top =
            Rect::new(rect.min_x() + self.px(4.0), b.min_y(), rect.size.width - self.px(8.0), line);
        let alpha = if lit { 0.95 } else { 0.55 };
        layer.quad(Quad::filled(top, color.with_alpha(color.a * alpha)).rounded(self.px(1.0)));
    }

    /// A tab: its underline, file icon, title and right slot.
    fn paint_tab(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        tab: &TabTitle<'_>,
        slot: TabSlot,
    ) {
        let (palette, b, rect) = (self.palette, self.bounds, slot.rect);
        let (is_active, hovered) = (slot.active, slot.hovered);
        let (tab_top, tab_h, icon) = (b.min_y(), b.size.height, self.icon);
        // The underline: accent on the active tab, subtle under the cursor.
        let underline = self.px(2.0);
        let under = Rect::new(
            rect.min_x() + self.px(4.0),
            b.max_y() - underline,
            rect.size.width - self.px(8.0),
            underline,
        );
        if tab.outside {
            self.paint_outside(layer, rect, is_active || hovered);
        }
        if is_active {
            layer.quad(Quad::filled(under, palette.tab_active_indicator).rounded(self.px(1.0)));
        } else if hovered {
            layer.quad(Quad::filled(under, palette.text.with_alpha(0.14)).rounded(self.px(1.0)));
        }
        let mut label_x = rect.min_x() + self.padding;
        if let Some((id, color)) = tab.icon {
            let r = Rect::new(label_x, (tab_top + (tab_h - icon) / 2.0).round(), icon, icon);
            // Inactive tab icons, slightly dimmed (the active one stands out).
            let color = if is_active || hovered { color } else { color.with_alpha(color.a * 0.75) };
            draw_icon(layer, self.icons, id, r, color);
            if let Some(accent) = tab.accent {
                // Its color: a dot on the icon's lower right, ringed in the bar's color.
                let (dot, ring) = (self.px(7.0), self.px(1.5));
                let at = Rect::new(r.max_x() - dot + ring, r.max_y() - dot + ring, dot, dot);
                let outer = Rect::new(
                    at.min_x() - ring,
                    at.min_y() - ring,
                    dot + 2.0 * ring,
                    dot + 2.0 * ring,
                );
                let back = if is_active {
                    palette.tab_active_background
                } else {
                    palette.tab_bar_background
                };
                layer.quad(Quad::filled(outer, back).rounded(outer.size.width / 2.0));
                layer.quad(Quad::filled(at, accent).rounded(dot / 2.0));
            }
            label_x += icon + self.gap * 0.75;
        }
        // A long name cut in the middle (its extension kept): the whole of it, with its path,
        // in the tab's tooltip.
        let font = if is_active { &self.strong } else { &self.font };
        let title = crate::ds::ellipsize_file(text, tab.title, font, self.title_max());
        let label = text.shape_line(&title, font);
        let color = if is_active || hovered { palette.text } else { palette.text_muted };
        draw_text(layer, &label, label_x.round(), self.baseline, color);
        let mut after = label_x + label.width;
        // Locked, outside the project: small icons right after the title (muted; the ↗ in
        // the color of what's outside).
        for (name, id) in tab.marks().filter_map(|name| Some((name, self.icons.id(name)?))) {
            let size = self.mark_size();
            let x = (after + (self.gap * 0.5).round()).round();
            let r = Rect::new(x, (tab_top + (tab_h - size) / 2.0).round(), size, size);
            let color = if name == OUTSIDE_MARK { palette.warning } else { palette.text_muted };
            draw_icon(layer, self.icons, id, r, color);
            after += self.mark_width();
        }
        if let Some((badge, tint)) = tab.badge {
            // Its mark: small bold letters on a tinted pill after the title.
            let x = (after + self.gap * 0.75).round();
            let w = self.badge_width(text, badge);
            let h = self.px(15.0);
            let r = Rect::new(x, (b.min_y() + (b.size.height - h) / 2.0).round(), w, h);
            layer.quad(Quad::filled(r, tint.with_alpha(0.16)).rounded(self.px(4.0)));
            let shaped = text.shape_line(badge, &self.badge_font());
            let base = ui_kit::centered_baseline(r.min_y(), h, &text.metrics(&self.badge_font()));
            draw_text(layer, &shaped, (r.min_x() + self.px(5.0)).round(), base, tint);
        }
        self.paint_right_slot(layer, tab, slot);
    }

    /// The right slot: the pin (pinned tab); • for unsaved changes (the active tab too),
    /// turning into × under the mouse; × on the active / hovered tab.
    fn paint_right_slot(&self, layer: &mut render_api::Layer, tab: &TabTitle<'_>, slot: TabSlot) {
        let (palette, close) = (self.palette, slot.close);
        let (is_active, hovered) = (slot.active, slot.hovered);
        let centered = |size: f32| {
            Rect::new(
                close.min_x() + (close.size.width - size) / 2.0,
                close.min_y() + (close.size.height - size) / 2.0,
                size,
                size,
            )
        };
        if tab.pinned && !hovered {
            if let Some(id) = self.icons.id("pin") {
                draw_icon(layer, self.icons, id, centered(self.px(13.0)), palette.accent);
            }
        } else if (is_active && !tab.dirty) || hovered {
            if let Some(id) = self.icons.id("x") {
                draw_icon(layer, self.icons, id, centered(self.px(13.0)), palette.text_muted);
            }
        } else if tab.dirty {
            let dot = self.px(7.0);
            layer.quad(Quad::filled(centered(dot), palette.accent).rounded(dot / 2.0));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tab_bar.rs"]
mod tests;
