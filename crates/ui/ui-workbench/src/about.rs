//! The "About" window: the splash card (illustration, the app's name, version) over a dimmed
//! backdrop, with the app info in the bottom right. Closes with `esc` (the key or its button).

use std::time::Duration;

use kernel::{Point, Rect, Rgba};
use render_api::ImageData;
use text_layout::Font;
use ui_kit::draw_text;

use crate::Painter;
use crate::anim::Appear;
use crate::ds::{self, Ctx};
use crate::splash::Splash;

#[derive(Debug, Default)]
pub struct AboutView {
    open: bool,
    viewport: Rect,
    scale: f32,
    appear: Appear,
    /// The `esc` button (from the last paint): the only click that closes the window.
    esc: Rect,
}

impl AboutView {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.appear.start(Duration::from_millis(160));
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    /// Appearing (only while open: closed, it's gone at once and moves for nobody).
    pub fn is_animating(&self) -> bool {
        self.is_open() && self.appear.animating()
    }

    /// A click while open: only the `esc` button closes the window (the key does too); the
    /// click is consumed either way. `true` while open.
    pub fn click(&mut self, p: Point) -> bool {
        if !self.open {
            return false;
        }
        if self.esc.contains(p) {
            self.open = false;
        }
        true
    }

    /// `lines`: the info in the bottom right (e.g. description, platform, modules).
    pub fn paint(
        &mut self,
        version: &str,
        lines: &[String],
        art: Option<&ImageData>,
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, icons } = painter;
        if !self.open {
            return;
        }
        let t = self.appear.value();
        let ctx = Ctx::new(palette, self.scale).with_opacity(t);
        ds::backdrop(scene.push_layer(None), &ctx, self.viewport);
        // Rises slightly from below, fading in.
        let lift = ((1.0 - t) * 10.0 * self.scale).round();
        let bounds = Rect::new(
            self.viewport.min_x(),
            self.viewport.min_y() + lift,
            self.viewport.size.width,
            self.viewport.size.height,
        );
        let (card, chip) = Splash::paint_card(
            bounds,
            self.scale,
            t,
            version,
            art,
            Painter { text, scene, palette, icons },
        );
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(bounds));
        // The info, right-aligned, bottom to top (the last one on the version line).
        let font = Font::new(palette.ui_font.clone(), px(12.5));
        let pad = px(30.0);
        let mut y = (chip.min_y() + (chip.size.height + font.size * 0.72) / 2.0).round();
        for (i, line) in lines.iter().rev().enumerate() {
            // The first line (the description) stands out: semibold, full white.
            let first = i + 1 == lines.len();
            let line_font = if first { font.clone().with_weight(600) } else { font.clone() };
            let shaped = text.shape_line(line, &line_font);
            let alpha = if first { 1.0 } else { 0.78 };
            draw_text(
                layer,
                &shaped,
                (card.max_x() - pad - shaped.width).round(),
                y,
                ctx.fade(Rgba::WHITE.with_alpha(alpha)),
            );
            y -= px(19.0);
        }
        // `esc`, in the card's top right: a dark chip, readable over any illustration.
        let key_font = ds::overlay_key_font(&ctx);
        let esc = text.shape_line("esc", &key_font);
        let corner = Point::new(card.max_x() - px(14.0), card.min_y() + px(14.0));
        self.esc = ds::overlay_key(layer, &ctx, &esc, &key_font, corner);
    }
}
