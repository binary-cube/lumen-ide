//! **Stars** (3D): a field of stars you drift through, slowly — each comes from the far
//! depth (fading in), grows and brightens as it nears and leaves the screen; the whole
//! field turns very slowly about the line of sight and the camera sways a little (the near
//! stars shift more than the far ones). Behind them, three soft clouds of the theme's
//! colors wander. Every star's place is worked out on the CPU from its fixed position and
//! the time: nothing accumulates, the same time gives the same sky.

use std::f32::consts::TAU;

use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, Scene};

use super::{Camera, RelaxScene, SceneColors, dot, rand, smoothstep, sprite};
use crate::art;

pub const ID: &str = "stars";

/// Stars at density 1 (about a quarter of them on screen at once).
const BASE: f32 = 5200.0;
/// The nearest and the farthest depth.
const NEAR: f32 = 0.06;
const DEPTH: f32 = 1.0;
/// Depth crossed per second (a star takes about 45 s from far to near).
pub const FLIGHT: f32 = 0.022;
/// The field's turn about the line of sight (radians a second: once in about 10 min).
pub const ROLL: f32 = 0.01;

/// A star as laid out: its place in the field's disc, its starting depth (0..1), how
/// bright, its twinkle's phase and its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    pub x: f32,
    pub y: f32,
    pub z0: f32,
    pub magnitude: f32,
    pub twinkle: f32,
    pub color: u8,
}

/// A star as placed at a time, on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
    pub alpha: f32,
    pub color: u8,
}

#[derive(Debug, Default)]
pub struct Stars {
    bounds: Rect,
    scale: f32,
    density: f32,
    camera: Option<Camera>,
    stars: Vec<Star>,
    placed: Vec<Placed>,
    /// The clouds placed: center, size, opacity.
    clouds: [(f32, f32, f32, f32); 3],
    colors: Option<SceneColors>,
    tints: [Rgba; 4],
    sky: Option<ImageData>,
    glows: Vec<ImageData>,
    hazes: Vec<ImageData>,
}

/// The stars at density `density`, the same every time.
pub fn layout(density: f32) -> Vec<Star> {
    let n = (BASE * density.clamp(0.25, 2.0)) as u32;
    (0..n)
        .map(|i| {
            // Evenly over a disc (its radius covers the window's corners at the far depth).
            let r = rand(i, 1).sqrt().max(0.02);
            let a = rand(i, 2) * TAU;
            let pick = rand(i, 5);
            Star {
                x: r * a.cos(),
                y: r * a.sin(),
                z0: rand(i, 3),
                magnitude: 0.5 + 0.5 * rand(i, 4).powf(1.6),
                twinkle: rand(i, 6),
                color: if pick < 0.62 { 0 } else { 1 + ((pick - 0.62) / 0.38 * 3.0) as u8 % 3 },
            }
        })
        .collect()
}

/// Where star `s` is at time `t`: its depth (0: near, 1: far), and its place in the
/// camera's space (turned, the sway taken off).
pub fn star_at(s: &Star, t: f32) -> (f32, f32, f32) {
    let depth = (s.z0 - t * FLIGHT).rem_euclid(1.0);
    let (sin, cos) = (t * ROLL).sin_cos();
    let (sway_x, sway_y) = sway(t);
    let x = s.x * cos - s.y * sin - sway_x;
    let y = s.x * sin + s.y * cos - sway_y;
    (depth, x, y)
}

/// The camera's sway at `t`.
pub fn sway(t: f32) -> (f32, f32) {
    ((t * 0.043).sin() * 0.05, (t * 0.031 + 1.0).sin() * 0.035)
}

impl RelaxScene for Stars {
    fn id(&self) -> &'static str {
        ID
    }

    fn own_colors(&self) -> SceneColors {
        SceneColors {
            sky_top: Rgba::rgb(0x03040c),
            sky_bottom: Rgba::rgb(0x111735),
            horizon: Rgba::rgb(0x1d2552),
            lights: [
                Rgba::rgb(0xe4ecff),
                Rgba::rgb(0x6f96ff),
                Rgba::rgb(0xb08cff),
                Rgba::rgb(0x58d2e0),
            ],
            warm: Rgba::rgb(0xffd8a0),
            dark: true,
        }
    }

    fn resize(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
        let (w, h) = (bounds.size.width, bounds.size.height);
        self.camera = Some(Camera {
            center: Point::new(bounds.min_x() + w / 2.0, bounds.min_y() + h / 2.0),
            focal: 0.6 * w.max(h),
            near: NEAR * 0.5,
        });
    }

    fn set_density(&mut self, density: f32) {
        if self.density == density && !self.stars.is_empty() {
            return;
        }
        self.density = density;
        self.stars = layout(density);
        self.placed = Vec::with_capacity(self.stars.len());
    }

    fn set_colors(&mut self, colors: &SceneColors) {
        if self.colors.as_ref() == Some(colors) {
            return;
        }
        self.colors = Some(*colors);
        let [l0, l1, l2, l3] = colors.lights;
        // Most stars near white (on a pale sky, deep), the others the lights'.
        let plain = if colors.dark { l0.mix(Rgba::WHITE, 0.7) } else { l0.mix(Rgba::BLACK, 0.25) };
        self.tints = [plain, l1, l2, l3];
        self.sky = Some(art::gradient(colors.sky_top, colors.sky_bottom));
        self.glows = self.tints.iter().map(|c| art::glow(*c, 0.18)).collect();
        self.hazes = [l1, l2, l3].iter().map(|c| art::glow(*c, 0.0)).collect();
    }

    fn update(&mut self, t: f32) {
        let Some(camera) = self.camera else { return };
        let b = self.bounds;
        let margin = 24.0 * self.scale;
        let (min_x, max_x) = (b.min_x() - margin, b.max_x() + margin);
        let (min_y, max_y) = (b.min_y() - margin, b.max_y() + margin);
        self.placed.clear();
        let (sin, cos) = (t * ROLL).sin_cos();
        let (sway_x, sway_y) = sway(t);
        for s in &self.stars {
            let depth = (s.z0 - t * FLIGHT).rem_euclid(1.0);
            let z = NEAR + depth * DEPTH;
            let x = s.x * cos - s.y * sin - sway_x;
            let y = s.x * sin + s.y * cos - sway_y;
            let Some((sx, sy, _)) = camera.project(x, y, z) else { continue };
            if sx < min_x || sx > max_x || sy < min_y || sy > max_y {
                continue;
            }
            // Fading in far away, out right before they pass.
            let fade = (1.0 - smoothstep(0.72, 1.0, depth)) * smoothstep(0.0, 0.08, depth);
            let twinkle = 0.78 + 0.22 * (t * (1.1 + 0.8 * s.twinkle) + s.twinkle * TAU).sin();
            let alpha = s.magnitude * fade * twinkle;
            if alpha < 0.02 {
                continue;
            }
            let diameter = s.magnitude * self.scale * (1.5 + 0.6 / z);
            self.placed.push(Placed { x: sx, y: sy, diameter, alpha, color: s.color });
        }
        let (w, h) = (b.size.width, b.size.height);
        let c = Point::new(b.min_x() + w / 2.0, b.min_y() + h / 2.0);
        for (i, cloud) in self.clouds.iter_mut().enumerate() {
            let i = i as f32;
            *cloud = (
                c.x + (t * 0.013 + i * 2.1).sin() * 0.28 * w,
                c.y + (t * 0.011 + i * 1.3).cos() * 0.2 * h,
                (1.0 + 0.25 * i) * h,
                0.2 + 0.06 * (t * 0.05 + i).sin(),
            );
        }
    }

    fn paint(&self, scene: &mut Scene, clip: Rect) {
        let (Some(sky), Some(colors)) = (&self.sky, &self.colors) else { return };
        let back = scene.push_layer(Some(clip));
        back.image(sky, self.bounds);
        let haze = if colors.dark { 1.0 } else { 0.8 };
        for (image, (x, y, size, opacity)) in self.hazes.iter().zip(self.clouds) {
            sprite(back, image, x, y, size * 1.6, size, opacity * haze);
        }
        let dots = scene.push_layer(Some(clip));
        let glow_from = 2.4 * self.scale;
        for p in &self.placed {
            let tint = self.tints[usize::from(p.color)];
            dot(dots, p.x, p.y, p.diameter, tint.with_alpha(p.alpha));
        }
        let glows = scene.push_layer(Some(clip));
        for p in self.placed.iter().filter(|p| p.diameter > glow_from) {
            let size = p.diameter * 5.0;
            let image = &self.glows[usize::from(p.color)];
            sprite(glows, image, p.x, p.y, size, size, p.alpha * 0.7);
        }
    }

    fn memory(&self) -> usize {
        self.stars.capacity() * size_of::<Star>()
            + self.placed.capacity() * size_of::<Placed>()
            + self.sky.iter().chain(&self.glows).chain(&self.hazes).map(art::bytes).sum::<usize>()
    }
}

impl Stars {
    /// The stars placed at the last update.
    pub fn placed(&self) -> &[Placed] {
        &self.placed
    }
}
