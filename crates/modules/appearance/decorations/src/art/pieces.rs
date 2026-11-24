//! The single pictures of the seasonal decorations: an ornament, a twinkle's sparkle, the
//! particles (flakes, leaves, petals, stars, confetti, drops), the glow's edge strips, a
//! corner's bow, holly, star, flower or leaf.

use super::*;

/// An ornament: a thin thread from its hook, a metal cap, a glass globe with depth and a
/// glint.
pub fn ornament(scale: f32, color: Rgba) -> Canvas {
    let s = scale;
    let (w, h) = ((ORNAMENT_WIDTH * s).ceil() as u32, (ORNAMENT_HEIGHT * s).ceil() as u32);
    let mut canvas = Canvas::new(w, h, color);
    let cx = w as f32 / 2.0;
    let gold = GOLD;
    canvas.capsule((cx, 0.0), (cx, 3.6 * s), 0.3 * s, darker(gold, 0.25), 0.9);
    canvas.capsule((cx - 0.9 * s, 4.2 * s), (cx + 0.9 * s, 4.2 * s), 0.75 * s, gold, 1.0);
    let (gy, r) = (9.5 * s, 4.5 * s);
    canvas.ellipse((cx, gy), (r, r), 0.0, 1.0, |u, v| {
        // Lit from the upper left: lighter there, deeper toward the lower right.
        let light = (-(u * 0.6 + v * 0.8)).clamp(-1.0, 1.0);
        let rim = (u * u + v * v).sqrt().powi(3);
        let base =
            if light > 0.0 { lighter(color, 0.28 * light) } else { darker(color, -0.35 * light) };
        darker(base, 0.25 * rim)
    });
    canvas
        .ellipse((cx - 1.6 * s, gy - 1.9 * s), (1.3 * s, 0.8 * s), -0.6, 0.55, |_, _| Rgba::WHITE);
    canvas.disc(cx + 1.9 * s, gy + 2.2 * s, 0.6 * s, lighter(color, 0.4), 0.35);
    canvas
}

/// A twinkle: a soft swell of the light's color with a faint four-pointed glint.
pub fn sparkle(scale: f32, color: Rgba) -> Canvas {
    let s = scale;
    let side = (SPARKLE * s).ceil() as u32;
    let mut canvas = Canvas::new(side, side, color);
    let c = side as f32 / 2.0;
    let bright = lighter(color, 0.45);
    canvas.glow(c, c, c, color, 0.55);
    canvas.glow(c, c, 2.6 * s, bright, 0.75);
    let ray = 5.6 * s;
    for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
        canvas.capsule(
            (c - dx * ray, c - dy * ray),
            (c + dx * ray, c + dy * ray),
            0.22 * s,
            bright,
            0.35,
        );
    }
    canvas.disc(c, c, 0.9 * s, lighter(color, 0.75), 0.9);
    canvas
}

/// A flake of size 0..=2: a soft dot; the largest a six-armed crystal.
pub fn flake(scale: f32, size: usize, color: Rgba) -> Canvas {
    let s = scale;
    let r = FLAKE_RADII[size.min(2)] * s;
    let side = (2.0 * r + 2.0 * s).ceil() as u32;
    let mut canvas = Canvas::new(side, side, color);
    let c = side as f32 / 2.0;
    if size < 2 {
        canvas.glow(c, c, r + s, color, 0.7);
        canvas.disc(c, c, r * 0.6, color, 0.9);
    } else {
        for k in 0..3 {
            let a = k as f32 * std::f32::consts::PI / 3.0 + 0.26;
            let (dx, dy) = (a.cos() * r, a.sin() * r);
            canvas.capsule((c - dx, c - dy), (c + dx, c + dy), 0.28 * s, color, 0.9);
        }
        canvas.disc(c, c, 0.55 * s, color, 1.0);
    }
    canvas
}

/// A leaf of `color` turned by `angle`: pointed at both ends, a midrib, a stem.
pub(super) fn leaf(
    canvas: &mut Canvas,
    (cx, cy): (f32, f32),
    (rx, ry): (f32, f32),
    angle: f32,
    color: Rgba,
    vein: Rgba,
) {
    // Pointed: the narrower toward its ends.
    canvas.ellipse((cx, cy), (rx, ry), angle, 1.0, |u, v| {
        let edge = v.abs() / (1.0 - u * u * 0.7).max(0.05);
        lighter(color, 0.16 * (-v).max(0.0))
            .mix(darker(color, 0.28), (u.abs() * 0.5 + edge * 0.2).min(1.0))
    });
    let (sin, cos) = angle.sin_cos();
    let (dx, dy) = (cos * rx * 0.85, sin * rx * 0.85);
    canvas.capsule((cx - dx, cy - dy), (cx + dx, cy + dy), (ry * 0.09).max(0.25), vein, 0.7);
    let (ex, ey) = (cx - cos * rx * 1.25, cy - sin * rx * 1.25);
    canvas.capsule((cx - dx, cy - dy), (ex, ey), (ry * 0.12).max(0.3), vein, 0.85);
}

/// A particle's image: its shape at `scale`, in `color`, its variant (a flake's size; a
/// leaf's, petal's or confetto's orientation; nothing for the rest).
pub fn particle(shape: ParticleShape, scale: f32, variant: usize, color: Rgba) -> Canvas {
    let s = scale;
    let angle = variant as f32 * std::f32::consts::FRAC_PI_4 + 0.35;
    match shape {
        ParticleShape::Snow => flake(scale, variant, color),
        ParticleShape::Sparkles => sparkle(scale, color),
        ParticleShape::Leaves => {
            let side = (11.0 * s).ceil() as u32;
            let mut canvas = Canvas::new(side, side, color);
            let c = side as f32 / 2.0;
            leaf(&mut canvas, (c, c), (3.9 * s, 1.75 * s), angle, color, darker(color, 0.35));
            canvas
        }
        ParticleShape::Petals => {
            let side = (8.0 * s).ceil() as u32;
            let mut canvas = Canvas::new(side, side, color);
            let c = side as f32 / 2.0;
            canvas.ellipse((c, c), (2.6 * s, 1.5 * s), angle, 1.0, |u, v| {
                let rim = (u * u + v * v).sqrt();
                lighter(color, 0.25 * (1.0 - rim)).mix(darker(color, 0.12), rim * 0.5)
            });
            canvas
        }
        ParticleShape::Confetti => {
            let side = (7.0 * s).ceil() as u32;
            let mut canvas = Canvas::new(side, side, color);
            let c = side as f32 / 2.0;
            // A small strip turning: shorter as it turns away.
            let length = (1.9 - 0.9 * (variant as f32 * 0.5).sin().abs()) * s;
            let (sin, cos) = angle.sin_cos();
            let (dx, dy) = (cos * length, sin * length);
            canvas.capsule((c - dx, c - dy), (c + dx, c + dy), 0.75 * s, color, 1.0);
            canvas.capsule(
                (c - dx * 0.8, c - dy * 0.8),
                (c + dx * 0.8, c + dy * 0.8),
                0.25 * s,
                lighter(color, 0.3),
                0.6,
            );
            canvas
        }
        ParticleShape::Stars => {
            let side = (7.0 * s).ceil() as u32;
            let mut canvas = Canvas::new(side, side, color);
            let c = side as f32 / 2.0;
            canvas.glow(c, c, c, color, 0.35);
            let ray = 2.9 * s;
            canvas.capsule((c - ray, c), (c + ray, c), 0.32 * s, color, 0.8);
            canvas.capsule((c, c - ray), (c, c + ray), 0.32 * s, color, 0.8);
            canvas.disc(c, c, 0.8 * s, lighter(color, 0.6), 1.0);
            canvas
        }
        ParticleShape::Rain => {
            let (w, h) = ((4.0 * s).ceil() as u32, (13.0 * s).ceil() as u32);
            let mut canvas = Canvas::new(w, h, color);
            let (x0, x1) = (w as f32 * 0.65, w as f32 * 0.35);
            // Fainter toward its tail (the top).
            for k in 0..6 {
                let (t0, t1) = (k as f32 / 6.0, (k + 1) as f32 / 6.0);
                let y = |t: f32| (1.0 + 11.0 * t) * s;
                let x = |t: f32| x0 + (x1 - x0) * t;
                canvas.capsule((x(t0), y(t0)), (x(t1), y(t1)), 0.38 * s, color, 0.25 + 0.75 * t1);
            }
            canvas
        }
    }
}

/// The glow's strips for the top, the bottom, the left and the right edge, `reach` pixels
/// deep (a gradient: stretched along its edge).
pub fn glow_strips(spec: &GlowSpec, reach: f32) -> [Canvas; 4] {
    let depth = reach.ceil().max(1.0) as u32;
    const ALONG: u32 = 4;
    let falloff = |d: f32| match spec.style {
        GlowStyle::Edge => (1.0 - d).max(0.0).powi(2),
        GlowStyle::Vignette => (1.0 - d).max(0.0).powf(1.6),
    };
    let strip = |vertical: bool, from_end: bool| {
        let (w, h) = if vertical { (ALONG, depth) } else { (depth, ALONG) };
        let mut canvas = Canvas::new(w, h, spec.color);
        for i in 0..depth {
            let d = (i as f32 + 0.5) / depth as f32;
            let a = falloff(d);
            let at = if from_end { depth - 1 - i } else { i } as i32;
            for k in 0..ALONG as i32 {
                if vertical {
                    canvas.blend(k, at, spec.color, a);
                } else {
                    canvas.blend(at, k, spec.color, a);
                }
            }
        }
        canvas
    };
    [strip(true, false), strip(true, true), strip(false, false), strip(false, true)]
}

/// A top corner's ornament (the left's; the right's is it mirrored).
pub fn corner(scale: f32, spec: &CornerSpec) -> Canvas {
    let s = scale;
    let side = (CORNER * s).round().max(1.0) as u32;
    let mut canvas = Canvas::new(side, side, spec.color);
    // Drawn on an 18-unit grid, fitted to the corner's side.
    let u = |v: f32| v * s * CORNER / 18.0;
    match spec.style {
        CornerStyle::Bow => {
            // A sprig of the accent behind it, then two loops, the tails, the knot.
            for (a, b) in [((1.0, 9.0), (6.5, 6.5)), ((17.0, 3.0), (11.0, 6.0))] {
                canvas.capsule((u(a.0), u(a.1)), (u(b.0), u(b.1)), u(0.9), spec.accent, 0.9);
            }
            let ribbon = spec.color;
            let loop_shade = move |uu: f32, vv: f32| {
                let inner = 1.0 - (uu * uu + vv * vv).sqrt();
                lighter(ribbon, 0.18 * (-vv).max(0.0)).mix(darker(ribbon, 0.35), inner * 0.5)
            };
            canvas.ellipse((u(5.0), u(6.4)), (u(4.6), u(3.1)), -0.42, 1.0, loop_shade);
            canvas.ellipse((u(13.0), u(6.4)), (u(4.6), u(3.1)), 0.42, 1.0, loop_shade);
            canvas.ellipse((u(5.4), u(6.5)), (u(1.9), u(0.8)), -0.45, 0.55, |_, _| {
                darker(ribbon, 0.4)
            });
            canvas.ellipse((u(12.6), u(6.5)), (u(1.9), u(0.8)), 0.45, 0.55, |_, _| {
                darker(ribbon, 0.4)
            });
            canvas.capsule((u(8.4), u(8.0)), (u(5.2), u(16.2)), u(1.35), darker(ribbon, 0.1), 1.0);
            canvas.capsule((u(9.6), u(8.0)), (u(12.9), u(15.8)), u(1.35), darker(ribbon, 0.1), 1.0);
            canvas.disc(u(9.0), u(7.2), u(2.2), darker(ribbon, 0.15), 1.0);
            canvas.disc(u(8.5), u(6.6), u(0.7), lighter(ribbon, 0.35), 0.6);
        }
        CornerStyle::Holly => {
            let leaf = spec.accent;
            for (cx, cy, a) in [(5.5, 7.0, -0.5), (12.0, 5.5, 0.35), (9.5, 12.0, 1.4)] {
                canvas.ellipse((u(cx), u(cy)), (u(4.2), u(1.9)), a, 1.0, |uu, vv| {
                    lighter(leaf, 0.15 * (-vv).max(0.0)).mix(darker(leaf, 0.3), uu.abs() * 0.4)
                });
                // The leaf's vein.
                let (sin, cos) = f32::sin_cos(a);
                let (dx, dy) = (cos * u(3.4), sin * u(3.4));
                canvas.capsule(
                    (u(cx) - dx, u(cy) - dy),
                    (u(cx) + dx, u(cy) + dy),
                    u(0.18),
                    lighter(leaf, 0.3),
                    0.6,
                );
            }
            for (cx, cy) in [(8.6, 7.6), (10.6, 8.8), (8.4, 10.0)] {
                canvas.disc(u(cx), u(cy), u(1.5), spec.color, 1.0);
                canvas.disc(u(cx) - u(0.45), u(cy) - u(0.5), u(0.45), Rgba::WHITE, 0.6);
            }
        }
        CornerStyle::Star => {
            let (cx, cy) = (u(9.0), u(8.5));
            canvas.glow(cx, cy, u(8.5), spec.color, 0.25);
            let (outer, inner) = (u(6.0), u(2.5));
            let color = spec.color;
            canvas.each((0.0, 0.0, side as f32, side as f32), |x, y| {
                let (px, py) = (x - cx, y - cy);
                let angle = px.atan2(-py);
                let seg = std::f32::consts::TAU / 5.0;
                let local = (angle.rem_euclid(seg) - seg / 2.0).abs() / (seg / 2.0);
                let edge = inner + (outer - inner) * (1.0 - local);
                let d = px.hypot(py) - edge;
                let c = (0.5 - d).clamp(0.0, 1.0);
                (c > 0.0).then(|| (lighter(color, 0.25 * (1.0 - px.hypot(py) / outer).max(0.0)), c))
            });
            canvas.disc(cx, cy, u(0.9), spec.accent, 0.8);
        }
        CornerStyle::Flower => {
            let (cx, cy) = (u(9.0), u(8.5));
            for k in 0..5 {
                let a = k as f32 * std::f32::consts::TAU / 5.0 - std::f32::consts::FRAC_PI_2;
                let (px, py) = (cx + a.cos() * u(3.9), cy + a.sin() * u(3.9));
                let petal = spec.color;
                canvas.ellipse((px, py), (u(3.3), u(2.2)), a, 1.0, move |uu, vv| {
                    let rim = (uu * uu + vv * vv).sqrt();
                    lighter(petal, 0.22 * (1.0 - rim)).mix(darker(petal, 0.2), (-uu).max(0.0) * 0.5)
                });
            }
            canvas.disc(cx, cy, u(2.2), spec.accent, 1.0);
            canvas.disc(cx - u(0.6), cy - u(0.6), u(0.7), lighter(spec.accent, 0.5), 0.7);
        }
        CornerStyle::Leaf => {
            leaf(&mut canvas, (u(9.5), u(8.0)), (u(6.6), u(3.2)), -0.62, spec.color, spec.accent);
            // Two side veins on each half.
            let (sin, cos) = f32::sin_cos(-0.62);
            for t in [-0.35f32, 0.2] {
                let (bx, by) = (u(9.5) + cos * u(6.6) * t, u(8.0) + sin * u(6.6) * t);
                for side in [-1.0f32, 1.0] {
                    let (nx, ny) = (-sin * side, cos * side);
                    let (ex, ey) = (bx + (cos * 0.9 + nx) * u(2.2), by + (sin * 0.9 + ny) * u(2.2));
                    canvas.capsule((bx, by), (ex, ey), u(0.16), spec.accent, 0.45);
                }
            }
        }
        // Drawn as the app's icon (a glyph), not here.
        CornerStyle::Icon => {}
    }
    canvas
}
