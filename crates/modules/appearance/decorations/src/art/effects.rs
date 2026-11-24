//! The effects' pictures: glowing dots, bubbles, aurora bands, waves, a sun, a grid, the
//! sky's fade, rain, clouds, fog, a butterfly.

use super::*;

/// A soft glowing dot (a firefly, a burst's flash): a halo of `radius` (physical pixels)
/// around a bright core.
pub fn glow_dot(radius: f32, color: Rgba) -> Canvas {
    let side = (2.0 * radius).ceil().max(3.0) as u32;
    let mut c = Canvas::new(side, side, color);
    let m = side as f32 / 2.0;
    c.glow(m, m, radius, color, 0.55);
    c.glow(m, m, radius * 0.45, lighter(color, 0.35), 0.8);
    c.disc(m, m, (radius * 0.16).max(0.9), lighter(color, 0.75), 1.0);
    c
}

/// A bubble of `radius` (physical pixels): a thin rim, a faint fill, a highlight up left.
pub fn bubble(radius: f32, color: Rgba) -> Canvas {
    let side = (2.0 * radius + 2.0).ceil() as u32;
    let mut c = Canvas::new(side, side, color);
    let m = side as f32 / 2.0;
    let rim = (radius * 0.09).max(0.7);
    c.each((0.0, 0.0, side as f32, side as f32), |x, y| {
        let d = (x - m).hypot(y - m);
        if d > radius + 1.0 {
            return None;
        }
        // The fill, faint and brighter toward the rim; the rim itself.
        let fill = 0.08 + 0.18 * (d / radius).powi(3);
        let ring = coverage((d - (radius - rim)).abs() - rim);
        let edge = coverage(d - radius);
        Some((color, (fill * edge).max(0.85 * ring)))
    });
    let h = radius * 0.32;
    c.ellipse((m - radius * 0.38, m - radius * 0.42), (h, h * 0.55), -0.7, 0.75, |_, _| {
        Rgba::WHITE
    });
    c
}

/// One band of an aurora, `width` wide (one whole period: tiles join) and `height` tall
/// (physical pixels): a curtain of light brightest along a slow wave near its bottom,
/// fading upward, with fine vertical rays; `seed` makes each band its own.
pub fn aurora_band(width: u32, height: u32, color: Rgba, seed: u32) -> Canvas {
    let mut c = Canvas::new(width.max(1), height.max(1), color);
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let tau = std::f32::consts::TAU;
    let p1 = hash(seed * 3 + 1) * tau;
    let p2 = hash(seed * 3 + 2) * tau;
    let rays = 9.0 + (hash(seed * 3 + 3) * 6.0).floor();
    c.each((0.0, 0.0, w, h), |x, y| {
        let u = x / w * tau;
        // Where the curtain's lower edge is, and how tall it stands there.
        let edge = h * (0.62 + 0.14 * (u + p1).sin() + 0.07 * (2.0 * u + p2).sin());
        let tall = h * (0.42 + 0.12 * (3.0 * u + p2).sin().abs());
        let above = (edge - y) / tall;
        let a = if y > edge {
            // Below the edge: a quick, soft fade.
            let d = (y - edge) / (h * 0.06);
            (-d * d).exp()
        } else if above < 1.0 {
            (1.0 - above).powf(1.6)
        } else {
            0.0
        };
        let ray = 0.72 + 0.28 * (u * rays + 1.7 * (u * 2.0 + p1).sin()).sin().abs();
        let a = a * ray;
        (a > 0.004).then_some((color, a))
    });
    c
}

/// One period of a wave, `width` wide (tiles join) and `height` tall: the water under a
/// gentle crest, an anti-aliased top edge, brightest along it and fading downward.
pub fn wave_tile(width: u32, height: u32, color: Rgba, seed: u32) -> Canvas {
    let mut c = Canvas::new(width.max(1), height.max(1), color);
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let tau = std::f32::consts::TAU;
    let phase = hash(seed * 5 + 1) * tau;
    c.each((0.0, 0.0, w, h), |x, y| {
        let u = x / w * tau;
        let top = h * (0.34 + 0.2 * (u + phase).sin() + 0.05 * (2.0 * u + phase * 1.7).sin());
        let edge = coverage(top - y);
        if edge <= 0.0 {
            return None;
        }
        let depth = ((y - top) / (h - top).max(1.0)).clamp(0.0, 1.0);
        let crest = (-((y - top) / (h * 0.08)).powi(2)).exp();
        let a = edge * (0.45 * (1.0 - 0.6 * depth) + 0.55 * crest);
        Some((color.mix(Rgba::WHITE, 0.25 * crest), a))
    });
    c
}

/// A synthwave sun of `radius`: a disc from `top` to `bottom` in color, cut by horizontal
/// bands growing toward its bottom.
pub fn sun(radius: f32, top: Rgba, bottom: Rgba) -> Canvas {
    let side = (2.0 * radius + 2.0).ceil() as u32;
    let mut c = Canvas::new(side, side, top);
    let m = side as f32 / 2.0;
    c.each((0.0, 0.0, side as f32, side as f32), |x, y| {
        let d = (x - m).hypot(y - m);
        let edge = coverage(d - radius);
        if edge <= 0.0 {
            return None;
        }
        let v = ((y - (m - radius)) / (2.0 * radius)).clamp(0.0, 1.0);
        // The bands: below the middle, thicker as they go down.
        if v > 0.5 {
            let k = (v - 0.5) / 0.5;
            let period = 2.0 * radius * 0.11;
            let gap = period * (0.15 + 0.55 * k);
            if (y - m).rem_euclid(period) < gap {
                return None;
            }
        }
        Some((top.mix(bottom, v), edge))
    });
    c
}

/// A synthwave grid's lines converging to its horizon: `lines` of them from the middle of
/// the top edge to the bottom edge, fading toward the horizon (`width` × `height`).
pub fn grid_fan(width: u32, height: u32, lines: u32, color: Rgba, thickness: f32) -> Canvas {
    let mut c = Canvas::new(width.max(1), height.max(1), color);
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let cx = w / 2.0;
    c.each((0.0, 0.0, w, h), |x, y| {
        let v = y / h;
        // Where the line through this pixel meets the bottom edge (in line spacings).
        let at_bottom = cx + (x - cx) / v.max(0.02);
        let spacing = w * 2.4 / lines as f32;
        let k = (at_bottom - cx) / spacing;
        let dist = (k - k.round()).abs() * spacing * v;
        let a = coverage(dist - thickness * (0.4 + 0.6 * v));
        (a > 0.0).then_some((color, a * v.powf(0.8)))
    });
    c
}

// ---------------------------------------------------------------------------------------
// Plan 42h's effects.

pub(super) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A vertical fade (the sky lit by lightning): `color` strongest along the top edge, gone at
/// the bottom; a few pixels wide, stretched across the window.
pub fn sky_fade(height: u32, color: Rgba) -> Canvas {
    let h = height.max(2);
    let mut c = Canvas::new(4, h, color);
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        let a = (1.0 - v).powf(1.8);
        for x in 0..4 {
            c.blend(x, y as i32, color, a);
        }
    }
    c
}

/// A raindrop on the glass, `radius` (physical pixels) round at its bottom and drawn out
/// to a soft point above: a faint body, a darker rim, a bright highlight up left and a
/// glint where the light comes through at its bottom.
pub fn raindrop(radius: f32, color: Rgba) -> Canvas {
    let r = radius.max(1.5);
    let (w, h) = ((2.0 * r + 3.0).ceil() as u32, (2.7 * r + 3.0).ceil() as u32);
    let mut c = Canvas::new(w, h, color);
    let (m, cy) = (w as f32 / 2.0, h as f32 - r - 1.5);
    let tall = 1.7 * r;
    // The signed distance (roughly) to the drop's edge.
    let edge = move |x: f32, y: f32| {
        if y >= cy {
            return (x - m).hypot(y - cy) - r;
        }
        let t = (cy - y) / tall;
        if t >= 1.0 {
            return (x - m).abs().max(cy - tall - y) + 0.5;
        }
        let half = r * (1.0 - t * t).sqrt() * (1.0 - t).powf(0.55);
        (x - m).abs() - half
    };
    let rim = (r * 0.22).max(0.8);
    let dark = darker(color, 0.35);
    c.each((0.0, 0.0, w as f32, h as f32), |x, y| {
        let d = edge(x, y);
        let inside = coverage(d);
        if inside <= 0.0 {
            return None;
        }
        // Toward the rim (and more at the bottom: the lens's shadow) a darker edge.
        let toward = ((d + rim) / rim).clamp(0.0, 1.0);
        let low = ((y - cy) / r).clamp(0.0, 1.0);
        let shade = toward * (0.5 + 0.5 * low);
        Some((color.mix(dark, shade), inside * (0.32 + 0.5 * shade)))
    });
    c.ellipse((m - r * 0.36, cy - r * 0.38), (r * 0.26, r * 0.17), -0.6, 0.85, |_, _| Rgba::WHITE);
    c.disc(m + r * 0.3, cy + r * 0.5, (r * 0.12).max(0.6), lighter(color, 0.8), 0.6);
    c
}

/// A soft cloud `width` × `height` (texels): a flat body with round puffs on it, merged
/// smoothly (their fields added, then cut softly), `top` at its crown shading to `under` at
/// its base; `seed` makes each its own shape.
pub fn cloud(width: u32, height: u32, top: Rgba, under: Rgba, seed: u32) -> Canvas {
    let (w, h) = (width.max(4) as f32, height.max(4) as f32);
    let mut c = Canvas::new(width.max(4), height.max(4), top);
    let j = |i: u32| hash(seed * 23 + i) - 0.5;
    // (x, y, rx, ry), in fractions of the height (the width is ~2.4 heights).
    let a = w / h;
    let puffs = [
        (0.5 * a, 0.7, 0.4 * a, 0.17),
        ((0.3 + 0.06 * j(1)) * a, 0.55 + 0.05 * j(2), 0.22, 0.22),
        ((0.5 + 0.06 * j(3)) * a, 0.42 + 0.06 * j(4), 0.3, 0.29),
        ((0.69 + 0.05 * j(5)) * a, 0.52 + 0.05 * j(6), 0.24, 0.23),
        ((0.17 + 0.03 * j(7)) * a, 0.66, 0.15, 0.13),
        ((0.83 + 0.03 * j(8)) * a, 0.65, 0.16, 0.14),
    ];
    c.each((0.0, 0.0, w, h), |x, y| {
        let (u, v) = (x / h, y / h);
        let field: f32 = puffs
            .iter()
            .map(|&(px, py, rx, ry)| {
                let d2 = ((u - px) / rx).powi(2) + ((v - py) / ry).powi(2);
                (-1.6 * d2).exp()
            })
            .sum();
        let a = smoothstep(0.18, 0.75, field);
        if a <= 0.004 {
            return None;
        }
        Some((top.mix(under, smoothstep(0.35, 0.85, v)), a * 0.95))
    });
    c
}

/// One period of fog, `width` wide (tiles join) and `height` tall (texels): soft wisps,
/// thicker toward the bottom when it lies on the ground, a veil around its middle
/// otherwise; `seed` makes each band its own.
pub fn fog_band(width: u32, height: u32, color: Rgba, seed: u32, ground: bool) -> Canvas {
    let mut c = Canvas::new(width.max(1), height.max(1), color);
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let tau = std::f32::consts::TAU;
    let p = [hash(seed * 7 + 1) * tau, hash(seed * 7 + 2) * tau, hash(seed * 7 + 3) * tau];
    c.each((0.0, 0.0, w, h), |x, y| {
        let u = x / w * tau;
        let v = y / h;
        let thick = 0.6
            + 0.22 * (u + p[0]).sin()
            + 0.12 * (2.0 * u + p[1]).sin()
            + 0.06 * (5.0 * u + p[2]).sin();
        let a = if ground {
            let top = 0.22 + 0.14 * (u + p[1]).sin() + 0.06 * (3.0 * u + p[2]).sin();
            smoothstep(top, top + 0.55, v)
        } else {
            let mid = 0.5 + 0.1 * (2.0 * u + p[1]).sin();
            let spread = 0.2 + 0.06 * (3.0 * u + p[2]).sin();
            (-((v - mid) / spread).powi(2)).exp()
        };
        let a = a * thick.clamp(0.0, 1.0);
        (a > 0.004).then_some((color, a))
    });
    c
}

/// A butterfly seen from above, `span` (physical pixels) across its open wings, in `color`;
/// `open` (0..1) how far its wings spread now (the frames of its flapping).
pub fn butterfly(span: f32, color: Rgba, open: f32) -> Canvas {
    let side = (span + 3.0).ceil() as u32;
    let mut c = Canvas::new(side, side, color);
    let m = side as f32 / 2.0;
    let s = span;
    let k = open.clamp(0.08, 1.0);
    let rim = darker(color, 0.45);
    let wing = move |u: f32, v: f32| {
        let d = (u * u + v * v).sqrt();
        lighter(color, 0.25 * (1.0 - d).max(0.0)).mix(rim, smoothstep(0.65, 1.0, d))
    };
    for side_sign in [-1.0f32, 1.0] {
        // The hind wing, smaller, down and out; the fore wing over it, up and out.
        let (hx, hy) = (m + side_sign * s * 0.15 * k, m + s * 0.12);
        let hind = (s * 0.17 * k).max(0.8);
        c.ellipse((hx, hy), (hind, s * 0.12), side_sign * 0.7 * k, 1.0, wing);
        let (fx, fy) = (m + side_sign * s * 0.22 * k, m - s * 0.1);
        let fore = (s * 0.26 * k).max(0.8);
        c.ellipse((fx, fy), (fore, s * 0.16), -side_sign * 0.55 * k, 1.0, wing);
        // A pale spot near each fore wing's tip.
        if k > 0.3 {
            let (tx, ty) = (m + side_sign * s * 0.36 * k, m - s * 0.2);
            c.disc(tx, ty, s * 0.04 * k, lighter(color, 0.75), 0.9);
        }
    }
    let body = darker(color, 0.75);
    c.capsule((m, m - s * 0.18), (m, m + s * 0.24), (s * 0.035).max(0.7), body, 1.0);
    for side_sign in [-1.0f32, 1.0] {
        let tip = (m + side_sign * s * 0.1, m - s * 0.36);
        c.capsule((m, m - s * 0.17), tip, (s * 0.012).max(0.45), body, 0.9);
    }
    c
}
