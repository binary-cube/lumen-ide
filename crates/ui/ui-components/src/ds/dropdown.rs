//! A list to choose from: the field showing the value chosen (a chevron on its right). Its
//! list — and the suggestions completing a text field — is the shared picker
//! ([`crate::picker::Picker`]).

use kernel::Rect;
use render_api::{Layer, Quad};
use text_layout::TextSystem;
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

/// The field of a list to choose from, in `r`.
pub fn dropdown(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    r: Rect,
    spec: DropdownSpec<'_>,
) {
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let border = if spec.open { p.accent } else { p.input_border };
    let fill = if spec.hovered && spec.enabled && !spec.open {
        p.input_background.mix(p.text, 0.04)
    } else {
        p.input_background
    };
    layer.quad(Quad::filled(r, fill).rounded(px(radius::MD)).bordered(ctx.line(), border));
    let font = ctx.ui_font(-0.5);
    // The note on the right, the value in what's left.
    let mut room = r.size.width - px(40.0);
    if let Some(note) = spec.note {
        let small = ctx.ui_font(-1.0);
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
    let (value, empty) = match spec.placeholder {
        Some(placeholder) if spec.value.is_empty() => (placeholder, true),
        _ => (spec.value, false),
    };
    let shown = ellipsize(text, value, &font, room.max(0.0));
    let shaped = text.shape_line(&shown, &font);
    let color = if spec.enabled && !empty { p.text } else { p.text_muted };
    let baseline = optical_baseline(r.min_y(), r.size.height, &font);
    draw_text(layer, &shaped, r.min_x() + px(10.0), baseline, color);
    if spec.enabled
        && let Some(id) = icons.id("chevron-down")
    {
        let s = px(14.0);
        let at = Rect::new(
            r.max_x() - s - px(10.0),
            (r.min_y() + (r.size.height - s) / 2.0).round(),
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, at, p.text_muted);
    }
}
