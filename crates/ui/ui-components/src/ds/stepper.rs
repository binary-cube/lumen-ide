//! The stepper: a number with − and + beside it.

use super::*;

/// What [`stepper`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct StepperSpec<'a> {
    pub x: f32,
    pub width: f32,
    pub mid: f32,
    pub labels: &'a [&'a str],
    pub current: usize,
}

impl<'a> StepperSpec<'a> {
    /// `labels` across `width` from `x`, centered on `mid`, the first the current one.
    pub fn new((x, width): (f32, f32), mid: f32, labels: &'a [&'a str]) -> Self {
        Self { x, width, mid, labels, current: 0 }
    }

    /// The step under way (its place).
    pub fn current(self, current: usize) -> Self {
        Self { current, ..self }
    }
}

/// A step's number as written, the usual wizard's at most.
const STEP_NUMBERS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// A stepper's numbered circle, and the room between it and the lines (logical pixels).
const STEP: f32 = 22.0;
const STEP_GAP: f32 = 10.0;

/// A wizard's steps across `width` from `x`, centered on `mid`: each a numbered circle and
/// its name, joined by lines — the done ones filled with a check, the current one ringed in
/// the accent (its name bold), the next ones muted.
pub fn stepper(layer: &mut Layer, text: &mut TextSystem, ctx: &Ctx<'_>, spec: StepperSpec<'_>) {
    let StepperSpec { x, width, mid, labels, current } = spec;
    let p = ctx.palette;
    let font = ctx.ui_font(-0.5);
    let bold = font.clone().with_weight(Font::SEMIBOLD);
    let number = ctx.ui_font(-2.0).with_weight(Font::SEMIBOLD);
    let d = ctx.px(STEP);
    let gap = ctx.px(STEP_GAP);
    let shaped: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| text.shape_line(l, if i == current { &bold } else { &font }))
        .collect();
    let items: f32 = shaped.iter().map(|s| d + ctx.px(8.0) + s.width).sum();
    // The lines share what's left (none narrower than a short dash).
    let joins = labels.len().saturating_sub(1).max(1) as f32;
    let line_w = ((width - items - 2.0 * gap * joins) / joins).max(ctx.px(16.0));
    let mut cx = x;
    // To a screen reader: a list of the steps, the one under way chosen, those done checked.
    let all = Rect::new(x, (mid - d / 2.0).round(), width, d);
    let _steps = ui_kit::access::open(|| {
        ui_kit::access::Described::new(ui_kit::access::Role::List, "", all).count(labels.len())
    });
    for (i, label) in shaped.iter().enumerate() {
        let circle = Rect::new(cx.round(), (mid - d / 2.0).round(), d, d);
        let step = Rect::new(circle.min_x(), circle.min_y(), d + ctx.px(8.0) + label.width, d);
        let _step = ui_kit::access::open(|| {
            use ui_kit::access::{Described, Role};
            let name = labels.get(i).copied().unwrap_or_default();
            Described::new(Role::ListItem, name, step)
                .index(i)
                .selected(i == current)
                .checked(i < current)
        });
        let (fill, ring, color) = if i < current {
            (p.accent, p.accent, Rgba::WHITE)
        } else if i == current {
            (p.accent.with_alpha(0.08), p.accent, p.accent)
        } else {
            (Rgba::TRANSPARENT, p.border, p.text_muted)
        };
        if i == current {
            let halo = ctx.px(3.0);
            layer.quad(
                Quad::filled(
                    Rect::new(
                        circle.min_x() - halo,
                        circle.min_y() - halo,
                        d + 2.0 * halo,
                        d + 2.0 * halo,
                    ),
                    ctx.fade(p.accent.with_alpha(0.12)),
                )
                .rounded(d / 2.0 + halo),
            );
        }
        layer.quad(
            Quad::filled(circle, ctx.fade(fill))
                .rounded(d / 2.0)
                .bordered(ctx.px(1.5), ctx.fade(ring)),
        );
        // Its number from a table (no text made per frame); past it, written.
        let written;
        let mark = match STEP_NUMBERS.get(i) {
            _ if i < current => "✓",
            Some(n) => n,
            None => {
                written = (i + 1).to_string();
                written.as_str()
            }
        };
        let m = text.shape_line(mark, &number);
        draw_text(
            layer,
            &m,
            (circle.min_x() + (d - m.width) / 2.0).round(),
            optical_baseline(circle.min_y(), d, &number),
            ctx.fade(color),
        );
        let lx = circle.max_x() + ctx.px(8.0);
        let label_color = if i == current { p.text } else { p.text_muted };
        let f = if i == current { &bold } else { &font };
        draw_text(layer, label, lx, optical_baseline(circle.min_y(), d, f), ctx.fade(label_color));
        cx = lx + label.width;
        if i + 1 < shaped.len() {
            let line = Rect::new(cx + gap, mid.round(), line_w, ctx.px(1.5).max(1.0));
            let done = i < current;
            layer.quad(Quad::filled(line, ctx.fade(if done { p.accent } else { p.border })));
            cx = line.max_x() + gap;
        }
    }
}
