//! The search field (its pill, its magnifier), the fields' styles and the clear button.

use super::*;

/// The app's search field: a pill with the magnifier on the left (as in the project panel).
/// Returns the area where the typed text is drawn.
pub fn search_field(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    focused: bool,
) -> Rect {
    let p = ctx.palette;
    let border = if focused { p.search_field_border_focused } else { p.search_field_border };
    layer.quad(
        Quad::filled(rect, ctx.fade(p.search_field_background))
            .rounded(rect.size.height / 2.0)
            .bordered(ctx.line(), ctx.fade(border)),
    );
    let x = rect.min_x() + ctx.px(MAGNIFIER_LEFT);
    magnifier(layer, icons, ctx, x, rect);
    let left = x + ctx.px(MAGNIFIER) + ctx.px(9.0);
    Rect::new(left, rect.min_y(), rect.max_x() - left - ctx.px(14.0), rect.size.height)
}

/// A form's text field style (a dialog's, a wizard's) in `font`; `invalid`: its border in
/// the danger color (what's typed can't be used).
pub fn field_style(ctx: &Ctx<'_>, font: Font, invalid: bool) -> ui_kit::TextInputStyle {
    let p = ctx.palette;
    ui_kit::TextInputStyle {
        font,
        background: p.input_background,
        border: if invalid { p.danger } else { p.input_border },
        border_focused: if invalid { p.danger } else { p.accent },
        text: p.text,
        placeholder: p.text_muted,
        selection: p.selection,
        caret: p.caret,
        radius: ctx.px(super::radius::MD),
        border_width: ctx.line(),
        padding: ctx.px(10.0),
        caret_width: ctx.px(1.5).max(1.0),
        lead: 0.0,
        trail: 0.0,
    }
}

/// A search field's text style: the app's search pill, room left for its magnifier.
pub fn search_input_style(ctx: &Ctx<'_>, height: f32) -> ui_kit::TextInputStyle {
    let p = ctx.palette;
    ui_kit::TextInputStyle {
        font: ctx.ui_font(0.0),
        background: p.search_field_background,
        border: p.search_field_border,
        border_focused: p.search_field_border_focused,
        text: p.text,
        placeholder: p.text_muted,
        selection: p.selection,
        caret: p.caret,
        radius: height / 2.0,
        border_width: ctx.line(),
        padding: ctx.px(14.0),
        caret_width: ctx.px(1.5).max(1.0),
        lead: ctx.px(22.0),
        trail: 0.0,
    }
}

/// A search field being typed in (`input`) in `rect`: the pill, its magnifier, the text
/// (the placeholder while it's empty).
pub fn search_input(
    pen: &mut LayerPainter<'_>,
    rect: Rect,
    input: &ui_kit::TextInput,
    focused: bool,
    placeholder: &str,
) {
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let style = search_input_style(ctx, rect.size.height);
    input.paint(layer, text, rect, focused, placeholder, &style);
    magnifier(layer, icons, ctx, (rect.min_x() + ctx.px(MAGNIFIER_LEFT)).round(), rect);
}

/// A search pill's magnifier: its side, and its place from the pill's left (logical pixels).
const MAGNIFIER: f32 = 15.0;
const MAGNIFIER_LEFT: f32 = 13.0;

/// The magnifier of a search pill in `rect`, from `x`, centered on its height.
fn magnifier(layer: &mut Layer, icons: &icons::Icons, ctx: &Ctx<'_>, x: f32, rect: Rect) {
    let Some(id) = icons.id("search") else { return };
    let s = ctx.px(MAGNIFIER);
    let r = Rect::new(x, (rect.min_y() + (rect.size.height - s) / 2.0).round(), s, s);
    ui_kit::draw_icon(layer, icons, id, r, ctx.fade(ctx.palette.text_muted));
}

/// A field's clear button's side, and its ×'s (logical pixels).
const CLEAR: f32 = 18.0;
const CLEAR_ICON: f32 = 9.0;

/// The round clear button of a field (×), centered on `(cx, mid)`. Returns its area.
pub fn clear_button(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    cx: f32,
    mid: f32,
    hovered: bool,
) -> Rect {
    let size = ctx.px(CLEAR);
    let r = Rect::new((cx - size / 2.0).round(), (mid - size / 2.0).round(), size, size);
    let p = ctx.palette;
    let fill = p.text.with_alpha(if hovered { 0.12 } else { 0.06 });
    layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(size / 2.0));
    if let Some(id) = icons.id("x") {
        let icon = ctx.px(CLEAR_ICON);
        let ir = Rect::new(
            (r.min_x() + (size - icon) / 2.0).round(),
            (r.min_y() + (size - icon) / 2.0).round(),
            icon,
            icon,
        );
        let color = if hovered { p.text } else { p.text_muted };
        ui_kit::draw_icon(layer, icons, id, ir, ctx.fade(color));
    }
    r
}
