//! The scenes Relax mode can show: each a [`RelaxScene`], listed in [`SCENES`] (a new one
//! is a file here and a row there). A scene lays out its geometry when its area, its
//! density or its colors change, never per frame; [`RelaxScene::update`] places it at a
//! time (pure math: the same time gives the same picture) into buffers it keeps, and
//! [`RelaxScene::paint`] only pushes what it placed. 3D scenes project their points on the
//! CPU ([`Camera`]) and draw them as round quads and glow images.

pub mod ribbon;
pub mod stars;
pub mod waves;

use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, ImageInstance, Layer, Quad, Scene};
use ui_components::Palette;

/// A scene's colors: its sky (top, bottom), the light near the horizon and four lights
/// (stars, dots, a ribbon…), warm or not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneColors {
    pub sky_top: Rgba,
    pub sky_bottom: Rgba,
    pub horizon: Rgba,
    pub lights: [Rgba; 4],
    /// A warm light (a sun).
    pub warm: Rgba,
    /// A dark sky (light ones drawn bright); else a pale one (lights drawn deep).
    pub dark: bool,
}

impl SceneColors {
    /// The colors of the theme in use: its background for the sky, its accent and its
    /// code's colors for the lights — Matrix's greens, Cyberpunk's neons, a light theme's
    /// pale sky with deeper lights.
    pub fn from_palette(p: &Palette) -> Self {
        let bg = p.window_background;
        let dark = luminance(bg) < 0.45;
        let (a, b, c) = match &p.syntax {
            Some(s) => (s.function, s.keyword, s.string),
            None => (p.terminal_ansi[4], p.terminal_ansi[5], p.terminal_ansi[6]),
        };
        let tune = |c: Rgba| {
            let (h, s, l) = c.to_hsl();
            let s = (s * 0.92).clamp(0.25, 0.95);
            let l = if dark { l.clamp(0.62, 0.82) } else { l.clamp(0.30, 0.48) };
            Rgba::from_hsl(h, s, l, 1.0)
        };
        let accent = tune(p.accent);
        let (sky_top, sky_bottom) = if dark {
            (bg.mix(Rgba::BLACK, 0.35), bg.mix(p.accent, 0.24))
        } else {
            (bg.mix(Rgba::WHITE, 0.35), bg.mix(p.accent, 0.14))
        };
        Self {
            sky_top: sky_top.with_alpha(1.0),
            sky_bottom: sky_bottom.with_alpha(1.0),
            horizon: sky_bottom.mix(accent, 0.35).with_alpha(1.0),
            lights: [accent, tune(a), tune(b), tune(c)],
            warm: tune(p.warning),
            dark,
        }
    }
}

/// Relative luminance (0: black, 1: white), roughly.
pub fn luminance(c: Rgba) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// A scene of Relax mode.
pub trait RelaxScene: Send {
    fn id(&self) -> &'static str;
    /// Its own colors (when the theme's aren't taken).
    fn own_colors(&self) -> SceneColors;
    /// Its area (physical pixels) and the scale: what depends on them laid out again.
    fn resize(&mut self, bounds: Rect, scale: f32);
    /// How much it shows (1: as designed; 0.5–1.5): its points made again.
    fn set_density(&mut self, density: f32);
    /// Its colors: its images made again.
    fn set_colors(&mut self, colors: &SceneColors);
    /// Places everything at time `t` (seconds of the scene's own clock).
    fn update(&mut self, t: f32);
    /// Pushes what [`Self::update`] placed, in layers of its own clipped to `clip` (its
    /// background first; within a layer quads go under images).
    fn paint(&self, scene: &mut Scene, clip: Rect);
    /// What it holds (bytes): its points, its images.
    fn memory(&self) -> usize;
}

/// A scene as listed: its id (saved in the settings), its name's key, whether it's 3D,
/// and how it's made.
#[derive(Debug, Clone, Copy)]
pub struct SceneInfo {
    pub id: &'static str,
    pub name_key: &'static str,
    pub three_d: bool,
    pub make: fn() -> Box<dyn RelaxScene>,
}

/// Every scene, in the settings' order.
pub const SCENES: [SceneInfo; 3] = [
    SceneInfo {
        id: stars::ID,
        name_key: "relax.scene_stars",
        three_d: true,
        make: || Box::new(stars::Stars::default()),
    },
    SceneInfo {
        id: waves::ID,
        name_key: "relax.scene_waves",
        three_d: true,
        make: || Box::new(waves::Waves::default()),
    },
    SceneInfo {
        id: ribbon::ID,
        name_key: "relax.scene_ribbon",
        three_d: false,
        make: || Box::new(ribbon::Ribbon::default()),
    },
];

/// The scene with `id`.
pub fn find(id: &str) -> Option<&'static SceneInfo> {
    SCENES.iter().find(|s| s.id == id)
}

/// The scene after `id` (the first after the last, or for an unknown id).
pub fn next_after(id: &str) -> &'static SceneInfo {
    let at = SCENES.iter().position(|s| s.id == id).map_or(0, |i| (i + 1) % SCENES.len());
    &SCENES[at]
}

/// The scene before `id`.
pub fn previous_before(id: &str) -> &'static SceneInfo {
    let at = SCENES.iter().position(|s| s.id == id).unwrap_or(0);
    &SCENES[(at + SCENES.len() - 1) % SCENES.len()]
}

/// A number in `0..1` that depends only on `i` and `salt` (the scenes' layouts are the
/// same every time).
pub fn rand(i: u32, salt: u32) -> f32 {
    let mut x = u64::from(i).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ u64::from(salt) << 32;
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// `0` at `a`, `1` at `b`, smooth between (either order).
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A pinhole camera: points in its own space (x right, y up, z ahead) onto the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub center: Point,
    /// The focal length (pixels).
    pub focal: f32,
    /// Nearer than this, nothing is drawn.
    pub near: f32,
}

impl Camera {
    /// Where `(x, y, z)` shows, and how much a unit there measures on screen; `None`:
    /// behind (or too near).
    pub fn project(&self, x: f32, y: f32, z: f32) -> Option<(f32, f32, f32)> {
        if z < self.near {
            return None;
        }
        let k = self.focal / z;
        Some((self.center.x + x * k, self.center.y - y * k, k))
    }
}

/// A round dot as a quad.
pub fn dot(layer: &mut Layer, x: f32, y: f32, d: f32, color: Rgba) {
    if color.a <= 0.004 {
        return;
    }
    let d = d.max(1.0);
    layer
        .quads
        .push(Quad::filled(Rect::new(x - d / 2.0, y - d / 2.0, d, d), color).rounded(d / 2.0));
}

/// An image centered at `(x, y)`, `w` × `h`.
pub fn sprite(layer: &mut Layer, image: &ImageData, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
    if opacity <= 0.004 {
        return;
    }
    layer.images.push(ImageInstance {
        image: image.clone(),
        bounds: Rect::new(x - w / 2.0, y - h / 2.0, w, h),
        opacity: opacity.min(1.0),
    });
}

#[cfg(test)]
#[path = "../../tests/unit/scene.rs"]
mod tests;
