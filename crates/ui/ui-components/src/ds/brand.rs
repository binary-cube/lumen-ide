//! The app's own look, shared by the "About" card, the splash and the calm screens' panes:
//! the night it sits on, a soft wide shadow with no visible edge, the name written in
//! widely spaced capitals with an accent shadow under each letter, the version on a pill.

use super::*;
use text_layout::FontFamily;

/// The deep night under the app's card and its panes of glass.
pub const NIGHT: Rgba = Rgba::rgb(0x0d0f24);

/// A soft, wide shadow under `rect` (rounded by `radius`): many nearly transparent layers,
/// a little lower than it, `strength` of the card's (1: the "About" card's).
pub fn soft_shadow(layer: &mut Layer, rect: Rect, radius: f32, scale: f32, strength: f32) {
    const STEPS: u32 = 16;
    let px = |v: f32| (v * scale).round();
    for i in (1..=STEPS).rev() {
        let grow = px(1.6 * i as f32);
        let shadow = Rect::new(
            rect.min_x() - grow,
            rect.min_y() - grow + px(6.0),
            rect.size.width + 2.0 * grow,
            rect.size.height + 2.0 * grow,
        );
        let alpha = 0.022 * strength * (1.0 - i as f32 / (STEPS as f32 + 1.0));
        layer.quad(Quad::filled(shadow, Rgba::BLACK.with_alpha(alpha)).rounded(radius + grow));
    }
}

/// How the name is written: its size and letters' spacing (physical pixels), the accent of
/// the shadow under each letter, how much of it shows.
#[derive(Debug, Clone, Copy)]
pub struct Wordmark<'a> {
    pub family: &'a FontFamily,
    pub size: f32,
    pub tracking: f32,
    pub accent: Rgba,
    pub opacity: f32,
}

impl Wordmark<'_> {
    fn font(&self) -> Font {
        Font::new(self.family.clone(), self.size).with_weight(Font::SEMIBOLD)
    }

    /// Its width (physical pixels): the letters and the spaces between them.
    pub fn width(&self, text: &mut TextSystem, name: &str) -> f32 {
        let font = self.font();
        let letters: Vec<f32> = name
            .to_uppercase()
            .chars()
            .map(|c| text.shape_line(&c.to_string(), &font).width)
            .collect();
        letters.iter().sum::<f32>() + self.tracking * letters.len().saturating_sub(1) as f32
    }
}

/// `name` in capitals, widely spaced, white, from `x` on `baseline`, a subtle accent shadow
/// under each letter. Returns where it ends.
pub fn wordmark(
    layer: &mut Layer,
    text: &mut TextSystem,
    mark: &Wordmark<'_>,
    name: &str,
    (x, baseline): (f32, f32),
) -> f32 {
    let font = mark.font();
    let unit = (mark.size / 40.0).max(0.5);
    let fade = |c: Rgba| c.with_alpha(c.a * mark.opacity);
    let start = x;
    let mut x = x;
    // Its letters (each twice: a shadow, then the letter) aren't read one by one: the name
    // is, once.
    let _quiet = ui_kit::access::silenced();
    for letter in name.to_uppercase().chars() {
        let shaped = text.shape_line(&letter.to_string(), &font);
        draw_text(
            layer,
            &shaped,
            x + unit,
            baseline + 2.0 * unit,
            fade(mark.accent.with_alpha(0.55)),
        );
        draw_text(layer, &shaped, x, baseline, fade(Rgba::WHITE));
        x += shaped.width + mark.tracking;
    }
    drop(_quiet);
    ui_kit::access::node(|| {
        let bounds = Rect::new(start, baseline - mark.size, (x - start).max(0.0), mark.size * 1.2);
        ui_kit::access::Described::new(ui_kit::access::Role::Label, name, bounds)
    });
    x - mark.tracking
}

/// `version` on a pill of glass at `(x, y)` (its top left corner), `scale`d, `opacity` of
/// it shown. Returns the pill.
pub fn version_pill(
    layer: &mut Layer,
    text: &mut TextSystem,
    family: &FontFamily,
    version: &str,
    (x, y): (f32, f32),
    (scale, opacity): (f32, f32),
) -> Rect {
    let px = |v: f32| (v * scale).round();
    let fade = |c: Rgba| c.with_alpha(c.a * opacity);
    let caption = Font::new(family.clone(), px(13.0)).with_weight(Font::SEMIBOLD);
    let tag = text.shape_line(version, &caption);
    let h = px(24.0);
    let pill = Rect::new(x, y, tag.width + px(20.0), h);
    layer.quad(
        Quad::filled(pill, fade(Rgba::WHITE.with_alpha(0.10)))
            .rounded(h / 2.0)
            .bordered(scale.round().max(1.0), fade(Rgba::WHITE.with_alpha(0.16))),
    );
    draw_text(
        layer,
        &tag,
        pill.min_x() + px(10.0),
        (pill.min_y() + (h + caption.size * 0.72) / 2.0).round(),
        fade(Rgba::WHITE.with_alpha(0.92)),
    );
    pill
}
