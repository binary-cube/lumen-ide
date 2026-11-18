//! Parameter help while typing a call: the callee's signature above the cursor, with the
//! parameter being typed highlighted. It follows the cursor (computed at paint time).

use std::ops::Range;
use std::time::Duration;

use kernel::Rect;
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{centered_baseline, draw_text};

use crate::anim::Tween;
use crate::ds::{self, Ctx};
use crate::palette::Palette;

/// A signature split into parameters (ranges in `code`), with the one being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamHintCard {
    pub code: String,
    pub params: Vec<Range<usize>>,
    pub active: Option<usize>,
}

#[derive(Debug)]
struct Shown {
    /// The call's `(` (document offset): the same call keeps the card.
    open: usize,
    card: ParamHintCard,
    appear: Tween,
}

#[derive(Debug, Default)]
pub struct ParamHint {
    shown: Option<Shown>,
}

impl ParamHint {
    /// The call (its `(`) whose signature is shown.
    pub fn open_at(&self) -> Option<usize> {
        self.shown.as_ref().map(|s| s.open)
    }

    pub fn card(&self) -> Option<&ParamHintCard> {
        self.shown.as_ref().map(|s| &s.card)
    }

    pub fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    /// Shows the card for the call at `open` (keeps the animation if it's the same call).
    pub fn show(&mut self, open: usize, card: ParamHintCard) {
        if let Some(shown) = self.shown.as_mut().filter(|s| s.open == open) {
            shown.card = card;
            return;
        }
        let mut appear = Tween::new(0.0, Duration::from_millis(100));
        appear.retarget(1.0);
        self.shown = Some(Shown { open, card, appear });
    }

    /// Another argument of the same call is being typed.
    pub fn set_active(&mut self, active: Option<usize>) -> bool {
        match &mut self.shown {
            Some(s) if s.card.active != active => {
                s.card.active = active;
                true
            }
            _ => false,
        }
    }

    pub fn hide(&mut self) -> bool {
        self.shown.take().is_some()
    }

    pub fn is_animating(&self) -> bool {
        self.shown.as_ref().is_some_and(|s| s.appear.animating())
    }

    /// Draws the card above the `caret` line (below it if there is no room), within `bounds`.
    pub fn paint(
        &mut self,
        caret: Rect,
        bounds: Rect,
        scale: f32,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let Some(shown) = &self.shown else { return };
        if !bounds.contains(caret.origin) {
            return;
        }
        let px = |v: f32| (v * scale).round();
        let font = Font::new(palette.code_font.clone(), px(palette.code_font_size - 0.5));
        let metrics = text.metrics(&font);
        let pad = px(10.0);
        let max_w = (bounds.size.width * 0.8).min(px(720.0)) - 2.0 * pad;
        let line_h = (metrics.ascent + metrics.descent + px(5.0)).round();

        // The code in pieces (between and on the parameters), laid out in lines that break
        // only before a parameter.
        let card = &shown.card;
        let mut pieces: Vec<(Range<usize>, bool)> = Vec::new();
        let mut at = 0;
        for (i, p) in card.params.iter().enumerate() {
            pieces.push((at..p.start, false));
            pieces.push((p.clone(), card.active == Some(i)));
            at = p.end;
        }
        pieces.push((at..card.code.len(), false));
        let mut lines: Vec<Vec<(f32, std::sync::Arc<text_layout::ShapedLine>, bool)>> =
            vec![Vec::new()];
        let mut x = 0.0;
        let mut widest: f32 = 0.0;
        for (range, active) in pieces {
            let piece = &card.code[range.clone()];
            if piece.is_empty() {
                continue;
            }
            let shaped = text.shape_line(piece, &font);
            // A parameter that doesn't fit goes on a new, indented line.
            if x > 0.0 && x + shaped.width > max_w && card.params.contains(&range) {
                lines.push(Vec::new());
                x = px(16.0);
            }
            let w = shaped.width;
            if let Some(line) = lines.last_mut() {
                line.push((x, shaped, active));
            }
            x += w;
            widest = widest.max(x);
        }
        let w = (widest + 2.0 * pad).round();
        let h = (2.0 * pad + line_h * lines.len() as f32 - px(5.0)).round();
        let left = caret.min_x().min(bounds.max_x() - w - px(8.0)).max(bounds.min_x() + px(4.0));
        let mut top = caret.min_y() - h - px(4.0);
        if top < bounds.min_y() {
            top = caret.max_y() + px(4.0);
        }
        let rect = Rect::new(left.round(), top.round(), w, h);
        let alpha = shown.appear.value();
        let ctx = Ctx::new(palette, scale).with_opacity(alpha);
        let layer = scene.push_layer(None);
        ds::surface(layer, &ctx, rect, ds::Elevation::Low, ds::radius::MD);
        let mut y = rect.min_y() + pad;
        for line in &lines {
            let baseline = centered_baseline(y, line_h - px(5.0), &metrics);
            for (dx, shaped, active) in line {
                let x = rect.min_x() + pad + dx;
                let color = if *active {
                    // The parameter being typed: accent text on a soft highlight.
                    let bg = Rect::new(
                        x - px(2.0),
                        y - px(1.0),
                        shaped.width + px(4.0),
                        line_h - px(3.0),
                    );
                    layer.quad(
                        Quad::filled(bg, palette.accent.with_alpha(0.14 * alpha)).rounded(px(3.0)),
                    );
                    palette.accent
                } else {
                    palette.text_muted
                };
                draw_text(layer, shaped, x, baseline, color.with_alpha(alpha));
            }
            y += line_h;
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/param_hint.rs"]
mod tests;
