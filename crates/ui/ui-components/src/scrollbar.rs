//! The scrollbar: a subtle thread, always visible, with a rounded thumb that
//! thickens on hover and while dragging. Vertical or horizontal. Along an area's edge
//! ([`EdgeScrollbar`]) it may also hide at rest: it fades in when the content scrolls or the
//! pointer comes to its lane, stays a moment after the last scroll, then fades out.
//!
//! The component doesn't hold the scroll position: on each call it receives the length
//! of the content, of the visible area and the offset, and returns the new offset.
//! The appearance comes from [`ScrollbarStyle`] (from the theme or given individually),
//! shaped by the user's [`ScrollbarLook`] (Settings → Appearance → "Scrollbars"): how a bar
//! answers the pointer, how thick it is, when it shows.

use std::sync::RwLock;
use std::time::{Duration, Instant};

use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};

use crate::palette::Palette;
use crate::splitter::Orientation;

/// How a bar answers the pointer over it (or a drag).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarHover {
    /// It widens into a rounded rail, the thumb wide inside it.
    Rail,
    /// Only the thumb thickens; the rest stays a thin line.
    #[default]
    Thumb,
}

impl ScrollbarHover {
    /// Every one, by the id the settings save.
    pub const ALL: [(Self, &'static str); 2] = [(Self::Rail, "rail"), (Self::Thumb, "thumb")];

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, i)| *i == id).map(|(h, _)| *h)
    }
}

/// When a bar along an area's edge shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarVisibility {
    /// As each view chose (some always, some only while scrolling).
    #[default]
    ByView,
    Always,
    /// Hidden at rest: it comes while the content scrolls or the pointer is on it.
    WhileScrolling,
}

impl ScrollbarVisibility {
    /// Every one, by the id the settings save.
    pub const ALL: [(Self, &'static str); 3] =
        [(Self::ByView, "view"), (Self::Always, "always"), (Self::WhileScrolling, "scrolling")];

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, i)| *i == id).map(|(v, _)| *v)
    }
}

/// The user's look for every scrollbar: how it answers the pointer, its thumb's thickness at
/// rest (logical pixels; on hover it grows by [`GROWTH`]), whether the thin line runs under the
/// thumb, when an edge's bar shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarLook {
    pub hover: ScrollbarHover,
    pub thickness: f32,
    pub line: bool,
    pub visibility: ScrollbarVisibility,
}

/// How much a thumb thickens under the pointer (logical pixels).
pub const GROWTH: f32 = 3.0;

impl ScrollbarLook {
    /// The look before the user chose one: only the thumb growing, 5 px, the line under it,
    /// as each view decides.
    pub const DEFAULT: Self = Self {
        hover: ScrollbarHover::Thumb,
        thickness: 5.0,
        line: true,
        visibility: ScrollbarVisibility::ByView,
    };
}

impl Default for ScrollbarLook {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The look every bar paints with (set from the settings, read at each paint).
static LOOK: RwLock<ScrollbarLook> = RwLock::new(ScrollbarLook::DEFAULT);

/// The scrollbars' look from now on (the settings applied).
pub fn set_look(look: ScrollbarLook) {
    if let Ok(mut current) = LOOK.write() {
        *current = look;
    }
}

/// The scrollbars' look.
pub fn look() -> ScrollbarLook {
    LOOK.read().map_or(ScrollbarLook::DEFAULT, |l| *l)
}

/// The bar's appearance. Dimensions are in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollbarStyle {
    pub track: Rgba,
    pub thumb: Rgba,
    pub thumb_active: Rgba,
    /// The wide bar's rail (its background and outline).
    pub rail: Rgba,
    pub rail_border: Rgba,
    /// The thumb thickness at rest and on hover / drag.
    pub thickness: f32,
    pub thickness_active: f32,
    /// The thumb's minimum length (very long documents).
    pub min_thumb: f32,
    /// How it answers the pointer: widening into its rail, or only its thumb thickening.
    pub hover: ScrollbarHover,
    /// The thin line along the track, under the thumb (off: the thumb alone).
    pub line: bool,
}

impl ScrollbarStyle {
    /// The theme's colors, in the user's look ([`look`]).
    pub fn from_palette(p: &Palette) -> Self {
        let look = look();
        Self {
            track: p.scrollbar_track,
            thumb: p.scrollbar_thumb,
            thumb_active: p.scrollbar_thumb_active,
            rail: p.text.with_alpha(0.045),
            rail_border: p.text.with_alpha(0.06),
            thickness: look.thickness,
            thickness_active: look.thickness + GROWTH,
            min_thumb: 28.0,
            hover: look.hover,
            line: look.line,
        }
    }
}

impl Default for ScrollbarStyle {
    fn default() -> Self {
        Self::from_palette(&Palette::dark())
    }
}

/// The length of the content, of the visible area, and the current position, on the bar's axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollMetrics {
    pub content: f32,
    pub viewport: f32,
    pub offset: f32,
}

impl ScrollMetrics {
    fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    fn scrollable(&self) -> bool {
        self.content > self.viewport + 0.5
    }
}

#[derive(Debug, Default)]
pub struct Scrollbar {
    style: ScrollbarStyle,
    orientation: Orientation,
    /// The grab area (the track), physical pixels.
    track: Rect,
    scale: f32,
    hovered: bool,
    /// Distance from the thumb's start to the cursor, while dragging.
    drag: Option<f32>,
    /// How widened the bar is (0 = subtle thread, 1 = wide, with rail): the track, the rail and
    /// the thumb align with each other.
    expansion: f32,
    /// How visible it is (1: as always; less while it fades).
    opacity: Option<f32>,
}

impl Scrollbar {
    /// `Vertical` = vertical bar (scrolls on y), `Horizontal` = on x.
    pub fn new(style: ScrollbarStyle, orientation: Orientation) -> Self {
        Self { style, orientation, scale: 1.0, ..Self::default() }
    }

    pub fn set_style(&mut self, style: ScrollbarStyle) {
        self.style = style;
    }

    /// The thickness of the grab area (physical pixels), for layout.
    pub fn track_thickness(scale: f32) -> f32 {
        (12.0 * scale).round()
    }

    pub fn set_track(&mut self, track: Rect, scale: f32) {
        self.track = track;
        self.scale = scale;
    }

    pub fn contains(&self, p: Point) -> bool {
        self.track.size.width > 0.0 && self.track.size.height > 0.0 && self.track.contains(p)
    }

    /// The bar's area (the track), in physical pixels.
    pub fn track_rect(&self) -> Rect {
        self.track
    }

    /// The bar's widening (0..1), from whoever animates it.
    pub fn set_expansion(&mut self, expansion: f32) {
        self.expansion = expansion.clamp(0.0, 1.0);
    }

    /// How widened it's drawn: none when only its thumb answers the pointer
    /// ([`ScrollbarHover::Thumb`]).
    fn widening(&self) -> f32 {
        if self.style.hover == ScrollbarHover::Rail { self.expansion } else { 0.0 }
    }

    /// How visible it is (0..1: fading in or out), from whoever animates it.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = Some(opacity.clamp(0.0, 1.0));
    }

    /// The ends of the usable area on the bar's axis (where the thumb and the marks sit), in
    /// physical pixels: the track minus the margins, larger on the wide bar (inside the rail).
    pub fn inner_span(&self) -> (f32, f32) {
        let (start, length) = self.span();
        let px = |v: f32| (v * self.scale).round();
        let pad = px(2.0) + (px(7.0) - px(2.0)) * self.widening();
        (start + pad, start + length - pad)
    }

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn axis(&self, p: Point) -> f32 {
        match self.orientation {
            Orientation::Vertical => p.y,
            Orientation::Horizontal => p.x,
        }
    }

    /// (start, length) of the track on the bar's axis.
    fn span(&self) -> (f32, f32) {
        match self.orientation {
            Orientation::Vertical => (self.track.min_y(), self.track.size.height),
            Orientation::Horizontal => (self.track.min_x(), self.track.size.width),
        }
    }

    /// (start, length) of the thumb on the bar's axis.
    fn thumb_span(&self, m: ScrollMetrics) -> (f32, f32) {
        let (start, length) = self.span();
        let min = (self.style.min_thumb * self.scale).min(length);
        let thumb = (length * m.viewport / m.content.max(1.0)).clamp(min, length);
        let ratio = if m.max_offset() > 0.0 { m.offset / m.max_offset() } else { 0.0 };
        (start + (length - thumb) * ratio.clamp(0.0, 1.0), thumb)
    }

    /// The thumb as a block across the whole track (a lane drawn by its owner, as the
    /// comparison's overview); `None` with nothing to scroll.
    pub fn thumb_rect(&self, m: ScrollMetrics) -> Option<Rect> {
        if !m.scrollable() {
            return None;
        }
        let (start, length) = self.thumb_span(m);
        let t = self.track;
        Some(match self.orientation {
            Orientation::Vertical => Rect::new(t.min_x(), start, t.size.width, length),
            Orientation::Horizontal => Rect::new(start, t.min_y(), length, t.size.height),
        })
    }

    /// The offset for the thumb starting at `thumb_start`.
    fn offset_for(&self, thumb_start: f32, m: ScrollMetrics) -> f32 {
        let (start, length) = self.span();
        let (_, thumb) = self.thumb_span(m);
        let room = (length - thumb).max(1.0);
        ((thumb_start - start) / room).clamp(0.0, 1.0) * m.max_offset()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hovered = self.contains(p);
        let changed = hovered != self.hovered;
        self.hovered = hovered;
        changed
    }

    /// Click on the track: on the thumb, dragging starts; outside it, the thumb jumps under
    /// the cursor (and can keep being dragged). Returns the new offset.
    pub fn begin_drag(&mut self, p: Point, m: ScrollMetrics) -> Option<f32> {
        if !self.contains(p) || !m.scrollable() {
            return None;
        }
        let (thumb_start, thumb) = self.thumb_span(m);
        let at = self.axis(p);
        if at >= thumb_start && at <= thumb_start + thumb {
            self.drag = Some(at - thumb_start);
            return Some(m.offset);
        }
        self.drag = Some(thumb / 2.0);
        Some(self.offset_for(at - thumb / 2.0, m))
    }

    pub fn drag_to(&self, p: Point, m: ScrollMetrics) -> Option<f32> {
        let grab = self.drag?;
        Some(self.offset_for(self.axis(p) - grab, m))
    }

    pub fn end_drag(&mut self) -> bool {
        self.drag.take().is_some()
    }

    pub fn paint(&self, scene: &mut Scene, m: ScrollMetrics) {
        let t = self.track;
        if t.size.width <= 0.0 || t.size.height <= 0.0 {
            return;
        }
        let px = |v: f32| (v * self.scale).round();
        let opacity = self.opacity.unwrap_or(1.0);
        if opacity <= 0.0 {
            return;
        }
        let faded;
        let st = if opacity < 1.0 {
            let fade = |c: Rgba| c.with_alpha(c.a * opacity);
            faded = ScrollbarStyle {
                track: fade(self.style.track),
                thumb: fade(self.style.thumb),
                thumb_active: fade(self.style.thumb_active),
                rail: fade(self.style.rail),
                rail_border: fade(self.style.rail_border),
                ..self.style.clone()
            };
            &faded
        } else {
            &self.style
        };
        let w = self.widening();
        let active = self.hovered || self.drag.is_some();
        let vertical = self.orientation == Orientation::Vertical;
        let layer = scene.push_layer(Some(t));
        let (start, length) = self.span();
        let cross = if vertical { t.size.width } else { t.size.height };
        let cross_min = if vertical { t.min_x() } else { t.min_y() };
        // On the cross axis: at rest next to the edge, on the wide bar in the center.
        let resting = cross - px(2.0) - px(st.thickness_active) / 2.0;
        let center = cross_min + resting + (cross / 2.0 - resting) * w;
        let rect = |from: f32, len: f32, thickness: f32| {
            let c = (center - thickness / 2.0).round();
            if vertical {
                Rect::new(c, from, thickness, len)
            } else {
                Rect::new(from, c, len, thickness)
            }
        };
        // With no content to scroll, nothing (neither the thread nor the rail).
        if !m.scrollable() {
            return;
        }
        // The subtle thread (fades out as the bar widens; none when the look leaves it out) and
        // the rounded rail (fades in).
        let line = px(1.0).max(1.0);
        if w < 0.99 && st.line {
            let inset = px(2.0);
            let hairline = rect(start + inset, length - 2.0 * inset, line);
            layer.quad(Quad::filled(hairline, st.track.with_alpha(st.track.a * (1.0 - w))));
        }
        let rail_w = cross - px(6.0);
        if w > 0.01 {
            let rail = rect(start + px(4.0), length - px(8.0), rail_w * w);
            layer.quad(
                Quad::filled(rail, st.rail.with_alpha(st.rail.a * w))
                    .rounded(rail_w * w / 2.0)
                    .bordered(line, st.rail_border.with_alpha(st.rail_border.a * w)),
            );
        }
        // The thumb: in the usable area (inside the rail on the wide bar), centered on it.
        let (a0, a1) = self.inner_span();
        let (thumb_start, thumb) = self.thumb_span(m);
        let k = (a1 - a0).max(1.0) / length.max(1.0);
        let from = a0 + (thumb_start - start) * k;
        let len = (thumb * k).max(px(st.min_thumb).min(a1 - a0));
        let narrow = px(if active { st.thickness_active } else { st.thickness });
        let wide = (rail_w - px(6.0)).max(narrow);
        let thick = (narrow + (wide - narrow) * w).round();
        let color = if active { st.thumb_active } else { st.thumb };
        layer
            .quad(Quad::filled(rect(from.round(), len.round(), thick), color).rounded(thick / 2.0));
    }
}

/// How an auto-hiding bar comes and goes: shown by a scroll, gone soon after the last.
const FADE_IN: Duration = Duration::from_millis(120);
const HOLD: Duration = Duration::from_millis(600);
const FADE_OUT: Duration = Duration::from_millis(200);
/// A fading bar's frames: about 60 a second.
const FRAME: Duration = Duration::from_millis(16);

/// When a scrollbar hidden at rest shows (one rule for every one: a list's, a page's, the
/// code editor's): a scroll brings it (fading in), it stays a moment after the last one,
/// then fades out; shown, the pointer on it (or a drag) keeps it; hidden, the pointer
/// doesn't bring it. As the view chose (`by_view`), unless the user's look says otherwise.
/// Its owner paints it at [`Self::opacity_at`] and asks for frames at
/// [`Self::next_frame_at`] (none at rest).
#[derive(Debug, Clone, Copy, Default)]
pub struct AutoHide {
    by_view: bool,
    /// When it was last scrolled, hovered or let go; since when it shows.
    active_at: Option<Instant>,
    shown_since: Option<Instant>,
    /// The offset it was last painted with (a change: it was scrolled).
    seen: Option<f32>,
    /// The pointer came to it while it showed: kept while it stays.
    kept: bool,
}

impl AutoHide {
    /// Hidden at rest when `by_view` (the user's look may say always / never).
    pub fn new(by_view: bool) -> Self {
        Self { by_view, ..Self::default() }
    }

    pub fn set_by_view(&mut self, on: bool) {
        self.by_view = on;
    }

    /// Hidden at rest: as the view chose, unless the user's look says otherwise.
    pub fn hides(&self) -> bool {
        match look().visibility {
            ScrollbarVisibility::ByView => self.by_view,
            ScrollbarVisibility::Always => false,
            ScrollbarVisibility::WhileScrolling => true,
        }
    }

    /// Painted at `offset` at `now`: a scroll (the offset moved) makes it show.
    pub fn observe(&mut self, offset: f32, now: Instant) {
        if self.seen.is_some_and(|o| (o - offset).abs() > 0.5) {
            self.touch(now);
        }
        self.seen = Some(offset);
    }

    /// Something happened to it at `now` (scrolled, let go): it shows (again).
    pub fn touch(&mut self, now: Instant) {
        if !self.hides() {
            return;
        }
        if self.opacity_at(now, false) <= 0.0 {
            self.shown_since = Some(now);
        }
        self.active_at = Some(now);
    }

    /// The pointer on it (`hovered`) or not, at `now`; `visible`: it showed before (held
    /// counted); `scrollable`: there's something to scroll. Come to it while it shows: kept;
    /// left: it stays a moment, then goes. Returns whether it was seen (a hidden one
    /// changes nothing seen).
    pub fn pointer(
        &mut self,
        hovered: bool,
        visible: bool,
        scrollable: bool,
        now: Instant,
    ) -> bool {
        if hovered {
            self.kept |= visible && scrollable;
        } else if std::mem::take(&mut self.kept) {
            self.touch(now);
        }
        visible || !self.hides()
    }

    /// Kept shown: the pointer on it since it showed (`hovered` now), or dragged.
    pub fn held(&self, hovered: bool, dragging: bool) -> bool {
        (self.kept && hovered) || dragging
    }

    /// Seen at `now`: shown, fading in (even at its first instant) or fading out.
    pub fn showing(&self, now: Instant, held: bool) -> bool {
        let fading_in =
            self.shown_since.is_some_and(|s| now.saturating_duration_since(s) < FADE_IN);
        fading_in || self.opacity_at(now, held) > 0.0
    }

    /// How visible it is at `now` (always 1 unless it hides at rest).
    pub fn opacity_at(&self, now: Instant, held: bool) -> f32 {
        if !self.hides() {
            return 1.0;
        }
        let (Some(active), Some(shown)) = (self.active_at, self.shown_since) else {
            return 0.0;
        };
        let fade_in =
            (now.saturating_duration_since(shown).as_secs_f32() / FADE_IN.as_secs_f32()).min(1.0);
        if held {
            return fade_in;
        }
        let idle = now.saturating_duration_since(active);
        if idle < HOLD {
            return fade_in;
        }
        let out = 1.0 - (idle - HOLD).as_secs_f32() / FADE_OUT.as_secs_f32();
        fade_in.min(out).max(0.0)
    }

    /// When it next needs a frame (fading, or its hold ending); `None`: at rest, shown or
    /// hidden — no frames.
    pub fn next_frame_at(&self, now: Instant, held: bool) -> Option<Instant> {
        if !self.hides() {
            return None;
        }
        let (active, shown) = (self.active_at?, self.shown_since?);
        if now.saturating_duration_since(shown) < FADE_IN {
            return Some(now + FRAME);
        }
        if held {
            return None;
        }
        let idle = now.saturating_duration_since(active);
        if idle < HOLD {
            return Some(active + HOLD);
        }
        (idle < HOLD + FADE_OUT).then_some(now + FRAME)
    }
}

/// The app's scrollbar along an area's right edge, for a view that scrolls (a list, a
/// page): it keeps what it was last painted with, so a press, a drag or the hover need
/// only the point. Widened while hovered or dragged. Always shown (the default), or hidden
/// at rest ([`Self::auto_hiding`]: [`AutoHide`]'s rule).
#[derive(Debug, Default)]
pub struct EdgeScrollbar {
    bar: Option<Scrollbar>,
    metrics: ScrollMetrics,
    timing: AutoHide,
}

impl EdgeScrollbar {
    /// One hidden at rest: it fades in when the content scrolls (the wheel, the keys, a
    /// jump), stays a moment after, then fades out; shown, it stays while hovered or
    /// dragged.
    pub fn auto_hiding() -> Self {
        Self { timing: AutoHide::new(true), ..Self::default() }
    }

    /// Hidden at rest (or always shown), for a bar kept in a type made by `Default`.
    pub fn set_auto_hiding(&mut self, on: bool) {
        self.timing.set_by_view(on);
    }

    /// Paints it along `area`'s right edge for `content` high scrolled by `offset`
    /// (nothing shows when it all fits).
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        palette: &Palette,
        scale: f32,
        area: Rect,
        content: f32,
        offset: f32,
    ) {
        let thickness = Scrollbar::track_thickness(scale);
        let track = Rect::new(area.max_x() - thickness, area.min_y(), thickness, area.size.height);
        self.paint_track(scene, palette, scale, track, content, offset);
    }

    /// Painted on a dividing line: its thread at rest on the line at `x` (from `top`,
    /// `height` high), so the line and the bar are one — no lane of its own beside it.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_on_line(
        &mut self,
        scene: &mut Scene,
        palette: &Palette,
        scale: f32,
        (x, top, height): (f32, f32, f32),
        content: f32,
        offset: f32,
    ) {
        let thickness = Scrollbar::track_thickness(scale);
        let px = |v: f32| (v * scale).round();
        let active = ScrollbarStyle::from_palette(palette).thickness_active;
        // At rest the thread sits this far from the track's left (see `Scrollbar::paint`).
        let resting = thickness - px(2.0) - px(active) / 2.0;
        let track = Rect::new((x - resting).round(), top, thickness, height);
        self.paint_track(scene, palette, scale, track, content, offset);
    }

    fn paint_track(
        &mut self,
        scene: &mut Scene,
        palette: &Palette,
        scale: f32,
        track: Rect,
        content: f32,
        offset: f32,
    ) {
        let now = Instant::now();
        self.observe(content, offset, now);
        let area = track;
        self.metrics = ScrollMetrics { content, viewport: area.size.height, offset };
        let style = ScrollbarStyle::from_palette(palette);
        let opacity = self.opacity_at(now);
        let bar =
            self.bar.get_or_insert_with(|| Scrollbar::new(style.clone(), Orientation::Vertical));
        bar.set_style(style);
        bar.set_track(track, scale);
        bar.set_expansion(if bar.is_hovered() || bar.is_dragging() { 1.0 } else { 0.0 });
        bar.set_opacity(opacity);
        bar.paint(scene, self.metrics);
    }

    /// What it's painted with at `now`: a scroll (the offset moved) makes it show.
    pub fn observe(&mut self, _content: f32, offset: f32, now: Instant) {
        self.timing.observe(offset, now);
    }

    fn held(&self) -> bool {
        let hovered = self.bar.as_ref().is_some_and(Scrollbar::is_hovered);
        self.timing.held(hovered, self.is_dragging())
    }

    /// How visible it is at `now` (always 1 unless it hides at rest).
    pub fn opacity_at(&self, now: Instant) -> f32 {
        self.timing.opacity_at(now, self.held())
    }

    /// When it next needs a frame (fading, or its hold ending); `None`: at rest, shown or
    /// hidden — no frames.
    pub fn next_frame_at(&self, now: Instant) -> Option<Instant> {
        self.timing.next_frame_at(now, self.held())
    }

    /// [`Self::next_frame_at`] now.
    pub fn next_frame(&self) -> Option<Instant> {
        self.next_frame_at(Instant::now())
    }

    /// A press on it: the new offset (the thumb dragged, or jumped there); `None`: not on it
    /// (hidden: not there either).
    pub fn press(&mut self, p: Point) -> Option<f32> {
        let m = self.metrics;
        if m.content <= m.viewport + 0.5 {
            return None;
        }
        if self.timing.hides() && !self.timing.showing(Instant::now(), self.held()) {
            return None;
        }
        self.bar.as_mut().filter(|b| b.contains(p)).and_then(|b| b.begin_drag(p, m))
    }

    /// `p` is on it, where a press takes it (something to scroll: it's there).
    pub fn contains(&self, p: Point) -> bool {
        let m = self.metrics;
        m.content > m.viewport + 0.5 && self.bar.as_ref().is_some_and(|b| b.contains(p))
    }

    /// The offset while its thumb is dragged.
    pub fn drag(&mut self, p: Point) -> Option<f32> {
        self.bar.as_ref().and_then(|b| b.drag_to(p, self.metrics))
    }

    pub fn release(&mut self) -> bool {
        let released = self.bar.as_mut().is_some_and(Scrollbar::end_drag);
        if released {
            self.timing.touch(Instant::now());
        }
        released
    }

    /// `true` if it changed (it widens under the mouse while it shows).
    pub fn hover(&mut self, p: Point) -> bool {
        self.hover_at(p, Instant::now())
    }

    /// [`Self::hover`] at `now`.
    pub fn hover_at(&mut self, p: Point, now: Instant) -> bool {
        let visible = self.timing.showing(now, self.held());
        let changed = self.bar.as_mut().is_some_and(|b| b.hover(p));
        let hovered = self.bar.as_ref().is_some_and(Scrollbar::is_hovered);
        let scrollable = self.metrics.content > self.metrics.viewport + 0.5;
        let seen = self.timing.pointer(hovered, visible, scrollable, now);
        changed && seen
    }

    pub fn is_dragging(&self) -> bool {
        self.bar.as_ref().is_some_and(Scrollbar::is_dragging)
    }
}

#[cfg(test)]
#[path = "../tests/unit/scrollbar.rs"]
mod tests;
