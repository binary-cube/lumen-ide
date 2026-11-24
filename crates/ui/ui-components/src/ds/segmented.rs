//! The segmented control: its segments (text or icons, disabled ones), its height, where
//! it sits.

use super::*;

/// What [`segmented`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct SegmentedSpec<'a> {
    pub right: f32,
    pub mid: f32,
    pub labels: &'a [&'a str],
    pub selected: usize,
    pub hovered: Option<usize>,
}

impl<'a> SegmentedSpec<'a> {
    /// `labels` ending at `right`, centered on `mid`, the first chosen.
    pub fn new(right: f32, mid: f32, labels: &'a [&'a str]) -> Self {
        Self { right, mid, labels, selected: 0, hovered: None }
    }

    /// The one chosen (its place).
    pub fn selected(self, selected: usize) -> Self {
        Self { selected, ..self }
    }

    /// The one under the mouse.
    pub fn hovered(self, hovered: Option<usize>) -> Self {
        Self { hovered, ..self }
    }
}

/// A segmented control (one of a few, side by side in a track: "All · Dark · Light"),
/// ending at `right`, centered on `mid`; the `selected` segment is a raised card. Returns
/// each segment's area (for clicks).
pub fn segmented(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: SegmentedSpec<'_>,
) -> Vec<Rect> {
    let mut out = Vec::with_capacity(spec.labels.len());
    segmented_with(layer, text, ctx, spec, |_, r| out.push(r));
    out
}

/// [`segmented`], each segment's area handed to `each` (its index, its area) instead of
/// collected: nothing allocated (a view drawing it every frame).
pub fn segmented_with(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: SegmentedSpec<'_>,
    each: impl FnMut(usize, Rect),
) {
    paint_segmented(layer, text, None, ctx, spec, SegmentedLook::default(), each);
}

/// The default height of a [`segmented`] control (logical pixels).
pub const SEGMENTED_H: f32 = 32.0;

/// Where a [`segmented_styled`] control stands.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum SegmentPlace {
    /// Ending at its spec's `right`, each segment as wide as its label.
    #[default]
    Right,
    /// Starting at `x`, each segment as wide as its label.
    Left(f32),
    /// Across `width` from `x`, its segments equal, their labels cut with `…` to fit.
    Fill { x: f32, width: f32 },
}

/// How a [`segmented_styled`] control looks beyond its [`SegmentedSpec`]; the default is
/// [`segmented`]'s.
#[derive(Debug, Clone, Copy)]
pub struct SegmentedLook<'a> {
    /// Its height (logical pixels).
    pub height: f32,
    pub place: SegmentPlace,
    /// The segments that can't be chosen now: greyed, never lit, not handed to `each`.
    pub disabled: &'a [usize],
    /// Each segment's icon, before its label (`""`: none); empty: no icons.
    pub icons: &'a [&'a str],
}

impl Default for SegmentedLook<'_> {
    fn default() -> Self {
        Self { height: SEGMENTED_H, place: SegmentPlace::Right, disabled: &[], icons: &[] }
    }
}

/// [`segmented_with`] as `look` says: its height, where it stands (from the left, or
/// filling a width), segments disabled, icons. Only the segments that can be pressed are
/// handed to `each`.
pub fn segmented_styled(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    (spec, look): (SegmentedSpec<'_>, SegmentedLook<'_>),
    each: impl FnMut(usize, Rect),
) {
    paint_segmented(layer, text, Some(icons), ctx, spec, look, each);
}

/// The segments' room inside the track, and the room around a label (logical pixels).
const SEGMENT_INSET: f32 = 3.0;
const SEGMENT_PAD: f32 = 14.0;
/// A segment's icon, and the room after it (logical pixels).
const SEGMENT_ICON: f32 = 14.0;
const SEGMENT_ICON_GAP: f32 = 6.0;

fn paint_segmented(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: Option<&icons::Icons>,
    ctx: &Ctx<'_>,
    spec: SegmentedSpec<'_>,
    look: SegmentedLook<'_>,
    mut each: impl FnMut(usize, Rect),
) {
    let SegmentedSpec { right, mid, labels, selected, hovered } = spec;
    let p = ctx.palette;
    let font = ctx.ui_font(-0.5);
    let (h, inset, pad) = (ctx.px(look.height), ctx.px(SEGMENT_INSET), ctx.px(SEGMENT_PAD));
    let icon_of = |i: usize| {
        let name = look.icons.get(i).copied().filter(|n| !n.is_empty())?;
        icons?.id(name)
    };
    let icon_w =
        |i: usize| icon_of(i).map_or(0.0, |_| ctx.px(SEGMENT_ICON) + ctx.px(SEGMENT_ICON_GAP));
    let natural = |text: &mut TextSystem, i: usize, l: &str| {
        (text.shape_line(l, &font).width + icon_w(i) + 2.0 * pad).round()
    };
    let (track, fill_w) = match look.place {
        SegmentPlace::Fill { x, width } => {
            let cell = ((width - 2.0 * inset) / labels.len().max(1) as f32).floor();
            (Rect::new(x.round(), (mid - h / 2.0).round(), width, h), Some(cell))
        }
        SegmentPlace::Right | SegmentPlace::Left(_) => {
            let total = labels.iter().enumerate().map(|(i, l)| natural(text, i, l)).sum::<f32>()
                + 2.0 * inset;
            let x = match look.place {
                SegmentPlace::Left(x) => x,
                _ => right - total,
            };
            (Rect::new(x.round(), (mid - h / 2.0).round(), total, h), None)
        }
    };
    layer.quad(
        Quad::filled(track, ctx.fade(p.text.with_alpha(0.05)))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(p.border)),
    );
    let mut x = track.min_x() + inset;
    for (i, label) in labels.iter().enumerate() {
        let w = match fill_w {
            Some(cell) => cell,
            None => natural(text, i, label),
        };
        let seg = Rect::new(x, track.min_y() + inset, w, h - 2.0 * inset);
        let on = i == selected;
        let off = look.disabled.contains(&i);
        if on {
            layer.quad(
                Quad::filled(seg, ctx.fade(p.popup_background))
                    .rounded(ctx.px(radius::MD) - inset)
                    .bordered(ctx.line(), ctx.fade(p.card_border)),
            );
        } else if hovered == Some(i) && !off {
            layer.quad(
                Quad::filled(seg, ctx.fade(p.text.with_alpha(0.04)))
                    .rounded(ctx.px(radius::MD) - inset),
            );
        }
        let color = match (off, on) {
            (true, _) => p.text_muted.with_alpha(0.5),
            (false, true) => p.text,
            (false, false) => p.text_muted,
        };
        let room = (w - 2.0 * pad - icon_w(i)).max(0.0);
        let shaped = match fill_w {
            Some(_) if text.shape_line(label, &font).width > room => {
                let cut = ellipsize(text, label, &font, room);
                text.shape_line(&cut, &font)
            }
            _ => text.shape_line(label, &font),
        };
        let content = icon_w(i) + shaped.width;
        let mut cx = (seg.min_x() + (w - content) / 2.0).round();
        // One of a few to a screen reader: named by its label, chosen when it's the one.
        let described = || {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Radio, *label, seg).checked(on).disabled(off)
        };
        ui_kit::access::group(described, || {
            if let (Some(id), Some(icons)) = (icon_of(i), icons) {
                let s = ctx.px(SEGMENT_ICON);
                let mid = seg.min_y() + seg.size.height / 2.0;
                let r = Rect::new(cx, (mid - s / 2.0).round(), s, s);
                ui_kit::draw_icon(layer, icons, id, r, ctx.fade(color));
                cx += icon_w(i);
            }
            draw_text(
                layer,
                &shaped,
                cx,
                optical_baseline(seg.min_y(), seg.size.height, &font),
                ctx.fade(color),
            );
        });
        if !off {
            each(i, seg);
        }
        x += w;
    }
}

/// How wide [`segmented`] draws these labels (to place it from its left edge).
pub fn segmented_width(text: &mut TextSystem, ctx: &Ctx<'_>, labels: &[&str]) -> f32 {
    let font = ctx.ui_font(-0.5);
    labels.iter().map(|l| (text.shape_line(l, &font).width + ctx.px(28.0)).round()).sum::<f32>()
        + ctx.px(6.0)
}
