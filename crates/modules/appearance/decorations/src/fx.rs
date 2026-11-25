//! Plan 42's effects, drawn: each has its layout (worked out again only when the window's
//! geometry changes), its art (rasterized once per scale, kept, its pixels leaving for the
//! GPU) and a frame that only places what it has — allocating nothing.
//!
//! - **Matrix**: columns of characters falling (their heads bright, their trails fading),
//!   drawn as glyphs of the app's icon set (the module's `fx-…` icons): one pass for all,
//!   each in its own color and brightness.
//! - **Stars**: toward you from the middle (warp: faster and bigger as they come), or
//!   drifting and twinkling; small round quads.
//! - **Aurora**: three bands of light at the top, each one image a period wide repeated
//!   across the window and moving at its own pace, breathing slowly.
//! - **Fireflies**: glowing dots wandering on slow curves, pulsing in turn.
//! - **Bubbles**: rising with a sway, swelling and fading as they pop at the top.
//! - **Fireworks**: a rare burst of sparks (falling, fading), a flash at its middle.
//! - **Shooting stars**: a rare streak across the sky, a row of fading dots behind its head.
//! - **Waves**: two or three along the bottom edge, each an image a period wide, moving.
//! - **Neon**: a bright line round the window and its halo, pulsing or turning its hue —
//!   each side its own quad, so a frame redraws only the edges.
//! - **Constellation**: drifting points, the near ones joined by fine dotted lines.
//! - **Synthwave**: a perspective grid along the bottom (its converging lines one image,
//!   the lines coming toward you quads at whole pixels), its glowing horizon, its sun or a
//!   city's skyline on it.
//! - **Lightning**: a rare strike — the sky lit from the top (one small gradient stretched),
//!   flickering two or three times, a forked bolt of dots from the top; nothing drawn
//!   between strikes.
//! - **Raindrops**: drops on the glass, still a while, then sliding down faster and faster,
//!   a trail of small dots left behind them.
//! - **Clouds**: soft clouds (three shapes, at a quarter of the resolution, stretched)
//!   floating slowly along the top, the far ones smaller and fainter.
//! - **Fog**: one to three bands (each a period wide, at low resolution, stretched and
//!   repeated) drifting along the bottom, or veils across the window.
//! - **Butterflies**: a few, resting with their wings slowly opening and closing, then
//!   flying a short way, flapping (four frames per color).
//! - **Balloons**: rising and swaying, each string a row of thin quads swinging behind.
//! - **Hearts**: floating up, fading in and out, swelling twice a beat.
//! - **Bats**: crossing now and then on a wavy, jinking path, flapping (three frames).
//! - **Lava lamp**: blobs of wax (one small soft image per color, stretched) rising and
//!   sinking; where two of a color come near, a neck between them, so they seem to merge
//!   and part (the metaballs' look, without drawing the field every frame).
//! - **Retro CRT**: scanlines (one tile repeated down the zone) or a curved screen's shaded
//!   edges, and a rare glitch — never behind the editor.
//! - **Glitch** (plan 45): now and then, for a quarter of a second, bands (or short blocks)
//!   shifted and split in two colors, jumping 30 times a second; nothing drawn between
//!   glitches, never behind the editor.
//! - **Neon rain**: fine streaks (a few thin quads, their heads bright) falling, each
//!   splashing at the bottom — a widening reflection and a few drops jumping.
//! - **Holographic signs**: one to four vertical neon signs in the corners (kanji-like glyphs
//!   of the module's, a frame, a soft halo), steady, flickering now and then.

use kernel::{Point, Rect, Rgba};
use render_api::{GlyphInstance, GlyphKey, ImageData, ImageInstance, Layer, Quad};
use theming::decorations::{EffectSpec, LayerKind};
use ui_components::decorations::{STATIC_TIME, hash, hidden};

use crate::art;

pub use self::celebration::*;
pub use self::creatures::*;
pub use self::night::*;
pub use self::retro::*;
pub use self::weather::*;

mod celebration;
mod creatures;
mod night;
mod retro;
mod weather;

/// An image whose pixels leave memory once on the GPU.
fn image(canvas: art::Canvas) -> ImageData {
    let (w, h) = (canvas.width, canvas.height);
    ImageData::gpu_only(w, h, canvas.into_rgba8())
}

fn push(layer: &mut Layer, image: &ImageData, bounds: Rect, opacity: f32) {
    if opacity > 0.003 {
        layer.images.push(ImageInstance::new(image.clone(), bounds, opacity));
    }
}

/// A round dot as a quad (stars, sparks, constellation nodes).
fn dot(layer: &mut Layer, center: Point, d: f32, color: Rgba) {
    if color.a <= 0.003 {
        return;
    }
    let d = d.max(1.0);
    layer.quads.push(
        Quad::filled(Rect::new(center.x - d / 2.0, center.y - d / 2.0, d, d), color)
            .rounded(d / 2.0),
    );
}

/// What an effect draws in a frame: where, at what scale and time, what hides it (the
/// cards its zone keeps it behind), the app's icons (Matrix's characters).
#[derive(Debug, Clone, Copy)]
pub struct FxFrame<'a> {
    pub window: Rect,
    pub scale: f32,
    pub time: f32,
    pub covered: &'a [Rect],
    pub icons: Option<&'a icons::Icons>,
}

/// The most of each thing an effect holds, whatever the window's size.
pub const MAX_COLUMNS: usize = 220;
pub const MAX_STARS: usize = 260;
pub const MAX_FLIES: usize = 60;
pub const MAX_BUBBLES: usize = 70;
/// Sparks in a burst at full size.
pub const SPARKS: usize = 44;

/// The window's area in units of 1200 × 700 logical pixels (counts scale with it).
fn area(window: Rect, scale: f32) -> f32 {
    (window.size.width / scale) * (window.size.height / scale) / (1200.0 * 700.0)
}

// ---------------------------------------------------------------------------------------
// Matrix.

/// An effect's painter (its kind's).
#[derive(Debug)]
pub enum FxPainter {
    Matrix(MatrixPainter),
    Stars(StarPainter),
    Aurora(AuroraPainter),
    Fireflies(FlyPainter),
    Bubbles(BubblePainter),
    Fireworks(FireworkPainter),
    ShootingStars,
    Waves(WavePainter),
    Neon,
    Constellation(ConstellationPainter),
    Synthwave(SynthwavePainter),
    Lightning(LightningPainter),
    Raindrops(RaindropPainter),
    Clouds(CloudPainter),
    Fog(FogPainter),
    Butterflies(ButterflyPainter),
    Balloons(BalloonPainter),
    Hearts(HeartPainter),
    Bats(BatPainter),
    Lava(LavaPainter),
    Crt(CrtPainter),
    Glitch,
    NeonRain(NeonRainPainter),
    Holo(HoloPainter),
}

impl FxPainter {
    pub fn new(kind: LayerKind) -> Option<Self> {
        Some(match kind {
            LayerKind::Matrix => Self::Matrix(MatrixPainter::default()),
            LayerKind::Starfield => Self::Stars(StarPainter::default()),
            LayerKind::Aurora => Self::Aurora(AuroraPainter::default()),
            LayerKind::Fireflies => Self::Fireflies(FlyPainter::default()),
            LayerKind::Bubbles => Self::Bubbles(BubblePainter::default()),
            LayerKind::Fireworks => Self::Fireworks(FireworkPainter::default()),
            LayerKind::ShootingStars => Self::ShootingStars,
            LayerKind::Waves => Self::Waves(WavePainter::default()),
            LayerKind::Neon => Self::Neon,
            LayerKind::Constellation => Self::Constellation(ConstellationPainter::default()),
            LayerKind::Synthwave => Self::Synthwave(SynthwavePainter::default()),
            LayerKind::Lightning => Self::Lightning(LightningPainter::default()),
            LayerKind::Raindrops => Self::Raindrops(RaindropPainter::default()),
            LayerKind::Clouds => Self::Clouds(CloudPainter::default()),
            LayerKind::Fog => Self::Fog(FogPainter::default()),
            LayerKind::Butterflies => Self::Butterflies(ButterflyPainter::default()),
            LayerKind::Balloons => Self::Balloons(BalloonPainter::default()),
            LayerKind::Hearts => Self::Hearts(HeartPainter::default()),
            LayerKind::Bats => Self::Bats(BatPainter::default()),
            LayerKind::LavaLamp => Self::Lava(LavaPainter::default()),
            LayerKind::Crt => Self::Crt(CrtPainter::default()),
            LayerKind::Glitch => Self::Glitch,
            LayerKind::NeonRain => Self::NeonRain(NeonRainPainter::default()),
            LayerKind::HoloSigns => Self::Holo(HoloPainter::default()),
            _ => return None,
        })
    }

    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        match self {
            Self::Matrix(p) => p.sync_layout(e, window, scale),
            Self::Stars(p) => p.sync_layout(e, window, scale),
            Self::Fireflies(p) => p.sync_layout(e, window, scale),
            Self::Bubbles(p) => p.sync_layout(e, window, scale),
            Self::Constellation(p) => p.sync_layout(e, window, scale),
            Self::Synthwave(p) => p.sync_art(e, window, scale),
            Self::Raindrops(p) => p.sync_layout(e, window, scale),
            Self::Clouds(p) => p.sync_layout(e, window, scale),
            Self::Butterflies(p) => p.sync_layout(e, window, scale),
            Self::Balloons(p) => p.sync_layout(e, window, scale),
            Self::Hearts(p) => p.sync_layout(e, window, scale),
            Self::Bats(p) => p.sync_layout(e, window, scale),
            Self::Lava(p) => p.sync_layout(e, window, scale),
            Self::NeonRain(p) => p.sync_layout(e, window, scale),
            Self::Holo(p) => p.sync_layout(e),
            _ => {}
        }
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        match self {
            Self::Aurora(p) => p.sync_art(e, scale),
            Self::Fireflies(p) => p.sync_art(e, scale),
            Self::Bubbles(p) => p.sync_art(e, scale),
            Self::Fireworks(p) => p.sync_art(e, scale),
            Self::Waves(p) => p.sync_art(e, scale),
            Self::Lightning(p) => p.sync_art(e),
            Self::Raindrops(p) => p.sync_art(e, scale),
            Self::Clouds(p) => p.sync_art(e),
            Self::Fog(p) => p.sync_art(e),
            Self::Butterflies(p) => p.sync_art(e, scale),
            Self::Balloons(p) => p.sync_art(e, scale),
            Self::Hearts(p) => p.sync_art(e, scale),
            Self::Bats(p) => p.sync_art(e, scale),
            Self::Lava(p) => p.sync_art(e),
            Self::Crt(p) => p.sync_art(e, scale),
            Self::Holo(p) => p.sync_art(e),
            _ => {}
        }
    }

    pub fn paint(&mut self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        match self {
            Self::Matrix(p) => p.paint(e, f, layer),
            Self::Stars(p) => p.paint(e, f, layer),
            Self::Aurora(p) => p.paint(e, f, layer),
            Self::Fireflies(p) => p.paint(e, f, layer),
            Self::Bubbles(p) => p.paint(e, f, layer),
            Self::Fireworks(p) => p.paint(e, f, layer),
            Self::ShootingStars => paint_streaks(e, f, layer),
            Self::Waves(p) => p.paint(e, f, layer),
            Self::Neon => paint_neon(e, f, layer),
            Self::Constellation(p) => p.paint(e, f, layer),
            Self::Synthwave(p) => p.paint(e, f, layer),
            Self::Lightning(p) => p.paint(e, f, layer),
            Self::Raindrops(p) => p.paint(e, f, layer),
            Self::Clouds(p) => p.paint(e, f, layer),
            Self::Fog(p) => p.paint(e, f, layer),
            Self::Butterflies(p) => p.paint(e, f, layer),
            Self::Balloons(p) => p.paint(e, f, layer),
            Self::Hearts(p) => p.paint(e, f, layer),
            Self::Bats(p) => p.paint(e, f, layer),
            Self::Lava(p) => p.paint(e, f, layer),
            Self::Crt(p) => p.paint(e, f, layer),
            Self::Glitch => paint_glitch(e, f, layer),
            Self::NeonRain(p) => p.paint(e, f, layer),
            Self::Holo(p) => p.paint(e, f, layer),
        }
    }

    pub fn images(&self) -> Box<dyn Iterator<Item = &ImageData> + '_> {
        match self {
            Self::Aurora(p) => Box::new(p.images()),
            Self::Fireflies(p) => Box::new(p.images()),
            Self::Bubbles(p) => Box::new(p.images()),
            Self::Fireworks(p) => Box::new(p.images()),
            Self::Waves(p) => Box::new(p.images()),
            Self::Synthwave(p) => Box::new(p.images()),
            Self::Lightning(p) => Box::new(p.images()),
            Self::Raindrops(p) => Box::new(p.images()),
            Self::Clouds(p) => Box::new(p.images()),
            Self::Fog(p) => Box::new(p.images()),
            Self::Butterflies(p) => Box::new(p.images()),
            Self::Balloons(p) => Box::new(p.images()),
            Self::Hearts(p) => Box::new(p.images()),
            Self::Bats(p) => Box::new(p.images()),
            Self::Lava(p) => Box::new(p.images()),
            Self::Crt(p) => Box::new(p.images()),
            Self::Holo(p) => Box::new(p.images()),
            _ => Box::new(std::iter::empty()),
        }
    }

    /// The bytes its layout holds.
    pub fn memory(&self) -> usize {
        match self {
            Self::Matrix(p) => p.memory(),
            Self::Stars(p) => p.memory(),
            Self::Fireflies(p) => p.memory(),
            Self::Bubbles(p) => p.memory(),
            Self::Constellation(p) => p.memory(),
            Self::Raindrops(p) => p.memory(),
            Self::Clouds(p) => p.memory(),
            Self::Butterflies(p) => p.memory(),
            Self::Balloons(p) => p.memory(),
            Self::Hearts(p) => p.memory(),
            Self::Bats(p) => p.memory(),
            Self::Lava(p) => p.memory(),
            Self::NeonRain(p) => p.memory(),
            Self::Holo(p) => p.memory(),
            _ => 0,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/fx.rs"]
mod tests;
