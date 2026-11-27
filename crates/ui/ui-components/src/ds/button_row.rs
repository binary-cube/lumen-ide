//! A row of buttons that keeps every one of them: with their labels while they all fit, each
//! by its icon alone when they don't (its label then its tooltip and its name to a screen
//! reader), never one drawn over another. Those at the row's start go from its left, those
//! at its end from its right, each side in its order; a row too narrow even for their icons
//! cuts the last at its edge. A filled button (primary, danger) alone by its icon is round.

use super::*;

/// The room between two buttons of a row (logical pixels).
const ROW_GAP: f32 = 8.0;

/// A button of a [`button_row`]: how it looks with its label, and whether it goes at the
/// row's end.
#[derive(Debug, Clone, Copy)]
pub struct RowButton<'a> {
    /// The button as it looks with its label.
    pub spec: ButtonSpec<'a>,
    /// At the row's end (right-aligned), else at its start.
    pub end: bool,
}

impl<'a> RowButton<'a> {
    /// At the row's start.
    pub fn start(spec: ButtonSpec<'a>) -> Self {
        Self { spec, end: false }
    }

    /// At the row's end.
    pub fn end(spec: ButtonSpec<'a>) -> Self {
        Self { spec, end: true }
    }
}

/// Where a button of the row was drawn, and its label when only its icon shows (its
/// tooltip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedButton<'a> {
    /// Where it was drawn: its hit.
    pub rect: Rect,
    /// Its label, when only its icon shows: its tooltip.
    pub tip: Option<&'a str>,
}

/// `buttons` drawn in `row` (its height theirs): with their labels if they all fit, else by
/// their icons; in the same order, where each went.
pub fn button_row<'a>(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    row: Rect,
    buttons: &[RowButton<'a>],
) -> Vec<PlacedButton<'a>> {
    let h = row.size.height;
    let gap = ctx.px(ROW_GAP);
    let mut shown: Vec<(ButtonSpec<'a>, Option<&'a str>)> =
        buttons.iter().map(|b| (b.spec, None)).collect();
    let widths = |text: &mut TextSystem, shown: &[(ButtonSpec<'a>, Option<&'a str>)]| {
        shown.iter().map(|(spec, _)| button_width(text, ctx, spec, h)).collect::<Vec<f32>>()
    };
    let mut wide = widths(text, &shown);
    if row_width(&wide, gap) > row.size.width {
        for (spec, tip) in &mut shown {
            if spec.has_icon() && spec.label.is_some() {
                *tip = spec.label.take();
                spec.name = *tip;
                if matches!(spec.kind, ButtonKind::Primary | ButtonKind::Danger) {
                    spec.shape = ButtonShape::Round;
                }
            }
        }
        wide = widths(text, &shown);
    }
    let sides: Vec<(f32, bool)> = wide.iter().zip(buttons).map(|(w, b)| (*w, b.end)).collect();
    let xs = row_places((row.min_x(), row.max_x()), gap, &sides);
    shown
        .into_iter()
        .zip(wide)
        .zip(xs)
        .map(|(((spec, tip), w), x)| {
            let rect = Rect::new(x.round(), row.min_y(), w, h);
            button(layer, text, icons, ctx, rect, spec);
            PlacedButton { rect, tip }
        })
        .collect()
}

/// How wide `widths` are side by side, `gap` between each two.
pub(crate) fn row_width(widths: &[f32], gap: f32) -> f32 {
    widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32
}

/// Where each item (its width, whether at the end) starts between `min_x` and `max_x`: those
/// at the start from the left, those at the end ending at the right, each side in order.
/// Too narrow for all of them, the end ones follow the start ones instead (past `max_x`, cut
/// by the row's edge): never one over another.
pub(crate) fn row_places((min_x, max_x): (f32, f32), gap: f32, items: &[(f32, bool)]) -> Vec<f32> {
    let mut xs = vec![0.0; items.len()];
    let mut x = min_x;
    for (i, (w, _)) in items.iter().enumerate().filter(|(_, (_, end))| !end) {
        xs[i] = x;
        x += w + gap;
    }
    let ends: Vec<usize> = (0..items.len()).filter(|&i| items[i].1).collect();
    let ends_width = row_width(&ends.iter().map(|&i| items[i].0).collect::<Vec<f32>>(), gap);
    let mut x = (max_x - ends_width).max(x);
    for i in ends {
        xs[i] = x;
        x += items[i].0 + gap;
    }
    xs
}

#[cfg(test)]
#[path = "../../tests/unit/ds/button_row.rs"]
mod tests;
