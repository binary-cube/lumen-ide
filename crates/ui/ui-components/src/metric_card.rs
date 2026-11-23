//! A metric's card: what it is, its value now (large), a line under it (muted — or, when a
//! threshold is passed, in the warning color *and* with a sign: never by color alone), and
//! its sparkline along the bottom (one or two series, a legend for two). Lit under the
//! mouse; the accent's edge when the table is sorted by it; the focus ring when it has the
//! keyboard. As a pill ([`CardForm::Pill`], a narrow page's band): the label and value on
//! one line, a small sparkline beside. Draws only — nothing allocated: the texts are the
//! caller's (kept per look), long ones cut with "…" as they're drawn.

use icons::Icons;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use crate::charts::spark::{self, Plot, Style};
use crate::ds::{self, Ctx, radius};

/// A series on a card.
#[derive(Debug, Clone, Copy)]
pub struct CardSeries<'a> {
    pub plot: &'a Plot,
    pub color: Rgba,
    /// Its name in the legend (shown when the card has two).
    pub label: &'a str,
}

/// What a card shows.
#[derive(Debug, Clone, Copy)]
pub struct MetricCard<'a> {
    pub label: &'a str,
    /// The value now (`3.4%`, `412 MB`).
    pub value: &'a str,
    /// The line under it (`of 12 cores · max 18%`).
    pub sub: Option<&'a str>,
    /// A threshold passed: the line in the warning color, with its sign.
    pub warning: bool,
    pub series: &'a [CardSeries<'a>],
    /// The sparkline's scale top.
    pub max: f32,
    pub hovered: bool,
    /// The table is sorted by its metric.
    pub active: bool,
    /// It has the keyboard.
    pub focused: bool,
}

impl<'a> MetricCard<'a> {
    /// A card of `label` showing `value` and `series` up to `max`: no line under it, no
    /// warning, not lit.
    pub fn new(label: &'a str, value: &'a str, series: &'a [CardSeries<'a>], max: f32) -> Self {
        let (sub, warning, hovered, active, focused) = (None, false, false, false, false);
        Self { label, value, sub, warning, series, max, hovered, active, focused }
    }

    /// The line under it.
    pub fn line_under(self, sub: &'a str) -> Self {
        Self { sub: Some(sub), ..self }
    }

    /// A threshold passed.
    pub fn warning(self, warning: bool) -> Self {
        Self { warning, ..self }
    }

    pub fn hovered(self, hovered: bool) -> Self {
        Self { hovered, ..self }
    }

    /// The table is sorted by its metric.
    pub fn active(self, active: bool) -> Self {
        Self { active, ..self }
    }

    /// It has the keyboard.
    pub fn focused(self, focused: bool) -> Self {
        Self { focused, ..self }
    }
}

/// How it's drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardForm {
    /// A card: label, value, line, sparkline.
    Card,
    /// A pill: label and value on a line, a small sparkline beside.
    Pill,
}

/// Where its parts went (for the mouse: the value's tooltip, the chart's reading).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CardAreas {
    pub value: Rect,
    pub chart: Rect,
}

/// The card's background: lit under the mouse, the accent's edge when active.
fn frame(layer: &mut Layer, ds: &Ctx<'_>, rect: Rect, card: &MetricCard<'_>) {
    let p = ds.palette;
    let fill = if card.hovered { p.text.with_alpha(0.06) } else { p.text.with_alpha(0.03) };
    let border = if card.active { p.accent.with_alpha(0.75) } else { p.card_border };
    let r = ds.px(radius::MD);
    layer.quad(Quad::filled(rect, ds.fade(fill)).rounded(r).bordered(ds.line(), ds.fade(border)));
    if card.focused {
        ds::focus_ring(layer, ds, rect, r);
    }
}

/// Card `card` in `rect`, as `form`.
pub fn metric_card(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ds: &Ctx<'_>,
    rect: Rect,
    card: &MetricCard<'_>,
    form: CardForm,
) -> CardAreas {
    frame(layer, ds, rect, card);
    match form {
        CardForm::Card => full(layer, text, icons, ds, rect, card),
        CardForm::Pill => pill(layer, text, ds, rect, card),
    }
}

/// Card `card` in `rect` on a pane of glass (a calm screen's widget: see
/// [`crate::screen_widgets`]), drawn with the screen's palette.
pub fn glass_metric_card(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ds: &Ctx<'_>,
    rect: Rect,
    card: &MetricCard<'_>,
    look: crate::screen_widgets::GlassLook,
) -> CardAreas {
    crate::screen_widgets::glass_pane(layer, ds, rect, look);
    full(layer, text, icons, ds, rect, card)
}

/// A full card's padding, its label's line (from the top, its height), its value's height,
/// and the heights under which it lays its label and value on one row, with the line under
/// them still shown from the second (logical pixels).
const PAD: f32 = 12.0;
const LABEL_TOP: f32 = 9.0;
const LABEL_H: f32 = 16.0;
const VALUE_H: f32 = 26.0;
const SHORT_BELOW: f32 = 88.0;
const SHORT_SUB_FROM: f32 = 76.0;

/// A pill's padding, its sparkline's width, least / most height and the room kept over and
/// under it together, and the width it's shown from (logical pixels).
const PILL_PAD: f32 = 10.0;
const PILL_SPARK_W: f32 = 28.0;
const PILL_SPARK_H: (f32, f32) = (8.0, 18.0);
const PILL_SPARK_ROOM: f32 = 16.0;
const PILL_SPARK_FROM: f32 = 96.0;

fn full(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ds: &Ctx<'_>,
    rect: Rect,
    card: &MetricCard<'_>,
) -> CardAreas {
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let pad = px(PAD);
    let x0 = rect.min_x() + pad;
    let inner = (rect.size.width - 2.0 * pad).max(0.0);
    let label_font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
    let small = ds.ui_font(-1.5);
    // Short cards: the label and the value on one row; the line under them when there's
    // height for it.
    let short = rect.size.height < px(SHORT_BELOW);
    let short_sub = short && rect.size.height >= px(SHORT_SUB_FROM) && card.sub.is_some();
    let label_top = rect.min_y() + px(LABEL_TOP);
    let label_h = px(LABEL_H);
    let base = ds::optical_baseline(label_top, label_h, &label_font);
    // The legend on the right of the label (two series).
    let mut legend_x = x0 + inner;
    if card.series.len() > 1 && !short {
        for s in card.series.iter().rev() {
            let shaped = text.shape_line(s.label, &small);
            legend_x -= shaped.width;
            draw_text(layer, &shaped, legend_x, base, ds.fade(p.text_muted));
            let dot = px(6.0);
            legend_x -= dot + px(5.0);
            let r = Rect::new(legend_x, (label_top + (label_h - dot) / 2.0).round(), dot, dot);
            layer.quad(Quad::filled(r, ds.fade(s.color)).rounded(dot / 2.0));
            legend_x -= px(10.0);
        }
    }
    let value_font =
        if short { ds.ui_font(1.0) } else { ds.ui_font(4.0) }.with_weight(Font::SEMIBOLD);
    let value_shaped = text.shape_line(card.value, &value_font);
    let value_area;
    if short {
        let vx = x0 + inner - value_shaped.width;
        let room = (vx - px(8.0) - x0).max(0.0);
        ds::draw_ellipsized(
            layer,
            text,
            card.label,
            &label_font,
            (x0, base),
            room,
            ds.fade(p.text_muted),
        );
        let vb = ds::optical_baseline(label_top, label_h, &value_font);
        draw_text(layer, &value_shaped, vx, vb, ds.fade(p.text));
        value_area = Rect::new(vx, label_top - px(2.0), value_shaped.width, label_h + px(4.0));
        if let Some(sub) = card.sub.filter(|_| short_sub) {
            let sb = ds::optical_baseline(label_top + label_h + px(1.0), px(15.0), &small);
            sub_line(layer, text, icons, ds, (x0, sb, inner), sub, card.warning);
        }
    } else {
        let room = (legend_x - x0 - px(6.0)).max(0.0);
        ds::draw_ellipsized(
            layer,
            text,
            card.label,
            &label_font,
            (x0, base),
            room,
            ds.fade(p.text_muted),
        );
        let value_top = label_top + label_h + px(2.0);
        let value_h = px(VALUE_H);
        let vb = ds::optical_baseline(value_top, value_h, &value_font);
        let shown = value_shaped.width.min(inner);
        ds::draw_ellipsized(layer, text, card.value, &value_font, (x0, vb), inner, ds.fade(p.text));
        value_area = Rect::new(x0, value_top, shown, value_h);
        // The line beside the value, on its baseline (a sign before it past a threshold).
        if let Some(sub) = card.sub.filter(|_| x0 + inner - (x0 + shown + px(10.0)) >= px(40.0)) {
            let sx = x0 + shown + px(10.0);
            sub_line(layer, text, icons, ds, (sx, vb, x0 + inner - sx), sub, card.warning);
        }
    }
    let chart_top = match (short, short_sub) {
        (true, true) => label_top + label_h + px(20.0),
        (true, false) => label_top + label_h + px(6.0),
        (false, _) => label_top + label_h + px(32.0),
    };
    let chart = Rect::new(x0, chart_top, inner, (rect.max_y() - px(8.0) - chart_top).max(0.0));
    for s in card.series {
        spark::sparkline(layer, ds, chart, s.plot, s.color, Style::Steps, card.max);
    }
    CardAreas { value: value_area, chart }
}

/// The line under (or beside) a card's value from `x` on `base` in `room`: muted, or past
/// a threshold in the warning color after its sign.
fn sub_line(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ds: &Ctx<'_>,
    (x, base, room): (f32, f32, f32),
    sub: &str,
    warning: bool,
) {
    let p = ds.palette;
    let small = ds.ui_font(-1.5);
    let color = if warning { p.warning } else { p.text_muted };
    let mut sx = x;
    if warning
        && room > ds.px(30.0)
        && let Some(id) = icons.id("alert")
    {
        let s = ds.px(12.0);
        let at = Rect::new(sx, (base - s + ds.px(1.0)).round(), s, s);
        ui_kit::draw_icon(layer, icons, id, at, ds.fade(color));
        sx += s + ds.px(4.0);
    }
    let room = (x + room - sx).max(0.0);
    ds::draw_ellipsized(layer, text, sub, &small, (sx, base), room, ds.fade(color));
}

fn pill(
    layer: &mut Layer,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    rect: Rect,
    card: &MetricCard<'_>,
) -> CardAreas {
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let pad = px(PILL_PAD);
    let label_font = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
    let value_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
    let spark_w = px(PILL_SPARK_W);
    let (least, most) = PILL_SPARK_H;
    let spark_h = (rect.size.height - px(PILL_SPARK_ROOM)).clamp(px(least), px(most));
    let spark = Rect::new(
        rect.max_x() - pad - spark_w,
        (rect.min_y() + (rect.size.height - spark_h) / 2.0).round(),
        spark_w,
        spark_h,
    );
    let room_for_spark = rect.size.width >= px(PILL_SPARK_FROM);
    let right = if room_for_spark { spark.min_x() - px(6.0) } else { rect.max_x() - pad };
    let x0 = rect.min_x() + pad;
    let base = ds::optical_baseline(rect.min_y(), rect.size.height, &value_font);
    let label_w = text.shape_line(card.label, &label_font).width;
    let label_color = if card.warning { p.warning } else { p.text_muted };
    draw_text(layer, &text.shape_line(card.label, &label_font), x0, base, ds.fade(label_color));
    let vx = x0 + label_w + px(6.0);
    let (w, _) = ds::draw_ellipsized(
        layer,
        text,
        card.value,
        &value_font,
        (vx, base),
        (right - vx).max(0.0),
        ds.fade(p.text),
    );
    let value = Rect::new(vx, rect.min_y(), w, rect.size.height);
    let chart = if room_for_spark { spark } else { Rect::default() };
    if room_for_spark {
        for s in card.series.iter().take(1) {
            spark::sparkline(layer, ds, spark, s.plot, s.color, Style::Bars, card.max);
        }
    }
    CardAreas { value, chart }
}

#[cfg(test)]
#[path = "../tests/unit/metric_card.rs"]
mod tests;
