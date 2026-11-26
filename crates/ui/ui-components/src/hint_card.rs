//! A hint's card (a coach mark): a popover's surface next to what it points at — a notch
//! toward it, a ring in the accent around it (the spotlight). Its header: an icon in a soft
//! accent circle, a small eyebrow label in the accent ("TIP"), the title under it, a step
//! counter in a tour ("2 / 4") and a × in the corner. Its text under the title, then where
//! the thing lives in the settings on a muted line of its own (a gear before it). A divider,
//! then one footer row: "Don't show again" on the left as a quiet link, the buttons on the
//! right as wide as their labels, the main one in the accent with its ↵ (only when they don't
//! fit does the link take a row of its own). The keys' focus shows as the shared focus ring.
//!
//! Placed by [`place`] (pure): under its anchor, above it when there's no room below, kept
//! inside the area; without an anchor, in the area's top-right corner. The texts are wrapped
//! once per content, width and scale; what's under the mouse is answered from the last
//! paint ([`HintCard::hit`]). It describes itself to screen readers as a dialog.

use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::TextSystem;
use ui_kit::draw_text;

use crate::ds::{self, ButtonKind, ButtonSpec, Ctx, Elevation, Tone, button_size, radius};
use crate::painter::Painter;

/// The card's width (logical pixels), narrower when the area is.
pub const WIDTH: f32 = 312.0;
/// The narrowest it gets in a small area (logical pixels): its title and buttons still read.
const MIN_WIDTH: f32 = 160.0;
/// Inside the card, and between its parts (logical).
const PAD: f32 = ds::space::LG;
const GAP_SMALL: f32 = ds::space::XS;
const GAP_PART: f32 = ds::space::MD;
/// Between the anchor and the card (logical), the notch in it.
const GAP: f32 = 10.0;
/// The notch's height (logical): its base is twice as wide.
const NOTCH: f32 = 7.0;
/// From the area's edges (logical).
const MARGIN: f32 = ds::space::MD;
/// The header's icon: its circle and the icon in it (logical).
const ICON_BOX: f32 = 32.0;
const ICON: f32 = 16.0;
/// How strongly the icon's circle is tinted by the accent.
const ICON_TINT: f32 = 0.14;
/// The lines' heights (logical): the eyebrow's, the title's, the text's.
const EYEBROW_LINE: f32 = 14.0;
const TITLE_LINE: f32 = 20.0;
const BODY_LINE: f32 = 18.0;
/// The place line's gear (logical).
const PLACE_ICON: f32 = 13.0;
/// The most lines of text shown (the last one ends in `…` when there's more).
const BODY_LINES: usize = 6;
/// The buttons' height and the gap between them (logical): a modal footer's compact ones.
const BUTTON_H: f32 = button_size::LARGE;
const BUTTON_GAP: f32 = ds::space::SM;
/// The ×'s size (logical).
const CLOSE: f32 = button_size::SMALL;
/// The spotlight's ring: inside the anchor by this much (never over what's around it: a
/// tab strip's edge), its thickness (logical), its strength (the accent's alpha).
const RING_IN: f32 = 1.0;
const RING: f32 = 1.5;
const RING_ALPHA: f32 = 0.7;

/// A button of the card: its label, whether it's the main one (accent, ↵) or set aside on
/// the left (a quiet link: "Don't show again").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardButton<'a> {
    pub label: &'a str,
    pub main: bool,
    pub aside: bool,
}

/// What the card shows.
#[derive(Debug, Clone, Copy)]
pub struct HintCardSpec<'a> {
    /// Its header's icon (an icon's name), in a soft accent circle; `None`: none.
    pub icon: Option<&'a str>,
    /// The small label over the title, in the accent ("TIP"), shown in capitals.
    pub eyebrow: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    /// Where it lives in the settings ("Settings › Terminal › …"): a muted line of its own.
    pub place: Option<&'a str>,
    /// "2 / 4" in a tour.
    pub counter: Option<(usize, usize)>,
    pub buttons: &'a [CardButton<'a>],
    /// The button the keys are on (the card has the keyboard).
    pub focus: Option<usize>,
}

/// Where it goes: what it points at (`None`: nothing), the area it stays in, the scale.
#[derive(Debug, Clone, Copy)]
pub struct HintPlace {
    pub anchor: Option<Rect>,
    pub area: Rect,
    pub scale: f32,
    /// A thin ring drawn just inside the anchor; off when the anchor shows itself otherwise
    /// (a tab tinted).
    pub spotlight: bool,
}

/// The notch's side and place: on the card's top edge (the card under its anchor) or its
/// bottom one, its tip at `x`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notch {
    pub top: bool,
    pub x: f32,
}

/// What's under a point of the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardHit {
    Close,
    Button(usize),
    /// Elsewhere on it.
    Card,
}

/// Where a card of `size` goes for `anchor` in `area` (physical pixels; `gap` between them,
/// `margin` from the area's edges): under it, else above it, kept inside; without an anchor,
/// the area's top-right corner.
pub fn place(
    (width, height): (f32, f32),
    anchor: Option<Rect>,
    area: Rect,
    (gap, margin): (f32, f32),
) -> (Rect, Option<Notch>) {
    let width = width.min((area.size.width - 2.0 * margin).max(0.0));
    let Some(anchor) = anchor else {
        let x = area.max_x() - margin - width;
        return (Rect::new(x.max(area.min_x()), area.min_y() + margin, width, height), None);
    };
    let below = anchor.max_y() + gap;
    let above = anchor.min_y() - gap - height;
    let fits_below = below + height <= area.max_y() - margin;
    let fits_above = above >= area.min_y() + margin;
    let (y, top) = match (fits_below, fits_above) {
        (true, _) | (false, false) => (below.min(area.max_y() - margin - height), true),
        (false, true) => (above, false),
    };
    let center = anchor.min_x() + anchor.size.width / 2.0;
    let lo = area.min_x() + margin;
    let hi = (area.max_x() - margin - width).max(lo);
    let x = (center - width / 2.0).clamp(lo, hi);
    let rect = Rect::new(x, y.max(area.min_y()), width, height);
    // The notch stays off the card's rounded corners.
    let inset = gap * 2.0;
    let tip = center.clamp(x + inset, (x + width - inset).max(x + inset));
    (rect, Some(Notch { top, x: tip }))
}

/// The card's state between frames: its last paint's places (what the mouse is over), what
/// the mouse is over, its texts wrapped.
#[derive(Debug, Default)]
pub struct HintCard {
    hits: Vec<(Rect, CardHit)>,
    bounds: Option<Rect>,
    /// Its notch at the last paint (none without an anchor).
    notch: Option<Notch>,
    hover: Option<CardHit>,
    /// The text wrapped for (the text, the width, the scale), kept until any changes.
    wrapped: Option<((String, u32, u32), Vec<String>)>,
}

impl HintCard {
    /// What's under `p` (from the last paint).
    pub fn hit(&self, p: Point) -> Option<CardHit> {
        let on = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        on.or_else(|| self.contains(p).then_some(CardHit::Card))
    }

    /// Whether `p` is on the card (from the last paint).
    pub fn contains(&self, p: Point) -> bool {
        self.bounds.is_some_and(|b| b.contains(p))
    }

    /// Its notch at the last paint (where it points: none without an anchor).
    pub fn notch(&self) -> Option<Notch> {
        self.notch
    }

    /// The card's area at the last paint (none while it isn't shown).
    pub fn bounds(&self) -> Option<Rect> {
        self.bounds
    }

    /// The mouse at `p` (`None`: gone): `true` if what it's over changed.
    pub fn hover(&mut self, p: Option<Point>) -> bool {
        let over = p.and_then(|p| self.hit(p)).filter(|h| *h != CardHit::Card);
        let changed = over != self.hover;
        self.hover = over;
        changed
    }

    /// Whether the mouse is over a button (its cursor: a hand).
    pub fn over_button(&self) -> bool {
        self.hover.is_some()
    }

    /// Not shown any more: nothing of it is under the mouse.
    pub fn hide(&mut self) {
        self.hits.clear();
        self.bounds = None;
        self.hover = None;
    }

    /// The text in lines `max` wide, wrapped once for it, its width and scale.
    fn wrap(&mut self, text: &mut TextSystem, body: &str, ctx: &Ctx<'_>, max: f32) {
        let (width, scale) = (max.to_bits(), ctx.scale.to_bits());
        // Compared before anything is made: a frame allocates nothing for it.
        let fresh = self
            .wrapped
            .as_ref()
            .is_some_and(|((b, w, s), _)| b == body && *w == width && *s == scale);
        if !fresh {
            let lines = ds::wrap(text, body, &ctx.ui_font(0.0), max, BODY_LINES);
            self.wrapped = Some(((body.to_owned(), width, scale), lines));
        }
    }

    /// Draws the card for `spec` at `place`, on a layer of its own over everything.
    pub fn paint(&mut self, painter: Painter<'_>, spec: &HintCardSpec<'_>, place: HintPlace) {
        let Painter { text, scene, palette, icons } = painter;
        let ctx = Ctx::new(palette, place.scale);
        let px = |v: f32| ctx.px(v);
        let width = px(WIDTH).min((place.area.size.width - 2.0 * px(MARGIN)).max(px(MIN_WIDTH)));
        let inner = width - 2.0 * px(PAD);
        // The title starts right of the icon's circle; the text under the divider takes the
        // card's whole width.
        let lead = if spec.icon.is_some() { px(ICON_BOX) + px(GAP_PART) } else { 0.0 };
        self.wrap(text, spec.body, &ctx, inner);
        // Lent out while it's drawn (put back after): never copied.
        let wrapped = self.wrapped.take();
        let lines: &[String] = wrapped.as_ref().map_or(&[], |(_, lines)| lines.as_slice());
        // The buttons, measured: what's set aside goes on a row of its own only when they
        // don't fit beside each other.
        let specs: Vec<(usize, ButtonSpec<'_>, f32)> = spec
            .buttons
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let look = button_spec(b, self.hover == Some(CardHit::Button(i)));
                let w = ds::button_width(text, &ctx, &look, px(BUTTON_H));
                (i, look, w)
            })
            .collect();
        // The link set aside needs no gap of its own before the buttons (its padding is one).
        let asides = spec.buttons.iter().filter(|b| b.aside).count();
        let gaps = spec.buttons.len().saturating_sub(1 + asides) as f32 * px(BUTTON_GAP);
        let row_width: f32 = specs.iter().map(|(_, _, w)| w).sum::<f32>() + gaps;
        let two_rows = row_width + asides as f32 * px(GAP_SMALL) > inner;
        let footer = if two_rows { 2.0 * px(BUTTON_H) + px(GAP_SMALL) } else { px(BUTTON_H) };
        let head = (px(EYEBROW_LINE) + px(TITLE_LINE)).max(if spec.icon.is_some() {
            px(ICON_BOX)
        } else {
            0.0
        });
        let place_line = if spec.place.is_some() { px(GAP_SMALL) + px(BODY_LINE) } else { 0.0 };
        // Header, divider, text, divider, footer: a part's room on each side of a divider.
        let height = (2.0 * px(PAD)
            + head
            + 2.0 * px(GAP_PART)
            + lines.len() as f32 * px(BODY_LINE)
            + place_line
            + 2.0 * px(GAP_PART)
            + footer)
            .round();
        let (rect, notch) =
            self::place((width, height), place.anchor, place.area, (px(GAP), px(MARGIN)));
        self.hits.clear();
        self.bounds = Some(rect);
        self.notch = notch;
        let layer = scene.push_layer(None);
        // The spotlight: a thin ring in the accent just inside what it points at.
        if let Some(anchor) = place.anchor.filter(|_| place.spotlight) {
            let ring = Rect::new(
                anchor.min_x() + px(RING_IN),
                anchor.min_y() + px(RING_IN),
                (anchor.size.width - 2.0 * px(RING_IN)).max(0.0),
                (anchor.size.height - 2.0 * px(RING_IN)).max(0.0),
            );
            let clear = palette.accent.with_alpha(0.0);
            layer.quad(
                Quad::filled(ring, clear)
                    .rounded(px(radius::MD))
                    .bordered(px(RING), ctx.fade(palette.accent.with_alpha(RING_ALPHA))),
            );
        }
        let describe = || {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Dialog, spec.title, rect).value(spec.body)
        };
        ui_kit::access::group(describe, || {
            // Opaque whatever the theme's popovers are: the text under it never shows through.
            let surface = palette.popup_background.with_alpha(1.0);
            ds::surface_with(layer, &ctx, rect, Elevation::Medium, radius::LG, surface);
            if let Some(notch) = notch {
                paint_notch(layer, &ctx, rect, notch);
            }
            let x = rect.min_x() + px(PAD);
            let top = rect.min_y() + px(PAD);
            let tx = x + lead;
            // The header: the icon's circle, the eyebrow, the title; the counter and the ×.
            if let Some(icon) = spec.icon {
                let circle = Rect::new(x, top, px(ICON_BOX), px(ICON_BOX));
                let tint = ctx.fade(palette.accent.with_alpha(ICON_TINT));
                layer.quad(Quad::filled(circle, tint).rounded(px(ICON_BOX) / 2.0));
                if let Some(id) = icons.id(icon) {
                    let at = Rect::new(
                        circle.min_x() + (circle.size.width - px(ICON)) / 2.0,
                        circle.min_y() + (circle.size.height - px(ICON)) / 2.0,
                        px(ICON),
                        px(ICON),
                    );
                    ui_kit::draw_icon(layer, icons, id, at, ctx.fade(palette.accent));
                }
            }
            let close = Rect::new(
                rect.max_x() - px(GAP_SMALL) - px(CLOSE) - px(GAP_SMALL),
                rect.min_y() + px(GAP_PART) - px(GAP_SMALL),
                px(CLOSE),
                px(CLOSE),
            );
            let x_button = ButtonSpec::new(ButtonKind::Ghost)
                .icon("x")
                .round()
                .rest_tone(Tone::Neutral)
                .hovered(self.hover == Some(CardHit::Close));
            ds::button(layer, text, icons, &ctx, close, x_button);
            self.hits.push((close, CardHit::Close));
            let mut room = close.min_x() - px(GAP_SMALL) - tx;
            let eyebrow_font = ctx.ui_font(-2.0).with_weight(600);
            if let Some((at, of)) = spec.counter {
                let counter = text.shape_line(&format!("{at} / {of}"), &eyebrow_font);
                let cx = close.min_x() - px(GAP_SMALL) - counter.width;
                let base = ds::optical_baseline(top, px(EYEBROW_LINE), &eyebrow_font);
                draw_text(layer, &counter, cx, base, palette.text_muted);
                room = cx - px(GAP_SMALL) - tx;
            }
            let eyebrow = spec.eyebrow.to_uppercase();
            let eyebrow = ds::ellipsize(text, &eyebrow, &eyebrow_font, room.max(0.0));
            let shaped = text.shape_line(&eyebrow, &eyebrow_font);
            let base = ds::optical_baseline(top, px(EYEBROW_LINE), &eyebrow_font);
            draw_text(layer, &shaped, tx, base, ctx.fade(palette.accent));
            let title_font = ctx.ui_font(1.0).with_weight(600);
            let title_room = (rect.max_x() - px(PAD) - tx).max(0.0);
            let title = ds::ellipsize(text, spec.title, &title_font, title_room);
            let shaped = text.shape_line(&title, &title_font);
            let title_top = top + px(EYEBROW_LINE);
            let base = ds::optical_baseline(title_top, px(TITLE_LINE), &title_font);
            draw_text(layer, &shaped, tx, base, palette.text);
            // The divider under the header, the whole card wide; then the text, from the
            // card's edge, and where it lives in the settings.
            let under = (top + head + px(GAP_PART)).round();
            ds::divider(layer, &ctx, rect.min_x(), rect.max_x(), under);
            let mut y = under + px(GAP_PART);
            let body_font = ctx.ui_font(0.0);
            for line in lines {
                let shaped = text.shape_line(line, &body_font);
                let base = ds::optical_baseline(y, px(BODY_LINE), &body_font);
                draw_text(layer, &shaped, x, base, palette.text_muted);
                y += px(BODY_LINE);
            }
            if let Some(where_) = spec.place {
                y += px(GAP_SMALL);
                let small = ctx.ui_font(-1.0);
                if let Some(id) = icons.id(PLACE_ICON_NAME) {
                    let at = Rect::new(
                        x,
                        y + (px(BODY_LINE) - px(PLACE_ICON)) / 2.0,
                        px(PLACE_ICON),
                        px(PLACE_ICON),
                    );
                    ui_kit::draw_icon(layer, icons, id, at, palette.text_muted);
                }
                let px_text = x + px(PLACE_ICON) + px(GAP_SMALL) + px(GAP_SMALL) / 2.0;
                let room = (rect.max_x() - px(PAD) - px_text).max(0.0);
                let line = ds::ellipsize(text, where_, &small, room);
                let shaped = text.shape_line(&line, &small);
                let base = ds::optical_baseline(y, px(BODY_LINE), &small);
                draw_text(layer, &shaped, px_text, base, palette.text_muted);
            }
            // The divider over the footer, the whole card wide.
            let bottom = rect.max_y() - px(PAD) - px(BUTTON_H);
            let row = if two_rows { bottom - px(BUTTON_H) - px(GAP_SMALL) } else { bottom };
            ds::divider(layer, &ctx, rect.min_x(), rect.max_x(), (row - px(GAP_PART)).round());
            // The footer: what's set aside on the left, the rest on the right.
            let mut left = x;
            let mut right = rect.max_x() - px(PAD);
            let (aside, rest): (Vec<_>, Vec<_>) =
                specs.into_iter().partition(|(i, _, _)| spec.buttons[*i].aside);
            for (i, b, w) in aside {
                let at = Rect::new(left, bottom, w, px(BUTTON_H));
                ds::button(layer, text, icons, &ctx, at, b);
                self.hits.push((at, CardHit::Button(i)));
                if spec.focus == Some(i) {
                    ds::focus_ring(layer, &ctx, at, radius::MD);
                }
                left += w + px(GAP_SMALL);
            }
            for (i, b, w) in rest.into_iter().rev() {
                let at = Rect::new(right - w, row, w, px(BUTTON_H));
                ds::button(layer, text, icons, &ctx, at, b);
                self.hits.push((at, CardHit::Button(i)));
                if spec.focus == Some(i) {
                    ds::focus_ring(layer, &ctx, at, radius::MD);
                }
                right -= w + px(BUTTON_GAP);
            }
        });
        self.wrapped = wrapped;
    }
}

/// The place line's icon (a gear: the settings).
const PLACE_ICON_NAME: &str = "settings";

/// A card button's look: the main one in the accent with its ↵, one set aside a quiet
/// link, the others bordered.
fn button_spec<'a>(button: &CardButton<'a>, hovered: bool) -> ButtonSpec<'a> {
    let spec = match (button.main, button.aside) {
        (true, _) => ButtonSpec::new(ButtonKind::Primary).key("↵").compact(),
        (false, true) => ButtonSpec::new(ButtonKind::Ghost).rest_tone(Tone::Neutral),
        (false, false) => ButtonSpec::new(ButtonKind::Secondary).compact(),
    };
    spec.label(button.label).hovered(hovered)
}

/// The notch toward the anchor: a small triangle in the card's color on its top or bottom
/// edge, drawn as one-pixel rows (crisp at every scale).
fn paint_notch(layer: &mut render_api::Layer, ctx: &Ctx<'_>, card: Rect, notch: Notch) {
    let rows = ctx.px(NOTCH).max(1.0) as usize;
    let line = 1.0;
    let fill = ctx.fade(ctx.palette.popup_background);
    let edge = ctx.fade(ctx.palette.popup_border);
    let border = ctx.line();
    for row in 0..rows {
        // Row 0 at the tip; each row its edges in the card's border color, then its fill.
        let half = row as f32 + 1.0;
        let y = if notch.top {
            card.min_y() - rows as f32 + row as f32
        } else {
            card.max_y() + rows as f32 - row as f32 - line
        };
        let x = (notch.x - half).round();
        layer.quad(Quad::filled(Rect::new(x - border, y, 2.0 * half + 2.0 * border, line), edge));
        layer.quad(Quad::filled(Rect::new(x, y, 2.0 * half, line), fill));
    }
    // The card's border under the notch's base, covered: one surface.
    let base = rows as f32;
    let y = if notch.top { card.min_y() } else { card.max_y() - border };
    let x = (notch.x - base).round();
    layer.quad(Quad::filled(Rect::new(x, y, 2.0 * base, border), fill));
}

#[cfg(test)]
#[path = "../tests/unit/hint_card.rs"]
mod tests;
