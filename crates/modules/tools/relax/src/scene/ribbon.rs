//! **Ribbon**: under a deep gradient sky, one or two translucent ribbons of light undulate
//! slowly across the middle — each a bundle of thin parallel curves that twists, the curves
//! drawing together (brighter) where it folds — with a soft glow along it, sparkles
//! drifting and twinkling around it and a few large out-of-focus lights in front and
//! behind. The curves are rows of small round dots (the renderer draws no lines), placed
//! along x samples laid out once; per frame only their heights are worked out.

use std::f32::consts::TAU;

use kernel::{Rect, Rgba};
use render_api::{ImageData, Scene};

use super::{RelaxScene, SceneColors, dot, rand, smoothstep, sprite};
use crate::art;

pub const ID: &str = "ribbon";

/// The sine and cosine of one radian (a curve's wiggle is a radian on from the last).
const ONE_RADIAN: (f32, f32) = (0.841_470_96, 0.540_302_3);

/// The ribbons, and the curves in one at density 1.
const RIBBONS: usize = 2;
const CURVES: f32 = 9.0;
/// A sample every this many logical pixels along x.
const STEP: f32 = 2.0;
const SPARKLES: f32 = 90.0;
const BOKEH: u32 = 12;

/// A dot placed at a time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
    pub color: Rgba,
}

#[derive(Debug, Default)]
pub struct Ribbon {
    bounds: Rect,
    scale: f32,
    density: f32,
    /// The samples along x (shares of the width).
    samples: Vec<f32>,
    curves: usize,
    placed: Vec<Placed>,
    /// Sparkles: start (share of the width), height around the ribbon (−1..1), speed, phase.
    sparkles: Vec<(f32, f32, f32, f32)>,
    /// The glow along the ribbons and the out-of-focus lights (placed at the last update).
    glows: Vec<(f32, f32, f32, f32)>,
    colors: Option<SceneColors>,
    tint: Rgba,
    sky: Option<ImageData>,
    glow: Option<ImageData>,
    bokeh: Option<ImageData>,
}

/// Ribbon `r`'s middle at `u` (share of the width) and time `t` (share of the height) and
/// its half-thickness, signed (it twists through 0).
pub fn ribbon_at(r: usize, u: f32, t: f32) -> (f32, f32) {
    let r = r as f32;
    let middle = 0.52
        + 0.035 * r
        + 0.09 * (2.1 * u + 0.21 * t + r * 1.7).sin()
        + 0.035 * (4.3 * u - 0.33 * t + r).sin();
    let half = (0.06 - 0.015 * r) * (3.1 * u + 0.17 * t + r * 2.3).sin();
    (middle, half)
}

impl Ribbon {
    /// The dots placed at the last update.
    pub fn placed(&self) -> &[Placed] {
        &self.placed
    }

    fn layout(&mut self) {
        let width = self.bounds.size.width;
        if width <= 0.0 || self.density <= 0.0 {
            return;
        }
        let step = STEP * self.scale.max(0.5) / self.density.clamp(0.25, 2.0).sqrt();
        let n = (width / step).ceil() as usize + 1;
        self.samples = (0..n).map(|i| i as f32 * step / width).collect();
        let sparkles = self.sparkles.len();
        self.placed = Vec::with_capacity(n * self.curves * RIBBONS + sparkles);
    }
}

impl RelaxScene for Ribbon {
    fn id(&self) -> &'static str {
        ID
    }

    fn own_colors(&self) -> SceneColors {
        SceneColors {
            sky_top: Rgba::rgb(0x020a2e),
            sky_bottom: Rgba::rgb(0x1a46a8),
            horizon: Rgba::rgb(0x2a66d0),
            lights: [
                Rgba::rgb(0xb8dcff),
                Rgba::rgb(0x7fb6ff),
                Rgba::rgb(0xd2c4ff),
                Rgba::rgb(0x9fefff),
            ],
            warm: Rgba::rgb(0xffe3b0),
            dark: true,
        }
    }

    fn resize(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
        self.layout();
    }

    fn set_density(&mut self, density: f32) {
        if self.density == density && !self.samples.is_empty() {
            return;
        }
        self.density = density;
        let d = density.clamp(0.25, 2.0);
        self.curves = ((CURVES * d.sqrt()).round() as usize).clamp(4, 14);
        let n = (SPARKLES * d) as u32;
        self.sparkles = (0..n)
            .map(|i| {
                (
                    rand(i, 91),
                    rand(i, 92) * 2.0 - 1.0,
                    0.004 + 0.01 * rand(i, 93),
                    rand(i, 94) * TAU,
                )
            })
            .collect();
        self.glows = Vec::with_capacity(16 + BOKEH as usize);
        self.layout();
    }

    fn set_colors(&mut self, colors: &SceneColors) {
        if self.colors.as_ref() == Some(colors) {
            return;
        }
        self.colors = Some(*colors);
        self.tint =
            if colors.dark { colors.lights[0].mix(Rgba::WHITE, 0.35) } else { colors.lights[0] };
        self.sky = Some(art::gradient(colors.sky_top, colors.sky_bottom));
        self.glow = Some(art::glow(colors.lights[1], 0.0));
        self.bokeh = Some(art::glow(colors.lights[2].mix(Rgba::WHITE, 0.3), 0.55));
    }

    fn update(&mut self, t: f32) {
        let b = self.bounds;
        let (w, h) = (b.size.width, b.size.height);
        let tint = self.tint;
        let curves = self.curves.max(2);
        let d = self.scale * 2.2;
        self.placed.clear();
        for r in 0..RIBBONS {
            let strength = if r == 0 { 1.0 } else { 0.6 };
            for &u in &self.samples {
                let (middle, half) = ribbon_at(r, u, t);
                // Where it folds (thin), the curves crowd together: brighter. The same for
                // every curve at this sample: worked out once.
                let fold = 1.0 - (half.abs() / 0.06).min(1.0);
                let x = b.min_x() + u * w;
                let light = tint.mix(Rgba::WHITE, fold * 0.5);
                let alpha = (0.16 + 0.4 * fold.powi(3)) * strength;
                let (inner, edge) =
                    (light.with_alpha(alpha.min(0.9)), light.with_alpha((alpha * 1.5).min(0.9)));
                // Each curve's wiggle a radian on from the last: turned on, not computed again.
                let (mut ws, mut wc) = (7.0 * u + 0.5 * t).sin_cos();
                for j in 0..curves {
                    let s = j as f32 / (curves - 1) as f32 - 0.5;
                    let y = b.min_y() + (middle + half * 2.0 * s + 0.004 * ws) * h;
                    let color = if j == 0 || j == curves - 1 { edge } else { inner };
                    self.placed.push(Placed { x, y, diameter: d, color });
                    (ws, wc) = (
                        ws * ONE_RADIAN.1 + wc * ONE_RADIAN.0,
                        wc * ONE_RADIAN.1 - ws * ONE_RADIAN.0,
                    );
                }
            }
        }
        // Sparkles drifting along, twinkling, around the first ribbon.
        for &(start, around, speed, phase) in &self.sparkles {
            let u = (start + t * speed).rem_euclid(1.0);
            let (middle, _) = ribbon_at(0, u, t);
            let bob = 0.01 * (t * 0.6 + phase).sin();
            let y = b.min_y() + (middle + around * 0.16 + bob) * h;
            let twinkle = (0.5 + 0.5 * (t * (0.8 + speed * 60.0) + phase).sin()).powi(3);
            let fade = smoothstep(0.0, 0.05, u) * (1.0 - smoothstep(0.95, 1.0, u));
            let diameter = self.scale * (1.2 + 1.8 * twinkle);
            let color = Rgba::WHITE.mix(tint, 0.3).with_alpha(0.15 + 0.7 * twinkle * fade);
            self.placed.push(Placed { x: b.min_x() + u * w, y, diameter, color });
        }
        // The glow along the first ribbon, and the out-of-focus lights.
        self.glows.clear();
        for k in 0..16 {
            let u = k as f32 / 15.0;
            let (middle, _) = ribbon_at(0, u, t);
            self.glows.push((b.min_x() + u * w, b.min_y() + middle * h, w / 7.0, 0.0));
        }
        for i in 0..BOKEH {
            let u = (rand(i, 95) + t * (0.002 + 0.004 * rand(i, 96))).rem_euclid(1.0);
            let y = 0.2 + 0.6 * rand(i, 97) + 0.03 * (t * 0.1 + i as f32).sin();
            let size = h * (0.04 + 0.08 * rand(i, 98));
            let alpha = (0.05 + 0.1 * rand(i, 99)) * (0.7 + 0.3 * (t * 0.3 + i as f32).sin());
            self.glows.push((b.min_x() + u * w, b.min_y() + y * h, size, alpha));
        }
    }

    fn paint(&self, scene: &mut Scene, clip: Rect) {
        let (Some(sky), Some(glow), Some(bokeh)) = (&self.sky, &self.glow, &self.bokeh) else {
            return;
        };
        let h = self.bounds.size.height;
        let back = scene.push_layer(Some(clip));
        back.image(sky, self.bounds);
        // The glow along the ribbon, and the far lights (the first half), behind it.
        let half = BOKEH as usize / 2;
        for &(x, y, size, alpha) in &self.glows {
            if alpha == 0.0 {
                sprite(back, glow, x, y, size * 1.6, h * 0.36, 0.3);
            }
        }
        for &(x, y, size, alpha) in self.glows.iter().skip(16).take(half) {
            sprite(back, bokeh, x, y, size, size, alpha);
        }
        let dots = scene.push_layer(Some(clip));
        for p in &self.placed {
            dot(dots, p.x, p.y, p.diameter, p.color);
        }
        // The near lights, over it.
        let front = scene.push_layer(Some(clip));
        for &(x, y, size, alpha) in self.glows.iter().skip(16 + half) {
            sprite(front, bokeh, x, y, size * 1.8, size * 1.8, alpha * 0.8);
        }
    }

    fn memory(&self) -> usize {
        self.samples.capacity() * size_of::<f32>()
            + self.placed.capacity() * size_of::<Placed>()
            + (self.sparkles.capacity() + self.glows.capacity()) * size_of::<(f32, f32, f32, f32)>()
            + self.sky.iter().chain(&self.glow).chain(&self.bokeh).map(art::bytes).sum::<usize>()
    }
}
