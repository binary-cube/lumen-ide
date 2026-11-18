//! The startup screen: a wide card with the app illustration as background, the app's name
//! written large and widely spaced over it, the version, the status and an animated progress
//! bar on the bottom edge.
//!
//! The illustration comes from the app (a texture decoded once); without it, the card stays
//! a dark gradient background. The app decides how long it is shown and gives it the
//! animation phase and the opacity.

use kernel::{Rect, Rgba};
use render_api::{ImageData, ImageInstance, Quad};
use text_layout::Font;
use ui_kit::draw_text;

use crate::Painter;

/// The card background (the illustration is dark, regardless of the theme).

#[derive(Debug, Default)]
pub struct Splash;

/// Where and how the splash is painted: its area, the scale, the bar's phase, the opacity.
#[derive(Debug, Clone, Copy)]
pub struct SplashFrame {
    pub bounds: Rect,
    pub scale: f32,
    pub phase: f32,
    pub opacity: f32,
}

impl Splash {
    /// The card width (logical pixels) and its corner radius: the app prepares the
    /// illustration for them (cropped to the card's aspect ratio, corners rounded in pixels).
    pub const WIDTH: f32 = 600.0;
    pub const RADIUS: f32 = 12.0;
    /// The card's aspect ratio (width / height).
    pub const ASPECT: f32 = 1.45;
}

impl Splash {
    /// `phase` grows continuously over time (seconds) and animates the bar; `opacity` ∈ [0, 1]
    /// fades the whole screen at the end.
    pub fn paint(
        &self,
        frame: SplashFrame,
        version: &str,
        status: &str,
        art: Option<&ImageData>,
        painter: Painter<'_>,
    ) {
        let SplashFrame { bounds, scale, phase, opacity } = frame;
        let Painter { text, scene, palette, icons } = painter;
        let px = |v: f32| (v * scale).round();
        let fade = |c: Rgba| c.with_alpha(c.a * opacity);
        scene.push_layer(Some(bounds)).quad(Quad::filled(bounds, fade(palette.window_background)));
        let (card, chip) = Self::paint_card(
            bounds,
            scale,
            opacity,
            version,
            art,
            Painter { text, scene, palette, icons },
        );
        let layer = scene.push_layer(Some(bounds));
        let pad = px(30.0);
        let small = Font::new(palette.ui_font.clone(), px(13.0));
        let chip_h = chip.size.height;
        let radius = px(Self::RADIUS);

        // The status, in the bottom right.
        let shaped = text.shape_line(status, &small);
        draw_text(
            layer,
            &shaped,
            (card.max_x() - pad - shaped.width).round(),
            (chip.min_y() + (chip_h + small.size * 0.72) / 2.0).round(),
            fade(Rgba::WHITE.with_alpha(0.70)),
        );

        // Indeterminate progress bar on the card's bottom edge: a bright segment
        // that travels from left to right.
        let track_h = px(3.0);
        let inset = radius;
        let track = Rect::new(
            card.min_x() + inset,
            card.max_y() - track_h - px(10.0),
            card.size.width - 2.0 * inset,
            track_h,
        );
        layer.quad(Quad::filled(track, fade(Rgba::WHITE.with_alpha(0.08))).rounded(track_h / 2.0));
        let segment = (track.size.width * 0.22).round();
        let t = (phase * 0.7).fract();
        let eased = t * t * (3.0 - 2.0 * t);
        let sx = track.min_x() - segment + eased * (track.size.width + segment);
        let visible = Rect::new(
            sx.max(track.min_x()),
            track.min_y(),
            (sx + segment).min(track.max_x()) - sx.max(track.min_x()),
            track_h,
        );
        if visible.size.width > 0.0 {
            let glow = palette.accent.mix(Rgba::rgb(0x22d3ee), 0.35);
            layer.quad(Quad::filled(visible, fade(glow)).rounded(track_h / 2.0));
        }
    }

    /// The shared card (splash and "About"): the shadow, the illustration, the name and the
    /// version. Returns the card and the version pill (so the rest can align with them).
    pub(crate) fn paint_card(
        bounds: Rect,
        scale: f32,
        opacity: f32,
        version: &str,
        art: Option<&ImageData>,
        painter: Painter<'_>,
    ) -> (Rect, Rect) {
        let Painter { text, scene, palette, .. } = painter;
        let px = |v: f32| (v * scale).round();
        let fade = |c: Rgba| c.with_alpha(c.a * opacity);
        let layer = scene.push_layer(Some(bounds));
        // The card: the illustration's proportions (≈ 2.5 : 1), as large as fits.
        let aspect = art.map_or(Self::ASPECT, |a| a.width as f32 / a.height.max(1) as f32);
        let card_w = px(Self::WIDTH).min(bounds.size.width - px(48.0)).max(px(200.0));
        let card_h = (card_w / aspect).min(bounds.size.height - px(48.0)).round();
        let card = Rect::new(
            (bounds.min_x() + (bounds.size.width - card_w) / 2.0).round(),
            (bounds.min_y() + (bounds.size.height - card_h) / 2.0).round(),
            card_w,
            card_h,
        );
        let radius = px(Self::RADIUS);
        // Soft, wide shadow (many nearly transparent layers), with no visible edge.
        crate::ds::soft_shadow(layer, card, radius, scale, opacity);
        layer.quad(Quad::filled(card, fade(crate::ds::NIGHT)).rounded(radius));

        // The illustration, over the whole card.
        if let Some(art) = art {
            let images = scene.push_layer(Some(card));
            images.images.push(ImageInstance { image: art.clone(), bounds: card, opacity });
        }

        let layer = scene.push_layer(Some(bounds));
        // (The bottom gradient, for text readability, is already in the illustration.)
        // The app's name (`brand.toml`): capitals, large, widely letter-spaced, white;
        // below it, the version.
        let pad = px(30.0);
        let baseline = card.max_y() - pad - px(40.0);
        let mark = crate::ds::Wordmark {
            family: &palette.ui_font,
            size: px(40.0),
            tracking: px(12.0),
            accent: palette.accent,
            opacity,
        };
        crate::ds::wordmark(
            layer,
            text,
            &mark,
            kernel::brand::NAME,
            (card.min_x() + pad, baseline),
        );
        let chip = crate::ds::version_pill(
            layer,
            text,
            &palette.ui_font,
            version,
            (card.min_x() + pad, baseline + px(16.0)),
            (scale, opacity),
        );

        (card, chip)
    }
}

#[cfg(test)]
#[path = "../tests/unit/splash.rs"]
mod tests;
