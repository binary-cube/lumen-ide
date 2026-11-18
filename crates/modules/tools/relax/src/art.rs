//! The scenes' images, made once per colors (never per frame): a soft round glow, a
//! vertical gradient. Small, kept in memory (the software renderer reads them too).

use kernel::Rgba;
use render_api::ImageData;

/// A glow's side (pixels): stretched to any size, its falloff smooth enough at that.
pub const GLOW: u32 = 64;

/// A round glow of `color`: a bright core fading softly to nothing at the edge. `core`
/// (0..1) is how much of it is the bright middle.
pub fn glow(color: Rgba, core: f32) -> ImageData {
    let mut pixels = Vec::with_capacity((GLOW * GLOW * 4) as usize);
    let half = GLOW as f32 / 2.0;
    let [r, g, b, _] = color.to_rgba8();
    for y in 0..GLOW {
        for x in 0..GLOW {
            let dx = (x as f32 + 0.5 - half) / half;
            let dy = (y as f32 + 0.5 - half) / half;
            let d = (dx * dx + dy * dy).sqrt().min(1.0);
            // A gaussian halo, zero at the edge, with a brighter core.
            let halo = (-(d * d) * 5.0).exp() - (-5.0f32).exp();
            let inner = (1.0 - d / core.max(0.01)).max(0.0);
            let a = (halo * 0.75 + inner * inner * 0.25).clamp(0.0, 1.0);
            pixels.extend_from_slice(&[r, g, b, (a * 255.0).round() as u8]);
        }
    }
    ImageData::new(GLOW, GLOW, pixels)
}

/// A vertical gradient from `top` to `bottom` (stretched over its area), eased so the
/// middle doesn't band as much.
pub fn gradient(top: Rgba, bottom: Rgba) -> ImageData {
    const H: u32 = 256;
    let mut pixels = Vec::with_capacity((H * 2 * 4) as usize);
    for y in 0..H {
        let t = y as f32 / (H - 1) as f32;
        let t = t * t * (3.0 - 2.0 * t);
        let c = top.mix(bottom, t).to_rgba8();
        pixels.extend_from_slice(&c);
        pixels.extend_from_slice(&c);
    }
    ImageData::new(2, H, pixels)
}

/// The bytes an image holds in memory.
pub fn bytes(image: &ImageData) -> usize {
    image.memory()
}
