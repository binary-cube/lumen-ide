//! The decorations' drawings, rasterized on the CPU once at the device's scale into small
//! images (RGBA8, not premultiplied; anti-aliased by each pixel's coverage, as
//! `progress_ring` does): a garland's swag and side tile (an evergreen strand of needles, a
//! wire or a ribbon, with its bulbs and their steady glow, its beads or its flags), an
//! ornament (a glass globe with its cap and thread), a twinkle's sparkle, the particles
//! (flakes, leaves, petals, stars, confetti, rain drops), the corners' bow, holly, star,
//! flower or leaf, the glow's edge strips.
//!
//! Every shape is drawn by a small premultiplied canvas: discs, capsules, ellipses and
//! glows, composited "over". Positions come from the layout's shapes
//! ([`SwagShape`], [`SideShape`]), so what's drawn here and the sparkles placed later meet.

use kernel::Rgba;
use theming::decorations::{
    CornerSpec, CornerStyle, GarlandSpec, GlowSpec, GlowStyle, Items, ParticleShape, Strand,
};
use ui_components::decorations::{
    CORNER, FLAKE_RADII, ORNAMENT_HEIGHT, ORNAMENT_WIDTH, SPARKLE, SideShape, SwagShape, hash,
};

mod effects;
mod figures;
mod garland;
mod pieces;

pub use effects::*;
pub use figures::*;
pub use garland::{side, swag};
pub use pieces::*;

/// A canvas of premultiplied colors.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixels: Vec<[f32; 4]>,
    /// The color written into fully transparent pixels (so filtering never darkens edges).
    hint: Rgba,
}

/// A bulb's socket (dark green metal) and an ornament's cap (gold): the pictures' own,
/// whatever the theme.
const SOCKET: Rgba = Rgba::rgb(0x3b4a3c);
const GOLD: Rgba = Rgba::rgb(0xc8ae6a);

/// How much of a pixel a shape covers, from its center's signed distance to the edge.
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Canvas {}x{}", self.width, self.height)
    }
}

impl Canvas {
    pub fn new(width: u32, height: u32, hint: Rgba) -> Self {
        Self { width, height, pixels: vec![[0.0; 4]; (width * height) as usize], hint }
    }

    /// `color` at `alpha` over the pixel.
    fn blend(&mut self, x: i32, y: i32, color: Rgba, alpha: f32) {
        if alpha <= 0.0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = (alpha * color.a).clamp(0.0, 1.0);
        let p = &mut self.pixels[(y as u32 * self.width + x as u32) as usize];
        let keep = 1.0 - a;
        p[0] = color.r * a + p[0] * keep;
        p[1] = color.g * a + p[1] * keep;
        p[2] = color.b * a + p[2] * keep;
        p[3] = a + p[3] * keep;
    }

    /// Runs `shade` over the pixels of a box (its centers), blending what it returns.
    fn each(
        &mut self,
        (x0, y0, x1, y1): (f32, f32, f32, f32),
        mut shade: impl FnMut(f32, f32) -> Option<(Rgba, f32)>,
    ) {
        let (xa, ya) = ((x0.floor() as i32).max(0), (y0.floor() as i32).max(0));
        let (xb, yb) =
            ((x1.ceil() as i32).min(self.width as i32), (y1.ceil() as i32).min(self.height as i32));
        for y in ya..yb {
            for x in xa..xb {
                if let Some((color, alpha)) = shade(x as f32 + 0.5, y as f32 + 0.5) {
                    self.blend(x, y, color, alpha);
                }
            }
        }
    }

    /// A filled disc.
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, color: Rgba, alpha: f32) {
        let b = (cx - r - 1.0, cy - r - 1.0, cx + r + 1.0, cy + r + 1.0);
        self.each(b, |x, y| Some((color, alpha * coverage((x - cx).hypot(y - cy) - r))));
    }

    /// A soft glow: brightest in the middle, gone at `r`.
    pub fn glow(&mut self, cx: f32, cy: f32, r: f32, color: Rgba, alpha: f32) {
        let b = (cx - r, cy - r, cx + r, cy + r);
        self.each(b, |x, y| {
            let d = (x - cx).hypot(y - cy) / r.max(0.5);
            (d < 1.0).then(|| {
                let fall = (1.0 - d * d).powi(2);
                (color, alpha * fall)
            })
        })
    }

    /// A segment with round caps, `half` its half-thickness.
    pub fn capsule(
        &mut self,
        (ax, ay): (f32, f32),
        (bx, by): (f32, f32),
        half: f32,
        color: Rgba,
        alpha: f32,
    ) {
        let pad = half + 1.0;
        let b = (ax.min(bx) - pad, ay.min(by) - pad, ax.max(bx) + pad, ay.max(by) + pad);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.each(b, |x, y| {
            let t = (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0);
            let d = (x - ax - t * dx).hypot(y - ay - t * dy) - half;
            Some((color, alpha * coverage(d)))
        });
    }

    /// A filled ellipse turned by `angle` (radians), shaded by `shade(u, v)` — the point's
    /// place in it (-1..1 on both axes).
    pub fn ellipse(
        &mut self,
        (cx, cy): (f32, f32),
        (rx, ry): (f32, f32),
        angle: f32,
        alpha: f32,
        shade: impl Fn(f32, f32) -> Rgba,
    ) {
        let r = rx.max(ry) + 1.0;
        let (sin, cos) = angle.sin_cos();
        self.each((cx - r, cy - r, cx + r, cy + r), |x, y| {
            let (px, py) = (x - cx, y - cy);
            let (u, v) = ((px * cos + py * sin) / rx, (-px * sin + py * cos) / ry);
            let d = ((u * u + v * v).sqrt() - 1.0) * rx.min(ry);
            let c = coverage(d);
            (c > 0.0).then(|| (shade(u, v), alpha * c))
        });
    }

    /// A filled convex polygon (its points in order, either way round), shaded by
    /// `shade(x, y)`.
    pub fn polygon(&mut self, points: &[(f32, f32)], alpha: f32, shade: impl Fn(f32, f32) -> Rgba) {
        if points.len() < 3 {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in points {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        // Which way round: the signed area.
        let area: f32 = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|((ax, ay), (bx, by))| ax * by - bx * ay)
            .sum();
        let turn = if area >= 0.0 { 1.0 } else { -1.0 };
        self.each((x0 - 1.0, y0 - 1.0, x1 + 1.0, y1 + 1.0), |x, y| {
            // The distance outside the farthest edge (inside: negative).
            let mut d = f32::MIN;
            for ((ax, ay), (bx, by)) in points.iter().zip(points.iter().cycle().skip(1)) {
                let (ex, ey) = (bx - ax, by - ay);
                let len = ex.hypot(ey).max(1e-4);
                let out = -turn * (ex * (y - ay) - ey * (x - ax)) / len;
                d = d.max(out);
            }
            let c = coverage(d);
            (c > 0.0).then(|| (shade(x, y), alpha * c))
        });
    }

    /// The pixels as RGBA8, not premultiplied.
    pub fn into_rgba8(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        let hint = self.hint.to_rgba8();
        for p in &self.pixels {
            if p[3] <= 1.0 / 512.0 {
                out.extend_from_slice(&[hint[0], hint[1], hint[2], 0]);
            } else {
                let a = p[3];
                out.extend_from_slice(&Rgba::new(p[0] / a, p[1] / a, p[2] / a, a).to_rgba8());
            }
        }
        out
    }

    /// Mirrored left to right (the right edge's tiles).
    pub fn mirrored(mut self) -> Self {
        let w = self.width as usize;
        for row in self.pixels.chunks_mut(w) {
            row.reverse();
        }
        self
    }
}

fn lighter(c: Rgba, t: f32) -> Rgba {
    c.mix(Rgba::WHITE, t).with_alpha(c.a)
}

fn darker(c: Rgba, t: f32) -> Rgba {
    c.mix(Rgba::BLACK, t).with_alpha(c.a)
}

#[cfg(test)]
#[path = "../../tests/unit/art.rs"]
mod tests;
