//! **Waves of light** (3D): a sea of glowing dots under a dusk sky, its swells rolling
//! toward you, a low sun (or moon) on the horizon. The dots are a lattice fixed to the
//! camera — rows spaced geometrically in depth, each as wide as the view there, so they're
//! evenly spread on screen — lifted by three travelling sine swells; the crests are the
//! brightest, the far rows fade into the haze. Projected on the CPU with a camera above
//! the water looking slightly down.

use std::f32::consts::TAU;

use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, Quad, Scene};

use super::{Camera, RelaxScene, SceneColors, dot, rand, smoothstep, sprite};
use crate::art;

pub const ID: &str = "waves";

/// The lattice at density 1: rows (in depth) and columns.
const ROWS: f32 = 60.0;
const COLUMNS: f32 = 120.0;
/// The nearest and the farthest row's depth (world units).
const NEAR: f32 = 3.0;
const FAR: f32 = 24.0;
/// The camera's height over the water.
const HEIGHT: f32 = 3.0;
/// How fast the swells come (world units a second).
pub const FLOW: f32 = 0.9;
/// The horizon's place (from the top, share of the height).
const HORIZON: f32 = 0.38;
/// The swells: amplitude, wave numbers across and along, angular speed, phase.
pub const SWELLS: [(f32, f32, f32, f32, f32); 3] =
    [(0.34, 0.30, 0.95, 0.55, 0.0), (0.18, -0.70, 1.20, 0.80, 1.7), (0.08, 1.90, 2.40, 1.30, 4.1)];
/// The highest a crest goes (all swells together).
const CREST: f32 = 0.60;
const SKY_STARS: u32 = 90;

/// A dot placed at a time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
    pub color: Rgba,
}

#[derive(Debug, Default)]
pub struct Waves {
    bounds: Rect,
    scale: f32,
    density: f32,
    camera: Option<Camera>,
    /// The camera's pitch (sine, cosine).
    pitch: (f32, f32),
    /// The view's half-width at depth 1 (with a margin).
    reach: f32,
    /// The columns' spacing (a share of the reach).
    step: f32,
    rows: Vec<f32>,
    columns: Vec<f32>,
    /// A row's heights (kept for its memory).
    heights: Vec<f32>,
    placed: Vec<Placed>,
    colors: Option<SceneColors>,
    sky: Option<ImageData>,
    sun: Option<ImageData>,
    haze: Option<ImageData>,
    water: Option<ImageData>,
    /// The sky's stars: place (share of the area above the horizon), size, phase.
    stars: Vec<(f32, f32, f32, f32)>,
}

/// The water's height at `(x, z)` (world units) and time `t`.
pub fn height(x: f32, z: f32, t: f32) -> f32 {
    let x = x + (t * 0.021).sin() * 2.0;
    let z = z + t * FLOW;
    SWELLS.iter().map(|(a, kx, kz, w, p)| a * (kx * x + kz * z + w * t + p).sin()).sum()
}

/// The heights along a row, as [`height`] gives them, at `n` points from `x0` every `dx`
/// (world units), at depth `z` and time `t`, into `out`. Each swell's phase grows by the
/// same step from a point to the next: its sine is turned on by that step (a rotation)
/// instead of computed again — three sines a row, not four a point.
pub fn row_heights(x0: f32, dx: f32, n: usize, z: f32, t: f32, out: &mut Vec<f32>) {
    out.clear();
    out.resize(n, 0.0);
    let x0 = x0 + (t * 0.021).sin() * 2.0;
    let z = z + t * FLOW;
    for (a, kx, kz, w, p) in SWELLS {
        let (mut s, mut c) = (kx * x0 + kz * z + w * t + p).sin_cos();
        let (ds, dc) = (kx * dx).sin_cos();
        for h in out.iter_mut() {
            *h += a * s;
            (s, c) = (s * dc + c * ds, c * dc - s * ds);
        }
    }
}

impl Waves {
    fn horizon(&self) -> f32 {
        self.bounds.min_y() + HORIZON * self.bounds.size.height
    }

    /// The dots placed at the last update.
    pub fn placed(&self) -> &[Placed] {
        &self.placed
    }
}

impl RelaxScene for Waves {
    fn id(&self) -> &'static str {
        ID
    }

    fn own_colors(&self) -> SceneColors {
        SceneColors {
            sky_top: Rgba::rgb(0x0a0720),
            sky_bottom: Rgba::rgb(0x3b1d4f),
            horizon: Rgba::rgb(0x8a3f7a),
            lights: [
                Rgba::rgb(0x5fe6ff),
                Rgba::rgb(0xff6fd8),
                Rgba::rgb(0xffb070),
                Rgba::rgb(0xa58bff),
            ],
            warm: Rgba::rgb(0xffb070),
            dark: true,
        }
    }

    fn resize(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
        let (w, h) = (bounds.size.width, bounds.size.height);
        let focal = 0.55 * w.max(h * 1.4);
        let center = Point::new(bounds.min_x() + w / 2.0, bounds.min_y() + h / 2.0);
        // Pitched down so the horizon sits at its place.
        let tan = (center.y - self.horizon()) / focal;
        let angle = tan.atan();
        self.pitch = angle.sin_cos();
        self.reach = 1.08 * (w / 2.0) / focal;
        self.camera = Some(Camera { center, focal, near: 0.3 });
    }

    fn set_density(&mut self, density: f32) {
        if self.density == density && !self.rows.is_empty() {
            return;
        }
        self.density = density;
        let k = density.clamp(0.25, 2.0).sqrt();
        let (rows, columns) = ((ROWS * k) as usize, (COLUMNS * k) as usize);
        let ratio = FAR / NEAR;
        self.rows = (0..rows).map(|i| NEAR * ratio.powf(i as f32 / (rows - 1) as f32)).collect();
        self.columns = (0..columns).map(|j| j as f32 / (columns - 1) as f32 * 2.0 - 1.0).collect();
        self.step = 2.0 / (columns - 1) as f32;
        self.placed = Vec::with_capacity(rows * columns);
        self.stars = (0..SKY_STARS)
            .map(|i| (rand(i, 11), rand(i, 12).powf(1.4), 0.6 + rand(i, 13), rand(i, 14) * TAU))
            .collect();
    }

    fn set_colors(&mut self, colors: &SceneColors) {
        if self.colors.as_ref() == Some(colors) {
            return;
        }
        self.colors = Some(*colors);
        self.sky = Some(art::gradient(colors.sky_top, colors.sky_bottom.mix(colors.horizon, 0.5)));
        self.sun = Some(art::glow(colors.warm.mix(colors.lights[2], 0.4), 0.12));
        self.haze = Some(art::glow(colors.horizon.mix(colors.lights[1], 0.3), 0.0));
        let deep = if colors.dark { Rgba::BLACK } else { Rgba::WHITE };
        self.water =
            Some(art::gradient(colors.sky_bottom.mix(deep, 0.35), colors.sky_top.mix(deep, 0.25)));
    }

    fn update(&mut self, t: f32) {
        let (Some(camera), Some(colors)) = (self.camera, self.colors) else { return };
        let b = self.bounds;
        let (sin, cos) = self.pitch;
        let [near, far, ..] = colors.lights;
        let last = (self.rows.len().max(2) - 1) as f32;
        self.placed.clear();
        for (k, &z) in self.rows.iter().enumerate() {
            let depth = k as f32 / last;
            // Far rows fade into the haze, the nearest come in softly at the bottom.
            let fog = (1.0 - smoothstep(0.3, 1.0, depth)) * smoothstep(0.0, 0.04, depth);
            let fog = 0.08 + 0.92 * fog;
            let tint = near.mix(far, smoothstep(0.0, 0.85, depth));
            // Every other row half a column aside: no columns lining up in the distance.
            let stagger = if k % 2 == 1 { self.step / 2.0 } else { 0.0 };
            // The row's heights at once (its columns evenly spaced).
            let (x0, dx) = ((-1.0 + stagger) * self.reach * z, self.step * self.reach * z);
            row_heights(x0, dx, self.columns.len(), z, t, &mut self.heights);
            for (&u, &h) in self.columns.iter().zip(&self.heights) {
                let x = (u + stagger) * self.reach * z;
                // Into the camera's space (pitched down), then onto the screen.
                let y = h - HEIGHT;
                let (cy, cz) = (y * cos + z * sin, z * cos - y * sin);
                let Some((sx, sy, _)) = camera.project(x, cy, cz) else { continue };
                if sy > b.max_y() + 8.0 || sy < b.min_y() {
                    continue;
                }
                let crest = ((h / CREST) * 0.5 + 0.5).clamp(0.0, 1.0);
                let alpha = (0.4 + 0.6 * crest * crest) * fog;
                let diameter = self.scale * (0.9 + 11.0 / cz).min(6.0) * (0.8 + 0.4 * crest);
                self.placed.push(Placed {
                    x: sx,
                    y: sy,
                    diameter,
                    color: tint.mix(Rgba::WHITE, crest * crest * 0.35).with_alpha(alpha),
                });
            }
        }
    }

    fn paint(&self, scene: &mut Scene, clip: Rect) {
        let (Some(sky), Some(sun), Some(haze), Some(colors)) =
            (&self.sky, &self.sun, &self.haze, &self.colors)
        else {
            return;
        };
        let b = self.bounds;
        let (w, h) = (b.size.width, b.size.height);
        let horizon = self.horizon();
        let back = scene.push_layer(Some(clip));
        back.image(sky, b);
        // The sky's stars and the sun's disc, above the horizon.
        let low = scene.push_layer(Some(clip));
        let star = if colors.dark { Rgba::WHITE } else { colors.lights[3] };
        for &(x, y, size, phase) in &self.stars {
            let sy = b.min_y() + y * (horizon - b.min_y()) * 0.9;
            let fade = 1.0 - smoothstep(0.5, 1.0, y);
            let alpha = (0.25 + 0.2 * (phase + sy * 0.01).sin()) * fade;
            dot(low, b.min_x() + x * w, sy, size * self.scale, star.with_alpha(alpha));
        }
        let disc = 0.085 * h;
        let sun_color = colors.warm.mix(Rgba::WHITE, 0.25).with_alpha(0.9);
        let sun_y = horizon - disc * 0.55;
        low.quad(
            Quad::filled(
                Rect::new(b.min_x() + w / 2.0 - disc, sun_y - disc, disc * 2.0, disc * 2.0),
                sun_color,
            )
            .rounded(disc),
        );
        let glows = scene.push_layer(Some(clip));
        sprite(glows, sun, b.min_x() + w / 2.0, sun_y, h * 0.9, h * 0.9, 0.55);
        // The water hides the sun's lower part; its light on it, and the haze on the line.
        let water = scene.push_layer(Some(clip));
        if let Some(sea) = &self.water {
            water.image(sea, Rect::new(b.min_x(), horizon, w, b.max_y() - horizon));
        }
        sprite(water, sun, b.min_x() + w / 2.0, horizon + 0.1 * h, disc * 2.6, 0.26 * h, 0.35);
        sprite(water, haze, b.min_x() + w / 2.0, horizon, w * 1.5, h * 0.3, 0.55);
        let dots = scene.push_layer(Some(clip));
        for p in &self.placed {
            dot(dots, p.x, p.y, p.diameter, p.color);
        }
    }

    fn memory(&self) -> usize {
        (self.rows.capacity() + self.columns.capacity()) * size_of::<f32>()
            + self.placed.capacity() * size_of::<Placed>()
            + self.stars.capacity() * size_of::<(f32, f32, f32, f32)>()
            + self
                .sky
                .iter()
                .chain(&self.sun)
                .chain(&self.haze)
                .chain(&self.water)
                .map(art::bytes)
                .sum::<usize>()
    }
}
