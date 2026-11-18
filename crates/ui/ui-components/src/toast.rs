//! The notifications drawn (toasts): each a "glass" card — a slightly translucent surface,
//! a soft glow of its kind's color from its top-left corner, a thin border, a soft
//! shadow — with its kind's icon in a tinted square, its title (and `×2` when merged), a
//! line or two of detail, at most two actions and a × with the time left as a ring around
//! it; and the stack of them, at the edge the user chose, the newest nearest it, entering
//! and leaving from that edge.
//!
//! What's shown comes from [`workbench_model::Notifications`] (the rules are there); this
//! only lays it out — once per width and content (a [`kernel::Memo`] keyed by the stack's
//! generation, its texts shaped then) — draws it and says what's under the mouse. Nothing is
//! allocated per frame (the areas painted reuse their vector), and nothing at all is done
//! while the stack is empty.

use std::sync::Arc;

use command_model::CommandId;
use kernel::cache::Memo;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, ShapedLine, TextSystem};
use ui_kit::{draw_icon, draw_text};
use workbench_model::{NotificationId, NotificationKind, Notifications, Toast, ToastPosition};

use crate::ds::{self, Ctx, Elevation, radius};
use crate::painter::Painter;
use crate::palette::Palette;
use crate::progress_ring::{ProgressRing, RingSpec};

/// A card's width at most (logical pixels); narrower when the room is.
pub const WIDTH: f32 = 380.0;
/// The space between cards, and between the stack and the edge of its area.
pub const GAP: f32 = 10.0;
/// From the side of its area (a stack on the right).
pub const MARGIN: f32 = 16.0;
/// How far a card slides in from its edge.
const SLIDE: f32 = 14.0;

const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 12.0;
const ICON_BOX: f32 = 30.0;
const ICON: f32 = 16.0;
const TEXT_GAP: f32 = 12.0;
const CLOSE: f32 = 24.0;
const TITLE_LINE: f32 = 19.0;
const DETAIL_LINE: f32 = 17.0;
const DETAIL_LINES: usize = 2;
const TITLE_LINES: usize = 2;
const ACTION_HEIGHT: f32 = 26.0;
const ACTION_TOP: f32 = 10.0;
const ACTION_PAD: f32 = 12.0;
/// The ring's steps for a whole turn (it's drawn again at each one).
pub const RING_STEPS: u16 = crate::progress_ring::STEPS;
/// A spinner's step (its next dot lit).
pub const SPINNER_STEP: std::time::Duration = std::time::Duration::from_millis(90);
/// Frames for the stack never come faster than this (about 30 a second): its rings.
pub const MIN_FRAME: std::time::Duration = std::time::Duration::from_millis(33);
/// A progress bar sweeping (work in progress): about 60 a second, smooth to the eye — only
/// while it shows.
pub const PROGRESS_FRAME: std::time::Duration = std::time::Duration::from_millis(16);

/// A kind's color, as the palette has it (green from the named colors, in the shade that
/// reads on the theme).
pub fn kind_color(kind: NotificationKind, p: &Palette) -> Rgba {
    match kind {
        NotificationKind::Success => crate::tag_colors::named("green", p).unwrap_or(p.vcs_added),
        NotificationKind::Info | NotificationKind::Progress => p.accent,
        NotificationKind::Warning => p.warning,
        NotificationKind::Error => p.danger,
    }
}

/// A kind's icon (work in progress has a spinner instead).
pub fn kind_icon(kind: NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Success => "check",
        NotificationKind::Info | NotificationKind::Progress => "info",
        NotificationKind::Warning => "alert",
        NotificationKind::Error => "circle-alert",
    }
}

/// How many of the ring's steps show the time left (`None`: no time, no ring).
pub fn ring_steps(remaining: Option<f32>) -> Option<u16> {
    remaining.map(crate::progress_ring::quantize)
}

/// A card laid out (physical pixels, from its top-left corner): its texts shaped.
#[derive(Debug, Clone)]
pub struct CardLayout {
    pub id: NotificationId,
    pub kind: NotificationKind,
    pub width: f32,
    pub height: f32,
    /// The title, in at most two lines (the last cut with "…" to its room), and the count
    /// after it (`×2`).
    pub title: Vec<Arc<ShapedLine>>,
    pub count: Option<Arc<ShapedLine>>,
    /// At most two lines, the last cut with "…".
    pub detail: Vec<Arc<ShapedLine>>,
    /// Each action's label, its button's offset from the text's left edge and width, and
    /// its command.
    pub actions: Vec<(Arc<ShapedLine>, f32, f32, CommandId)>,
    /// Where the text starts, and how wide it may be.
    pub text_x: f32,
    pub text_width: f32,
    /// Its progress bar (work in progress, or a notification telling how far): how far, or
    /// `None` inside: not known (it sweeps).
    pub progress: Option<Option<f32>>,
}

/// What the stack is laid out for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LayoutKey {
    generation: u64,
    width: f32,
    scale: f32,
    font: f32,
}

/// The fonts a card uses.
fn fonts(ctx: &Ctx<'_>) -> (Font, Font, Font, Font) {
    let title = ctx.ui_font(0.0).with_weight(Font::SEMIBOLD);
    let detail = ctx.ui_font(-1.0);
    let count = ctx.ui_font(-1.5).with_weight(Font::SEMIBOLD);
    (title, detail, count, ds::button_font(ctx))
}

/// Lays out `toast` on a card `width` wide (physical pixels).
pub fn layout_card(text: &mut TextSystem, ctx: &Ctx<'_>, toast: &Toast, width: f32) -> CardLayout {
    let px = |v: f32| ctx.px(v);
    let (title_font, detail_font, count_font, action_font) = fonts(ctx);
    let n = &toast.notification;
    let text_x = px(PAD_X + ICON_BOX + TEXT_GAP);
    let text_width = (width - text_x - px(CLOSE + 10.0 + 6.0)).max(px(40.0));
    let count =
        (toast.count > 1).then(|| text.shape_line(&format!("×{}", toast.count), &count_font));
    let count_room = count.as_ref().map_or(0.0, |c| c.width + px(14.0));
    let title_room = (text_width - count_room).max(px(20.0));
    let title: Vec<Arc<ShapedLine>> =
        wrap_lines(text, &n.title, &title_font, title_room, TITLE_LINES)
            .iter()
            .map(|l| text.shape_line(l, &title_font))
            .collect();
    let detail = n.detail.as_deref().map_or_else(Vec::new, |d| {
        wrap_lines(text, d, &detail_font, text_width, DETAIL_LINES)
            .iter()
            .map(|l| text.shape_line(l, &detail_font))
            .collect()
    });
    let mut x = 0.0;
    let actions = n
        .actions
        .iter()
        .map(|a| {
            let label = ds::ellipsize(
                text,
                &a.label,
                &action_font,
                text_width / 2.0 - px(ACTION_PAD * 2.0),
            );
            let shaped = text.shape_line(&label, &action_font);
            let w = (shaped.width + px(ACTION_PAD * 2.0)).round();
            let at = x;
            x += w + px(8.0);
            (shaped, at, w, a.command)
        })
        .collect::<Vec<_>>();
    let body = px(TITLE_LINE) * title.len().max(1) as f32
        + if detail.is_empty() { 0.0 } else { px(3.0) + px(DETAIL_LINE) * detail.len() as f32 };
    let mut height = px(PAD_Y) * 2.0 + body.max(px(ICON_BOX));
    let progress = (n.kind == NotificationKind::Progress || n.progress.is_some())
        .then_some(n.progress_fraction());
    if progress.is_some() {
        height += px(PROGRESS_TOP) + ds::ProgressBar::from(None).thickness(PROGRESS).height(ctx);
    }
    if !actions.is_empty() {
        height += px(ACTION_TOP + ACTION_HEIGHT);
    }
    CardLayout {
        id: n.id,
        kind: n.kind,
        width,
        height: height.round(),
        title,
        count,
        detail,
        actions,
        text_x,
        text_width,
        progress,
    }
}

/// How much what's under a card is blurred (logical pixels).
const GLASS_BLUR: f32 = 18.0;

/// The progress bar's thickness and the space over it (logical pixels).
const PROGRESS: f32 = 3.0;
const PROGRESS_TOP: f32 = 8.0;

/// `value` in at most `max` lines of `width`, by words; the last one cut with "…" when
/// there's more.
fn wrap_lines(
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    width: f32,
    max: usize,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut words = value.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if text.shape_line(&candidate, font).width <= width || line.is_empty() {
            line = candidate;
            continue;
        }
        if lines.len() + 1 == max {
            // The last line: the rest of it, cut.
            let rest: Vec<&str> = std::iter::once(word).chain(words.by_ref()).collect();
            line = format!("{line} {}", rest.join(" "));
            break;
        }
        lines.push(std::mem::replace(&mut line, word.to_owned()));
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.truncate(max);
    lines.into_iter().map(|l| ds::ellipsize(text, &l, font, width)).collect()
}

/// What's under the mouse on the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastHit {
    Close(NotificationId),
    Action(NotificationId, CommandId),
    /// Elsewhere on a card (nothing happens; nothing under it gets the click).
    Card(NotificationId),
}

/// Where a card was drawn, its × and its actions (physical pixels).
#[derive(Debug, Clone, Copy)]
struct Placed {
    id: NotificationId,
    card: Rect,
    close: Rect,
    actions: [Option<(Rect, CommandId)>; 2],
}

/// Where the stack goes: the area it may take (under the tab bar to above the bottom bar,
/// across the editor) and the edge chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StackPlace {
    pub area: Rect,
    pub position: ToastPosition,
    pub scale: f32,
}

/// The stack as drawn: its layout (kept until the content or the width change), where
/// each card went and what's under the mouse.
#[derive(Debug, Default)]
pub struct ToastStack {
    layout: Memo<LayoutKey, Vec<CardLayout>>,
    placed: Vec<Placed>,
    hover: Option<ToastHit>,
    /// Each counting card's ring (let go with the card, all of them when none is shown).
    rings: Vec<(NotificationId, ProgressRing)>,
}

impl ToastStack {
    /// The cards' width in `place` (physical pixels).
    pub fn card_width(place: &StackPlace) -> f32 {
        let px = |v: f32| (v * place.scale).round();
        let room = place.area.size.width;
        (room - 2.0 * px(MARGIN)).min(px(WIDTH)).max(px(160.0).min(room)).round()
    }

    /// The cards laid out (what the tests read): computed again only when what's shown or
    /// the width changed.
    pub fn layout(
        &mut self,
        text: &mut TextSystem,
        palette: &Palette,
        notifications: &Notifications,
        place: &StackPlace,
    ) -> &[CardLayout] {
        let ctx = Ctx::new(palette, place.scale);
        let width = Self::card_width(place);
        let key = LayoutKey {
            generation: notifications.generation(),
            width,
            scale: place.scale,
            font: palette.ui_font_size,
        };
        self.layout.get_or_update(key, || {
            notifications.shown().map(|t| layout_card(text, &ctx, t, width)).collect()
        })
    }

    /// Draws the stack (nothing when it's empty).
    pub fn paint(
        &mut self,
        painter: Painter<'_>,
        notifications: &Notifications,
        place: StackPlace,
    ) {
        self.placed.clear();
        if notifications.is_empty() {
            if self.rings.capacity() > 0 {
                self.rings = Vec::new();
            }
            return;
        }
        let Painter { text, scene, palette, icons } = painter;
        self.layout(text, palette, notifications, &place);
        let Some(cards) = self.layout.value() else { return };
        let px = |v: f32| (v * place.scale).round();
        let top = place.position.top();
        let width = Self::card_width(&place);
        let x = if place.position.centered() {
            (place.area.min_x() + (place.area.size.width - width) / 2.0).round()
        } else {
            place.area.max_x() - px(MARGIN) - width
        };
        // From the edge: the newest first.
        let mut edge = if top { place.area.min_y() } else { place.area.max_y() };
        for toast in notifications.shown().collect::<SmallRev<'_>>().iter() {
            let Some(card) = cards.iter().find(|c| c.id == toast.id()) else { continue };
            let presence = toast.presence();
            let slide = (1.0 - toast.entered()) * px(SLIDE);
            let y = if top { edge - slide } else { edge - card.height + slide };
            let rect = Rect::new(x, y.round(), width, card.height);
            let step = (card.height + px(GAP)) * presence;
            edge = if top { edge + step } else { edge - step };
            if presence <= 0.01 {
                continue;
            }
            let ctx = Ctx::new(palette, place.scale).with_opacity(presence);
            let layer = scene.push_layer(None);
            let ring = ring_steps(toast.remaining()).map(|steps| {
                let at = match self.rings.iter().position(|(id, _)| *id == card.id) {
                    Some(at) => at,
                    None => {
                        self.rings.push((card.id, ProgressRing::default()));
                        self.rings.len() - 1
                    }
                };
                (&mut self.rings[at].1, steps)
            });
            let placed = paint_card(layer, icons, &ctx, (card, toast), rect, self.hover, ring);
            self.placed.push(placed);
        }
        // The rings of cards gone (or no longer counting).
        let placed = &self.placed;
        self.rings.retain(|(id, _)| placed.iter().any(|p| p.id == *id));
    }

    /// The bytes the rings' images hold.
    pub fn memory(&self) -> usize {
        self.rings.iter().map(|(_, r)| r.memory()).sum()
    }

    /// How many rings are kept (tests).
    pub fn rings(&self) -> impl Iterator<Item = &ProgressRing> {
        self.rings.iter().map(|(_, r)| r)
    }

    /// What's at `p` on the cards drawn.
    pub fn hit(&self, p: Point) -> Option<ToastHit> {
        let placed = self.placed.iter().find(|c| c.card.contains(p))?;
        if placed.close.contains(p) {
            return Some(ToastHit::Close(placed.id));
        }
        for (rect, command) in placed.actions.iter().flatten() {
            if rect.contains(p) {
                return Some(ToastHit::Action(placed.id, *command));
            }
        }
        Some(ToastHit::Card(placed.id))
    }

    /// Whether `p` is on a card.
    pub fn contains(&self, p: Point) -> bool {
        self.placed.iter().any(|c| c.card.contains(p))
    }

    /// The mouse at `p`: `true` if what's under it changed (a redraw).
    pub fn hover(&mut self, p: Point) -> bool {
        let hit = self.hit(p);
        std::mem::replace(&mut self.hover, hit) != hit
    }

    /// The mouse over a button (×, an action).
    pub fn over_button(&self) -> bool {
        matches!(self.hover, Some(ToastHit::Close(_) | ToastHit::Action(..)))
    }

    /// The mouse is over the stack (its time stands still).
    pub fn hovered(&self) -> bool {
        self.hover.is_some()
    }

    /// The union of the cards drawn (where the stack is on screen).
    pub fn bounds(&self) -> Option<Rect> {
        let first = self.placed.first()?.card;
        Some(self.placed.iter().fold(first, |r, c| {
            let (x0, y0) = (r.min_x().min(c.card.min_x()), r.min_y().min(c.card.min_y()));
            let (x1, y1) = (r.max_x().max(c.card.max_x()), r.max_y().max(c.card.max_y()));
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        }))
    }

    /// Forgets the layout (the fonts or the theme changed).
    pub fn invalidate(&mut self) {
        self.layout = Memo::default();
    }
}

/// The shown toasts newest first, without allocating (at most a few: kept on the stack).
struct SmallRev<'a> {
    items: [Option<&'a Toast>; 8],
    len: usize,
}

impl<'a> FromIterator<&'a Toast> for SmallRev<'a> {
    fn from_iter<I: IntoIterator<Item = &'a Toast>>(iter: I) -> Self {
        let mut items = [None; 8];
        let mut len = 0;
        for toast in iter {
            if len < items.len() {
                items[len] = Some(toast);
                len += 1;
            }
        }
        items[..len].reverse();
        Self { items, len }
    }
}

impl<'a> SmallRev<'a> {
    fn iter(&self) -> impl Iterator<Item = &'a Toast> + '_ {
        self.items[..self.len].iter().flatten().copied()
    }
}

/// One card at `rect`.
fn paint_card(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    (card, toast): (&CardLayout, &Toast),
    rect: Rect,
    hover: Option<ToastHit>,
    ring: Option<(&mut ProgressRing, u16)>,
) -> Placed {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let color = kind_color(card.kind, p);
    let dark = crate::tag_colors::is_dark(p);
    // Frosted glass: what's under the card blurred, a translucent surface over it (the
    // blur seen through), a soft shadow.
    let corner = px(radius::LG);
    layer.backdrop_blur_rounded(rect, px(GLASS_BLUR), ctx.opacity, corner);
    let glass = p.popup_background.with_alpha(p.popup_background.a * 0.72);
    ds::surface_with(layer, ctx, rect, Elevation::Medium, radius::LG, glass);
    glow(layer, ctx, rect, color, if dark { 0.16 } else { 0.10 });
    // The icon in its tinted square (a spinner for work in progress).
    let icon_box =
        Rect::new(rect.min_x() + px(PAD_X), rect.min_y() + px(PAD_Y), px(ICON_BOX), px(ICON_BOX));
    layer.quad(
        Quad::filled(icon_box, ctx.fade(color.with_alpha(if dark { 0.18 } else { 0.13 })))
            .rounded(px(radius::SM + 2.0)),
    );
    if card.kind == NotificationKind::Progress {
        spinner(layer, ctx, icon_box, color, toast.age);
    } else if let Some(id) = icons.id(kind_icon(card.kind)) {
        let s = px(ICON);
        let at = Rect::new(
            icon_box.min_x() + (icon_box.size.width - s) / 2.0,
            icon_box.min_y() + (icon_box.size.height - s) / 2.0,
            s,
            s,
        );
        draw_icon(layer, icons, id, at, ctx.fade(color));
    }
    // The texts: the title centered on the icon when it's alone.
    let tx = rect.min_x() + card.text_x;
    let (title_font, detail_font, count_font, action_font) = fonts(ctx);
    let lone = card.detail.is_empty() && card.title.len() <= 1;
    let title_top = if lone {
        icon_box.min_y() + (icon_box.size.height - px(TITLE_LINE)) / 2.0
    } else {
        rect.min_y() + px(PAD_Y)
    };
    let mut line_top = title_top;
    for line in &card.title {
        let baseline = ds::optical_baseline(line_top, px(TITLE_LINE), &title_font);
        draw_text(layer, line, tx, baseline, ctx.fade(p.text));
        line_top += px(TITLE_LINE);
    }
    let last_top = line_top - px(TITLE_LINE);
    let last_width = card.title.last().map_or(0.0, |l| l.width);
    if let Some(count) = &card.count {
        let pill = Rect::new(
            tx + last_width + px(6.0),
            last_top + (px(TITLE_LINE) - px(16.0)) / 2.0,
            count.width + px(8.0),
            px(16.0),
        );
        layer.quad(Quad::filled(pill, ctx.fade(p.text.with_alpha(0.09))).rounded(px(8.0)));
        let base = ds::optical_baseline(pill.min_y(), pill.size.height, &count_font);
        draw_text(layer, count, pill.min_x() + px(4.0), base, ctx.fade(p.text_muted));
    }
    let mut y = line_top + px(3.0);
    for line in &card.detail {
        let base = ds::optical_baseline(y, px(DETAIL_LINE), &detail_font);
        draw_text(layer, line, tx, base, ctx.fade(p.text_muted));
        y += px(DETAIL_LINE);
    }
    // How far, under the texts (over the actions), in the kind's color.
    if let Some(fraction) = card.progress {
        let top = rect.min_y() + px(PAD_Y) + (y - rect.min_y() - px(PAD_Y)).max(px(ICON_BOX));
        let bar = Rect::new(tx, top + px(PROGRESS_TOP), card.text_width, px(PROGRESS));
        ds::ProgressBar::from(fraction).paint_bar(layer, ctx, bar);
    }
    // The actions, under the texts: the first tinted in the kind's color.
    let mut actions = [None, None];
    let actions_top = rect.max_y() - px(PAD_Y) - px(ACTION_HEIGHT);
    for (i, (label, at, w, command)) in card.actions.iter().enumerate().take(2) {
        let r = Rect::new(tx + at, actions_top, *w, px(ACTION_HEIGHT));
        let hovered = hover == Some(ToastHit::Action(card.id, *command));
        let (fill, text_color, border) = if i == 0 {
            (color.with_alpha(if hovered { 0.24 } else { 0.15 }), color, None)
        } else {
            (p.text.with_alpha(if hovered { 0.08 } else { 0.0 }), p.text, Some(p.border))
        };
        let mut quad = Quad::filled(r, ctx.fade(fill)).rounded(px(radius::SM));
        if let Some(border) = border {
            quad = quad.bordered(ctx.line(), ctx.fade(border));
        }
        layer.quad(quad);
        let base = ds::optical_baseline(r.min_y(), r.size.height, &action_font);
        draw_text(
            layer,
            label,
            (r.min_x() + (w - label.width) / 2.0).round(),
            base,
            ctx.fade(text_color),
        );
        actions[i] = Some((r, *command));
    }
    // ×, the time left as a ring around it.
    let close = Rect::new(
        rect.max_x() - px(10.0) - px(CLOSE),
        rect.min_y() + px(10.0),
        px(CLOSE),
        px(CLOSE),
    );
    let over_close = hover == Some(ToastHit::Close(card.id));
    if over_close {
        layer.quad(
            Quad::filled(close, ctx.fade(p.text.with_alpha(0.10))).rounded(close.size.width / 2.0),
        );
    }
    if let Some((cache, steps)) = ring {
        time_left(layer, ctx, cache, close, color, steps);
    }
    if let Some(id) = icons.id("x") {
        let s = px(12.0);
        let at = Rect::new(
            close.min_x() + (close.size.width - s) / 2.0,
            close.min_y() + (close.size.height - s) / 2.0,
            s,
            s,
        );
        draw_icon(layer, icons, id, at, ctx.fade(if over_close { p.text } else { p.text_muted }));
    }
    Placed { id: card.id, card: rect, close, actions }
}

/// A soft glow of `color` from the card's top-left corner: pills anchored at that corner,
/// growing, each faint (a radial gradient made of layers, as the shadows are). Every pill
/// is at least as rounded as the card, so none crosses its corners.
fn glow(layer: &mut Layer, ctx: &Ctx<'_>, card: Rect, color: Rgba, strength: f32) {
    const STEPS: usize = 16;
    let h_max = card.size.height;
    let w_max = (card.size.width * 0.62).max(h_max);
    let min_r = ctx.px(radius::LG);
    for i in 1..=STEPS {
        let k = i as f32 / STEPS as f32;
        let h = (h_max * k).max(2.0 * min_r);
        let w = (w_max * k).max(h);
        let r = Rect::new(card.min_x(), card.min_y(), w, h);
        let alpha = strength / STEPS as f32;
        layer.quad(Quad::filled(r, ctx.fade(color.with_alpha(alpha))).rounded(h / 2.0));
    }
}

/// The time left: a faint track around `close` and an arc of `steps` (of [`RING_STEPS`])
/// in `color` over it, clockwise from the top, smooth ([`ProgressRing`]).
fn time_left(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    cache: &mut ProgressRing,
    close: Rect,
    color: Rgba,
    steps: u16,
) {
    let spec = RingSpec {
        size: close.size.width.round().max(0.0) as u32,
        radius: close.size.width / 2.0 - ctx.px(1.0),
        stroke: (ctx.scale * 1.6).max(1.5),
        color: color.with_alpha(0.9),
        track: Some(ctx.palette.text.with_alpha(0.08)),
    };
    let center =
        Point::new(close.min_x() + close.size.width / 2.0, close.min_y() + close.size.height / 2.0);
    let sweep = f32::from(steps) / f32::from(RING_STEPS);
    cache.paint(layer, center, &spec, (0.0, sweep), ctx.opacity);
}

/// Work going on: eight dots turning in `rect`, one step every 90 ms of `age`.
fn spinner(layer: &mut Layer, ctx: &Ctx<'_>, rect: Rect, color: Rgba, age: std::time::Duration) {
    const DOTS: usize = 8;
    let step = (age.as_millis() / SPINNER_STEP.as_millis()) as usize % DOTS;
    let size = ctx.px(16.0);
    let (cx, cy) = (rect.min_x() + rect.size.width / 2.0, rect.min_y() + rect.size.height / 2.0);
    let radius = size * 0.38;
    let dot = (size * 0.2).max(2.0);
    for i in 0..DOTS {
        let angle = i as f32 / DOTS as f32 * std::f32::consts::TAU;
        let age = (step + DOTS - i) % DOTS;
        let alpha = 1.0 - age as f32 / DOTS as f32 * 0.85;
        let r = Rect::new(
            cx + angle.cos() * radius - dot / 2.0,
            cy + angle.sin() * radius - dot / 2.0,
            dot,
            dot,
        );
        layer.quad(Quad::filled(r, ctx.fade(color.with_alpha(color.a * alpha))).rounded(dot / 2.0));
    }
}

#[cfg(test)]
#[path = "../tests/unit/toast.rs"]
mod tests;
