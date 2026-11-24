//! The pictures drawn as shapes (a point inside or not, anti-aliased by samples): balloons,
//! hearts, bats, blobs, scanlines, halos, a skyline.

use super::*;

impl Canvas {
    /// A shape given by whether a point is inside it, anti-aliased by 4 × 4 samples a pixel,
    /// shaded by `shade(x, y)`.
    pub(super) fn shape(
        &mut self,
        b: (f32, f32, f32, f32),
        inside: impl Fn(f32, f32) -> bool,
        shade: impl Fn(f32, f32) -> Rgba,
    ) {
        self.each(b, |x, y| {
            let mut n = 0;
            for i in 0..4 {
                for j in 0..4 {
                    let (sx, sy) = (x - 0.375 + 0.25 * i as f32, y - 0.375 + 0.25 * j as f32);
                    if inside(sx, sy) {
                        n += 1;
                    }
                }
            }
            (n > 0).then(|| (shade(x, y), n as f32 / 16.0))
        });
    }
}

/// A balloon `radius` (physical pixels) across, in `color`: a body a little taller than wide,
/// lit from up left, a glossy highlight, its knot below (the string is drawn apart).
pub fn balloon(radius: f32, color: Rgba) -> Canvas {
    let (rx, ry) = (radius.max(2.0), radius.max(2.0) * 1.2);
    let (w, h) = ((2.0 * rx + 3.0).ceil() as u32, (2.0 * ry + rx * 0.35 + 3.0).ceil() as u32);
    let mut c = Canvas::new(w, h, color);
    let (cx, cy) = (w as f32 / 2.0, ry + 1.5);
    let shade = darker(color, 0.35);
    c.ellipse((cx, cy), (rx, ry), 0.0, 1.0, move |u, v| {
        let light = ((-u * 0.6 - v * 0.8) * 0.5 + 0.5).clamp(0.0, 1.0);
        shade.mix(lighter(color, 0.25), light)
    });
    let k = rx * 0.2;
    c.polygon(
        &[(cx - k, cy + ry + k * 0.9), (cx + k, cy + ry + k * 0.9), (cx, cy + ry - k * 0.3)],
        1.0,
        move |_, _| shade,
    );
    c.ellipse((cx - rx * 0.38, cy - ry * 0.42), (rx * 0.16, ry * 0.26), 0.5, 0.7, |_, _| {
        Rgba::WHITE
    });
    c
}

/// A heart `size` (physical pixels) across, in `color`, lit from up left, with a soft halo
/// of `glow` around it (the sprite is the halo's size, the heart in its middle).
pub fn heart(size: f32, color: Rgba, glow: f32) -> Canvas {
    let s = size.max(4.0);
    let side = (s * 1.7).ceil() as u32;
    let mut c = Canvas::new(side, side, color);
    let m = side as f32 / 2.0;
    if glow > 0.0 {
        c.glow(m, m, s * 0.85, color, 0.4 * glow);
    }
    // The classic curve, (x² + y² − 1)³ − x² y³ ≤ 0, about 2.3 units across.
    let unit = s / 2.3;
    let rim = darker(color, 0.3);
    let at = move |x: f32, y: f32| ((x - m) / unit, -(y - m) / unit + 0.12);
    c.shape(
        (m - s * 0.6, m - s * 0.6, m + s * 0.6, m + s * 0.6),
        |x, y| {
            let (u, v) = at(x, y);
            (u * u + v * v - 1.0).powi(3) - u * u * v * v * v <= 0.0
        },
        |x, y| {
            let (u, v) = at(x, y);
            let light = ((-u * 0.5 + v * 0.6) * 0.45 + 0.55).clamp(0.0, 1.0);
            rim.mix(lighter(color, 0.3), light)
        },
    );
    c.ellipse((m - s * 0.2, m - s * 0.18), (s * 0.09, s * 0.06), -0.7, 0.55, |_, _| Rgba::WHITE);
    c
}

/// A bat's silhouette `span` (physical pixels) across its wings, in `color`; `lift` (−1..1)
/// where its wings are now (up, level, down: the frames of its flapping).
pub fn bat(span: f32, color: Rgba, lift: f32) -> Canvas {
    let s = span.max(6.0);
    let (w, h) = ((s + 3.0).ceil() as u32, (s * 0.62 + 3.0).ceil() as u32);
    let mut c = Canvas::new(w, h, color);
    let (m, cy) = (w as f32 / 2.0, h as f32 * 0.42);
    let half = s / 2.0;
    let inside = move |x: f32, y: f32| {
        let (u, v) = ((x - m) / half, (y - cy) / half);
        // The body and the head with its ears.
        if (u / 0.11).powi(2) + ((v - 0.08) / 0.24).powi(2) <= 1.0 {
            return true;
        }
        if (u / 0.1).powi(2) + ((v + 0.14) / 0.1).powi(2) <= 1.0 {
            return true;
        }
        let ear = u.abs();
        if (0.03..0.09).contains(&ear) && v < -0.18 && v > -0.3 + (ear - 0.03) * 1.2 {
            return true;
        }
        // A wing: its top edge rising (or falling) toward the tip, its bottom edge scalloped.
        let a = u.abs();
        if !(0.06..=1.0).contains(&a) {
            return false;
        }
        let pi = std::f32::consts::PI;
        // The leading edge arches up from the shoulder, its tip a point.
        let top = -0.04 - lift * 0.55 * a.powf(1.3) - 0.1 * (a * pi).sin();
        let depth = 0.4 * (1.0 - a).powf(0.85) + 0.02;
        let scallop = 0.1 * ((a - 0.06) / 0.94 * 3.0 * pi).sin().abs() * (1.0 - a * 0.5);
        v >= top && v <= top + depth - scallop + 0.08
    };
    let lit = lighter(color, 0.12);
    c.shape((0.0, 0.0, w as f32, h as f32), inside, move |_, y| {
        color.mix(lit, ((cy - y) / half).clamp(0.0, 1.0))
    });
    c
}

/// A lava lamp's blob, `size` texels across, in `color`: a disc with a soft edge (stretched
/// far, it stays round and soft), brighter toward its upper middle like lit wax.
pub fn blob(size: u32, color: Rgba) -> Canvas {
    let side = size.max(8);
    let mut c = Canvas::new(side, side, color);
    let m = side as f32 / 2.0;
    let r = m - 1.0;
    let glossy = lighter(color, 0.35);
    c.each((0.0, 0.0, side as f32, side as f32), |x, y| {
        let d = (x - m).hypot(y - m) / r;
        let a = 1.0 - smoothstep(0.78, 1.0, d);
        if a <= 0.0 {
            return None;
        }
        let g = (1.0 - ((x - m).hypot(y - m * 0.7) / r)).clamp(0.0, 1.0);
        Some((color.mix(glossy, g * g), a))
    });
    c
}

/// A CRT's scanlines: `lines` periods of `period` pixels (a line `thick` pixels dark, the
/// rest clear), two pixels wide (stretched across the window, the rows stay exact).
pub fn scanlines(period: u32, thick: u32, lines: u32, color: Rgba) -> Canvas {
    let p = period.max(2);
    let t = thick.clamp(1, p - 1);
    let h = p * lines.max(1);
    let mut c = Canvas::new(2, h, color);
    for y in 0..h {
        if y % p < t {
            for x in 0..2 {
                c.blend(x, y as i32, color, 1.0);
            }
        }
    }
    c
}

// ---------------------------------------------------------------------------------------
// Plan 45's effects.

/// A soft halo with no core (a sign's glow): brightest in the middle, gone at its edge;
/// small, stretched to any size.
pub fn halo(size: u32, color: Rgba) -> Canvas {
    let side = size.max(8);
    let mut c = Canvas::new(side, side, color);
    let m = side as f32 / 2.0;
    c.glow(m, m, m, color, 1.0);
    c
}

/// A city's skyline, `width` × `height` texels: buildings of `body` standing on the bottom
/// edge, a few of their windows lit in `lights`, an antenna now and then; `seed` makes it
/// its own.
pub fn skyline(width: u32, height: u32, body: Rgba, lights: Rgba, seed: u32) -> Canvas {
    let (w, h) = (width.max(1), height.max(4));
    let mut c = Canvas::new(w, h, body);
    let mut x = 0u32;
    let mut i = 0u32;
    while x < w {
        let s = seed.wrapping_add(i * 7);
        let bw = ((w as f32 * (0.015 + 0.035 * hash(s))).round() as u32).max(3);
        let bh = ((h as f32 - 2.0) * (0.25 + 0.75 * hash(s + 1).powf(1.4))).round().max(2.0) as u32;
        let top = h - bh;
        for y in top..h {
            for px in x..(x + bw).min(w) {
                c.blend(px as i32, y as i32, body, 1.0);
            }
        }
        // Lit windows, one texel each, in a grid of every other texel.
        for y in (top + 1..h.saturating_sub(1)).step_by(2) {
            for px in (x + 1..(x + bw).min(w).saturating_sub(1)).step_by(2) {
                if hash(s.wrapping_mul(131) ^ (y * 977 + px * 31)) < 0.16 {
                    c.blend(px as i32, y as i32, lights, 0.9);
                }
            }
        }
        // An antenna on a tall one, its tip lit.
        if hash(s + 2) > 0.8 && top > 3 {
            let ax = (x + bw / 2) as i32;
            let tall = (2 + (hash(s + 3) * 3.0) as u32).min(top);
            for y in top - tall..top {
                c.blend(ax, y as i32, body, 1.0);
            }
            c.blend(ax, (top - tall) as i32, lights, 1.0);
        }
        x += bw + (hash(s + 4) * 2.0) as u32;
        i += 1;
    }
    c
}
