//! A list to choose from: the field showing the value chosen (a chevron on its right). Its
//! list — and the suggestions completing a text field — is the shared picker
//! ([`crate::picker::Picker`]).

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use super::{Ctx, ellipsize, optical_baseline, radius};

/// What [`dropdown`] shows.
#[derive(Debug, Clone, Copy)]
pub struct DropdownSpec<'a> {
    pub value: &'a str,
    /// Its list is open (the field in the accent).
    pub open: bool,
    pub hovered: bool,
    /// Off: shown dimmed, never hovered (nothing to choose).
    pub enabled: bool,
    /// Muted, on its right (why it's off: "Not allowed by the server").
    pub note: Option<&'a str>,
    /// Muted, while nothing is chosen (`value` empty).
    pub placeholder: Option<&'a str>,
}

/// What a [`dropdown_styled`] field adds to its [`DropdownSpec`]; the default is
/// [`dropdown`]'s.
#[derive(Debug, Clone, Copy, Default)]
pub struct DropdownLook<'a> {
    /// Muted, before the value ("Tx:" in "Tx: Auto").
    pub prefix: Option<&'a str>,
    /// A status dot before it all, in this color.
    pub dot: Option<Rgba>,
    /// A toolbar's pill: a smaller font and less room around it (its height is `r`'s).
    pub compact: bool,
    /// The value in this color, and its chevron (a request's method: a read green, a
    /// removal red); `None`: the text's.
    pub value_color: Option<Rgba>,
}

/// The room around a field's content, its chevron, its dot (logical pixels): a regular
/// field's, a compact one's.
const PAD: (f32, f32) = (10.0, 8.0);
const CHEVRON: (f32, f32) = (14.0, 12.0);
const DOT: f32 = 7.0;
/// A leading icon (a driver's), a regular field's and a compact one's (logical pixels).
const ICON: (f32, f32) = (14.0, 12.0);
/// The room between a field's parts (dot, prefix, value, chevron), logical pixels.
const PART_GAP: f32 = 6.0;

/// The field of a list to choose from, in `r`.
pub fn dropdown(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    r: Rect,
    spec: DropdownSpec<'_>,
) {
    dropdown_styled(layer, text, icons, ctx, r, (spec, DropdownLook::default()));
}

/// The fonts of a field: its value's, its prefix's and note's.
fn fonts(ctx: &Ctx<'_>, compact: bool) -> (Font, Font) {
    if compact {
        (ctx.ui_font(-1.0), ctx.ui_font(-1.0))
    } else {
        (ctx.ui_font(-0.5), ctx.ui_font(-1.0))
    }
}

/// How wide a [`dropdown_styled`] field is to show its whole value (a toolbar's pill sized
/// to what it says): its dot, its prefix, the value (or the placeholder), its chevron.
pub fn dropdown_width(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
) -> f32 {
    width_of(text, ctx, (spec, look), false)
}

/// [`dropdown_width`] of a [`dropdown_with_icon`] field: its icon's room counted.
pub fn dropdown_width_with_icon(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
) -> f32 {
    width_of(text, ctx, (spec, look), true)
}

fn width_of(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
    icon: bool,
) -> f32 {
    let (font, small) = fonts(ctx, look.compact);
    let pick = |regular: f32, compact: f32| ctx.px(if look.compact { compact } else { regular });
    let value = match spec.placeholder {
        Some(placeholder) if spec.value.is_empty() => placeholder,
        _ => spec.value,
    };
    let mut w = 2.0 * pick(PAD.0, PAD.1) + text.shape_line(value, &font).width;
    if look.dot.is_some() {
        w += ctx.px(DOT + PART_GAP);
    }
    if icon {
        w += pick(ICON.0, ICON.1) + ctx.px(PART_GAP);
    }
    if let Some(prefix) = look.prefix {
        w += text.shape_line(prefix, &small).width + ctx.px(PART_GAP);
    }
    if spec.enabled {
        w += pick(CHEVRON.0, CHEVRON.1) + ctx.px(PART_GAP);
    }
    // Rounded up: rounded down, its own value would be cut to "…" for want of a fraction.
    w.ceil()
}

/// [`dropdown`] with what `look` adds: a muted prefix, a status dot, a compact size.
pub fn dropdown_styled(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    r: Rect,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
) {
    paint(layer, text, icons, ctx, r, (spec, look), None);
}

/// [`dropdown_styled`] with a leading icon (a driver's) after its dot, before its prefix.
pub fn dropdown_with_icon(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    r: Rect,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
    icon: &str,
) {
    paint(layer, text, icons, ctx, r, (spec, look), Some(icon));
}

/// The field to a screen reader: a choice's box showing its value, its list open or not
/// (its name is the text on its row).
fn paint(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    r: Rect,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
    icon: Option<&str>,
) {
    let described = || {
        use ui_kit::access::{Described, Role};
        let mut node =
            Described::new(Role::ComboBox, "", r).value(spec.value).disabled(!spec.enabled);
        node.state.expanded = Some(spec.open);
        node
    };
    ui_kit::access::group(described, || {
        paint_field(layer, text, icons, ctx, r, (spec, look), icon)
    });
}

fn paint_field(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    r: Rect,
    (spec, look): (DropdownSpec<'_>, DropdownLook<'_>),
    icon: Option<&str>,
) {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let pick = |regular: f32, compact: f32| px(if look.compact { compact } else { regular });
    let (pad, chevron) = (pick(PAD.0, PAD.1), pick(CHEVRON.0, CHEVRON.1));
    let border = if spec.open { p.accent } else { p.input_border };
    let fill = if spec.hovered && spec.enabled && !spec.open {
        p.input_background.mix(p.text, 0.04)
    } else {
        p.input_background
    };
    layer.quad(Quad::filled(r, fill).rounded(px(radius::MD)).bordered(ctx.line(), border));
    let (font, small) = fonts(ctx, look.compact);
    // The note on the right, the value in what's left.
    let mut room = r.size.width - 2.0 * pad - chevron - px(PART_GAP);
    if let Some(note) = spec.note {
        let note = ellipsize(text, note, &small, (room * 0.5).max(0.0));
        let shaped = text.shape_line(&note, &small);
        let x = r.max_x() - px(12.0) - shaped.width;
        draw_text(
            layer,
            &shaped,
            x,
            optical_baseline(r.min_y(), r.size.height, &small),
            p.text_muted,
        );
        room = x - r.min_x() - px(22.0);
    }
    let mid = r.min_y() + r.size.height / 2.0;
    let mut x = r.min_x() + pad;
    if let Some(color) = look.dot {
        let d = px(DOT);
        let at = Rect::new(x, (mid - d / 2.0).round(), d, d);
        layer.quad(Quad::filled(at, ctx.fade(color)).rounded(d / 2.0));
        x += d + px(PART_GAP);
        room -= d + px(PART_GAP);
    }
    if let Some(name) = icon {
        let s = pick(ICON.0, ICON.1);
        if let Some(id) = icons.id(name) {
            let at = Rect::new(x, (mid - s / 2.0).round(), s, s);
            let color = if spec.enabled { p.text } else { p.text_muted };
            ui_kit::draw_icon(layer, icons, id, at, ctx.fade(color));
        }
        x += s + px(PART_GAP);
        room -= s + px(PART_GAP);
    }
    if let Some(prefix) = look.prefix {
        let shaped = text.shape_line(prefix, &small);
        let baseline = optical_baseline(r.min_y(), r.size.height, &small);
        draw_text(layer, &shaped, x, baseline, p.text_muted);
        x += shaped.width + px(PART_GAP);
        room -= shaped.width + px(PART_GAP);
    }
    let (value, empty) = match spec.placeholder {
        Some(placeholder) if spec.value.is_empty() => (placeholder, true),
        _ => (spec.value, false),
    };
    let shown = ellipsize(text, value, &font, room.max(0.0));
    let shaped = text.shape_line(&shown, &font);
    let color = match look.value_color {
        Some(color) if spec.enabled && !empty => color,
        _ if spec.enabled && !empty => p.text,
        _ => p.text_muted,
    };
    let baseline = optical_baseline(r.min_y(), r.size.height, &font);
    draw_text(layer, &shaped, x, baseline, color);
    if spec.enabled
        && let Some(id) = icons.id("chevron-down")
    {
        let at =
            Rect::new(r.max_x() - chevron - pad, (mid - chevron / 2.0).round(), chevron, chevron);
        ui_kit::draw_icon(layer, icons, id, at, look.value_color.unwrap_or(p.text_muted));
    }
}
