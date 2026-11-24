//! Choices: the pills (with a dot), a choice's cell, the row with a ✓.

use super::*;

/// What [`choice`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct ChoiceSpec<'a> {
    pub x: f32,
    pub mid: f32,
    pub label: &'a str,
    pub selected: bool,
    pub hovered: bool,
}

impl<'a> ChoiceSpec<'a> {
    /// `label` at `x`, centered on `mid`: not chosen, not under the mouse.
    pub fn new(x: f32, mid: f32, label: &'a str) -> Self {
        Self { x, mid, label, selected: false, hovered: false }
    }

    /// The one in force.
    pub fn selected(self, selected: bool) -> Self {
        Self { selected, ..self }
    }

    pub fn hovered(self, hovered: bool) -> Self {
        Self { hovered, ..self }
    }
}

/// A [`choice`] pill's width for `label` (to lay pills out before drawing them).
pub fn choice_width(text: &mut TextSystem, ctx: &Ctx<'_>, label: &str) -> f32 {
    (text.shape_line(label, &ctx.ui_font(0.0)).width + ctx.px(2.0 * CHOICE_PAD)).round()
}

/// An option in a choice group (pill), vertically centered on `mid`; the selected one
/// is in the accent color. Returns its area.
pub fn choice(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ChoiceSpec<'_>,
) -> Rect {
    paint_choice(layer, text, ctx, spec, None)
}

/// A choice pill's leading dot, and the room after it (logical pixels).
/// A choice pill's height, and the room on each side of its content (logical pixels).
const CHOICE_H: f32 = 32.0;
const CHOICE_PAD: f32 = 14.0;
/// Its dot's side.
const CHOICE_DOT: f32 = 7.0;
const CHOICE_DOT_GAP: f32 = 6.0;

/// [`choice_width`] of a [`choice_with_dot`] pill: its dot's room counted.
pub fn choice_width_with_dot(text: &mut TextSystem, ctx: &Ctx<'_>, label: &str) -> f32 {
    choice_width(text, ctx, label) + ctx.px(CHOICE_DOT + CHOICE_DOT_GAP)
}

/// [`choice`] with a color dot before its label (a severity, a status).
pub fn choice_with_dot(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ChoiceSpec<'_>,
    dot: Rgba,
) -> Rect {
    paint_choice(layer, text, ctx, spec, Some(dot))
}

fn paint_choice(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: ChoiceSpec<'_>,
    dot: Option<Rgba>,
) -> Rect {
    let ChoiceSpec { x, mid, label, selected, hovered } = spec;
    let font = ctx.ui_font(0.0);
    let shaped = text.shape_line(label, &font);
    let h = ctx.px(CHOICE_H);
    let lead = dot.map_or(0.0, |_| ctx.px(CHOICE_DOT + CHOICE_DOT_GAP));
    let rect = Rect::new(
        x,
        (mid - h / 2.0).round(),
        (shaped.width + lead + ctx.px(2.0 * CHOICE_PAD)).round(),
        h,
    );
    // To a screen reader: one of a group's choices (a radio), chosen or not.
    let _told = ui_kit::access::open(|| {
        use ui_kit::access::{Described, Role};
        Described::new(Role::Radio, label, rect).checked(selected)
    });
    let p = ctx.palette;
    let (fill, border, color) = if selected {
        (p.accent.with_alpha(0.10), p.accent.with_alpha(0.7), p.accent)
    } else if hovered {
        (p.text.with_alpha(0.05), p.border, p.text)
    } else {
        (Rgba::TRANSPARENT, p.border, p.text)
    };
    layer.quad(
        Quad::filled(rect, ctx.fade(fill))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(border)),
    );
    if let Some(dot) = dot {
        let d = ctx.px(CHOICE_DOT);
        let at = Rect::new(rect.min_x() + ctx.px(CHOICE_PAD), (mid - d / 2.0).round(), d, d);
        layer.quad(Quad::filled(at, ctx.fade(dot)).rounded(d / 2.0));
    }
    draw_text(
        layer,
        &shaped,
        rect.min_x() + ctx.px(CHOICE_PAD) + lead,
        optical_baseline(rect.min_y(), h, &font),
        ctx.fade(color),
    );
    rect
}

/// A choice laid in a grid of equal cells (versions, levels), as [`choice_cell`] draws it.
#[derive(Debug, Clone, Copy)]
pub struct ChoiceCell<'a> {
    pub label: &'a str,
    /// The one in force: filled in the accent, its label bold and white.
    pub selected: bool,
    pub hovered: bool,
    /// Under the keys: lit as under the mouse.
    pub active: bool,
}

/// The room kept on each side of a [`choice_cell`]'s label (logical pixels).
const CHOICE_CELL_PAD: f32 = 7.0;

/// A choice filling `rect` (a cell of a grid of them), its label centered and cut with "…":
/// the one in force filled in the accent, one under the mouse or the keys tinted.
pub fn choice_cell(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    cell: ChoiceCell<'_>,
) {
    let ChoiceCell { label, selected, hovered, active } = cell;
    let _told = ui_kit::access::open(|| {
        use ui_kit::access::{Described, Role};
        Described::new(Role::Radio, label, rect).checked(selected)
    });
    let p = ctx.palette;
    let (fill, border) = if selected {
        (p.accent, p.accent)
    } else if active || hovered {
        (p.accent.with_alpha(0.10), p.accent.with_alpha(0.6))
    } else {
        (p.text.with_alpha(0.035), p.border)
    };
    layer.quad(
        Quad::filled(rect, ctx.fade(fill))
            .rounded(ctx.px(radius::MD))
            .bordered(ctx.line(), ctx.fade(border)),
    );
    let font = ctx.ui_font(-0.5);
    let font = if selected { font.with_weight(Font::SEMIBOLD) } else { font };
    let room = (rect.size.width - 2.0 * ctx.px(CHOICE_CELL_PAD)).max(0.0);
    let w = ellipsized_width(text, label, &font, room);
    let at = (
        (rect.min_x() + (rect.size.width - w) / 2.0).round(),
        optical_baseline(rect.min_y(), rect.size.height, &font),
    );
    let color = ctx.fade(if selected { Rgba::WHITE } else { p.text });
    draw_ellipsized(layer, text, label, &font, at, room, color);
}

/// What [`check_row`] draws: its label and state, and, for a list that differs, its
/// corners, its hover's fill, its ✓'s size, the chosen one untinted.
#[derive(Debug, Clone, Copy)]
pub struct CheckRowSpec<'a> {
    pub label: &'a str,
    /// The one in force: tinted in the accent, a ✓ before its label.
    pub checked: bool,
    pub hovered: bool,
    /// The row under the keys: lit as under the mouse.
    pub active: bool,
    /// The highlight's corners (logical pixels).
    pub radius: f32,
    /// The hover's fill; `None`: the text, faint.
    pub hover_fill: Option<Rgba>,
    /// The ✓'s side (logical pixels).
    pub icon: f32,
    /// The chosen one tinted in the accent (else only its ✓ and label say it; the hover
    /// shows over it).
    pub tinted: bool,
    /// The chosen one's label in the accent (else in the text's color: only its ✓ says it).
    pub label_tinted: bool,
    /// The room kept at the row's right (logical pixels: a note drawn there), the label
    /// ellipsized before it; `None`: the label drawn whole.
    pub trailing: Option<f32>,
}

impl<'a> CheckRowSpec<'a> {
    /// The usual row: `checked` tinted, `hovered` lit.
    pub fn new(label: &'a str, checked: bool, hovered: bool) -> Self {
        Self {
            label,
            checked,
            hovered,
            active: false,
            radius: radius::MD,
            hover_fill: None,
            icon: CHECK_ICON,
            tinted: true,
            label_tinted: true,
            trailing: None,
        }
    }

    /// Under the keys.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// The hover's fill (a token: `hover_background`).
    pub fn hover_fill(mut self, fill: Rgba) -> Self {
        self.hover_fill = Some(fill);
        self
    }

    /// The ✓'s side (logical pixels).
    pub fn icon(mut self, side: f32) -> Self {
        self.icon = side;
        self
    }

    /// The chosen one not tinted.
    pub fn untinted(mut self) -> Self {
        self.tinted = false;
        self
    }

    /// The chosen one's label in the text's color (a menu's: only its ✓ says it).
    pub fn plain_label(mut self) -> Self {
        self.label_tinted = false;
        self
    }

    /// `trailing` logical pixels kept at the row's right, the label ellipsized before them.
    pub fn trailing(mut self, trailing: f32) -> Self {
        self.trailing = Some(trailing.max(0.0));
        self
    }
}

/// A check row's ✓, the room before it and its label's start (logical pixels), and its
/// fills' alphas (the chosen one's accent, the hover's text).
const CHECK_ICON: f32 = 14.0;
const CHECK_INSET: f32 = 8.0;
const CHECK_LABEL_X: f32 = 30.0;
const CHECK_TINT: f32 = 0.14;
const CHECK_HOVER: f32 = 0.06;

/// One of a list's choices as a row in `rect` (a calendar's presets): its label from the
/// left after a ✓'s gutter, the chosen one tinted in the accent with its ✓, the one under
/// the mouse lit.
pub fn check_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: CheckRowSpec<'_>,
) {
    // A choice to a screen reader: named by its label, checked when it's the one chosen.
    let described = || {
        use ui_kit::access::{Described, Role};
        Described::new(Role::ListItem, spec.label, rect)
            .selected(spec.checked)
            .checked(spec.checked)
    };
    ui_kit::access::group(described, || paint_check_row(layer, text, icons, ctx, rect, spec));
}

fn paint_check_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: CheckRowSpec<'_>,
) {
    let p = ctx.palette;
    let h = rect.size.height;
    let r = ctx.px(spec.radius);
    if spec.checked && spec.tinted {
        layer.quad(Quad::filled(rect, ctx.fade(p.accent.with_alpha(CHECK_TINT))).rounded(r));
    } else if spec.hovered || spec.active {
        let fill = spec.hover_fill.unwrap_or(p.text.with_alpha(CHECK_HOVER));
        layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(r));
    }
    let s = ctx.px(spec.icon);
    if spec.checked
        && let Some(id) = icons.id("check")
    {
        let at = Rect::new(
            rect.min_x() + ctx.px(CHECK_INSET),
            (rect.min_y() + (h - s) / 2.0).round(),
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, at, ctx.fade(p.accent));
    }
    let font = ctx.ui_font(-0.5);
    let x = rect.min_x() + ctx.px(CHECK_LABEL_X);
    let base = optical_baseline(rect.min_y(), h, &font);
    let color = ctx.fade(if spec.checked && spec.label_tinted { p.accent } else { p.text });
    match spec.trailing {
        Some(trailing) => {
            let room = (rect.max_x() - x - ctx.px(trailing)).max(0.0);
            super::draw_ellipsized(layer, text, spec.label, &font, (x, base), room, color);
        }
        None => draw_text(layer, &text.shape_line(spec.label, &font), x, base, color),
    }
}
