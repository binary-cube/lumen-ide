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

/// The evergreen strand along a path (`point(t)` for t in 0..1, `length` its length):
/// a thin stem with needles on both sides, pointing forward and out, in a few greens.
fn strand(
    canvas: &mut Canvas,
    scale: f32,
    wire: Rgba,
    length: f32,
    point: impl Fn(f32) -> (f32, f32),
) {
    let s = scale;
    let step = 0.9 * s;
    let count = (length / step).ceil() as usize + 1;
    let dark = darker(wire, 0.35);
    // The stem.
    for i in 0..count {
        let (a, b) = (point(i as f32 / count as f32), point((i + 1) as f32 / count as f32));
        canvas.capsule(a, b, 0.55 * s, dark, 1.0);
    }
    // Needles: two rows a side, alternating, so the strand looks full.
    for i in 0..count {
        let t = i as f32 / count as f32;
        let (x, y) = point(t);
        let (nx, ny) = point((t + 1.0 / count as f32).min(1.0));
        let (mut dx, mut dy) = (nx - x, ny - y);
        let len = dx.hypot(dy).max(1e-3);
        dx /= len;
        dy /= len;
        for side in [-1.0f32, 1.0] {
            let n = (i as u32) * 2 + u32::from(side > 0.0);
            let spread = 0.95 + 0.25 * hash(n * 5 + 1);
            let reach = (1.9 + 0.9 * hash(n * 5 + 2)) * s;
            // Forward along the strand, out to its side.
            let (ox, oy) = (-dy * side, dx * side);
            let (vx, vy) =
                (dx * spread.cos() + ox * spread.sin(), dy * spread.cos() + oy * spread.sin());
            let shade = hash(n * 5 + 3);
            let color = if shade < 0.35 {
                darker(wire, 0.2)
            } else if shade < 0.8 {
                wire
            } else {
                lighter(wire, 0.22)
            };
            canvas.capsule((x, y), (x + vx * reach, y + vy * reach), 0.42 * s, color, 0.95);
        }
    }
}

/// A light bulb hanging at `(cx, cy)` (its middle), with its socket up to the strand at
/// `socket`, its steady glow, a glint.
fn bulb(
    canvas: &mut Canvas,
    scale: f32,
    (cx, cy): (f32, f32),
    socket: (f32, f32),
    color: Rgba,
    glow: f32,
) {
    let s = scale;
    if glow > 0.0 {
        canvas.glow(cx, cy, 4.2 * s, color, 0.28 * glow);
    }
    canvas.capsule(socket, (cx, cy - 1.5 * s), 0.6 * s, SOCKET, 1.0);
    let angle = -(cx - socket.0).atan2(cy - socket.1);
    canvas.ellipse((cx, cy), (1.4 * s, 2.05 * s), angle, 1.0, |u, v| {
        // Brighter in the middle, a little deeper at the rim.
        let rim = (u * u + v * v).sqrt();
        lighter(color, 0.35 * (1.0 - rim)).mix(darker(color, 0.15), rim * 0.4)
    });
    canvas.disc(cx - 0.45 * s, cy - 0.7 * s, 0.42 * s, Rgba::WHITE, 0.7);
}

/// A thin wire along a path, with a faint light catching its top.
fn wire(
    canvas: &mut Canvas,
    scale: f32,
    color: Rgba,
    length: f32,
    point: impl Fn(f32) -> (f32, f32),
) {
    let s = scale;
    let count = (length / (1.2 * s)).ceil() as usize + 1;
    let dark = darker(color, 0.25);
    for i in 0..count {
        let (a, b) = (point(i as f32 / count as f32), point((i + 1) as f32 / count as f32));
        canvas.capsule(a, b, 0.5 * s, dark, 1.0);
    }
    let light = lighter(color, 0.35);
    for i in 0..count {
        let (a, b) = (point(i as f32 / count as f32), point((i + 1) as f32 / count as f32));
        canvas.capsule((a.0, a.1 - 0.2 * s), (b.0, b.1 - 0.2 * s), 0.16 * s, light, 0.55);
    }
}

/// A satin ribbon along a path: a band lighter along its top edge, darker along its
/// bottom, a soft twist now and then.
fn ribbon(
    canvas: &mut Canvas,
    scale: f32,
    color: Rgba,
    length: f32,
    point: impl Fn(f32) -> (f32, f32),
) {
    let s = scale;
    let count = (length / (1.0 * s)).ceil() as usize + 1;
    for i in 0..count {
        let (a, b) = (point(i as f32 / count as f32), point((i + 1) as f32 / count as f32));
        canvas.capsule(a, b, 1.35 * s, darker(color, 0.12), 1.0);
    }
    for i in 0..count {
        let (a, b) = (point(i as f32 / count as f32), point((i + 1) as f32 / count as f32));
        canvas.capsule(
            (a.0, a.1 - 0.55 * s),
            (b.0, b.1 - 0.55 * s),
            0.35 * s,
            lighter(color, 0.3),
            0.7,
        );
        canvas.capsule(
            (a.0, a.1 + 0.75 * s),
            (b.0, b.1 + 0.75 * s),
            0.3 * s,
            darker(color, 0.35),
            0.6,
        );
    }
    // A twist every ~14 px.
    let twists = (length / (14.0 * s)).floor() as usize;
    for k in 1..twists {
        let t = k as f32 / twists as f32;
        let (x, y) = point(t);
        canvas.capsule(
            (x - 0.5 * s, y - 1.2 * s),
            (x + 0.5 * s, y + 1.2 * s),
            0.35 * s,
            darker(color, 0.3),
            0.45,
        );
    }
}

/// The garland's string along a path, as its strand says.
fn string(
    canvas: &mut Canvas,
    scale: f32,
    g: &GarlandSpec,
    length: f32,
    point: impl Fn(f32) -> (f32, f32),
) {
    match g.strand {
        Strand::Evergreen => strand(canvas, scale, g.wire, length, point),
        Strand::Wire => wire(canvas, scale, g.wire, length, point),
        Strand::Ribbon => ribbon(canvas, scale, g.wire, length, point),
    }
}

/// A round bead sitting on the string, lit from the upper left.
fn bead(canvas: &mut Canvas, scale: f32, (cx, cy): (f32, f32), color: Rgba, glow: f32) {
    let s = scale;
    if glow > 0.0 {
        canvas.glow(cx, cy, 3.4 * s, color, 0.18 * glow);
    }
    let r = 1.75 * s;
    canvas.ellipse((cx, cy), (r, r), 0.0, 1.0, |u, v| {
        let light = (-(u * 0.6 + v * 0.8)).clamp(-1.0, 1.0);
        if light > 0.0 { lighter(color, 0.3 * light) } else { darker(color, -0.3 * light) }
    });
    canvas.disc(cx - 0.55 * s, cy - 0.6 * s, 0.45 * s, Rgba::WHITE, 0.65);
}

/// A pennant hanging from the string: its top along the string from `a` to `b`, its tip
/// at `tip`; lighter near the string.
fn flag(canvas: &mut Canvas, a: (f32, f32), b: (f32, f32), tip: (f32, f32), color: Rgba) {
    let top = (a.1 + b.1) / 2.0;
    let span = (tip.1 - top).abs().max((tip.0 - (a.0 + b.0) / 2.0).abs()).max(1.0);
    let (mx, my) = ((a.0 + b.0) / 2.0, top);
    canvas.polygon(&[a, b, tip], 1.0, |x, y| {
        let d = ((x - mx).hypot(y - my) / span).clamp(0.0, 1.0);
        lighter(color, 0.12 * (1.0 - d)).mix(darker(color, 0.18), d * 0.6)
    });
}

/// A swag's image: its string from end to end and its items (`width` may be rounded up from
/// the shape's: drawn on its own, then stretched by under 1 % into place).
pub fn swag(shape: &SwagShape, garland: &GarlandSpec) -> Canvas {
    let s = shape.scale;
    let (w, h) = (shape.width.ceil().max(1.0) as u32, shape.height.ceil().max(1.0) as u32);
    let mut canvas = Canvas::new(w, h, garland.wire);
    let width = shape.width;
    string(&mut canvas, s, garland, width, |t| (t * width, shape.strand_y(t * width)));
    if garland.has_items() {
        let spacing = width / shape.lights.max(1) as f32;
        for k in 0..shape.lights {
            let p = shape.light(k);
            let color = garland.colors[k % garland.colors.len().max(1)];
            match garland.items {
                Items::Bulbs => {
                    let socket = (p.x, shape.strand_y(p.x));
                    bulb(&mut canvas, s, (p.x, p.y), socket, color, garland.glow);
                }
                Items::Beads => bead(&mut canvas, s, (p.x, p.y), color, garland.glow),
                Items::Flags => {
                    let half = (spacing * 0.36).min(5.0 * s);
                    let (ax, bx) = (p.x - half, p.x + half);
                    let tip = (p.x, shape.strand_y(p.x) + 8.0 * s);
                    flag(
                        &mut canvas,
                        (ax, shape.strand_y(ax)),
                        (bx, shape.strand_y(bx)),
                        tip,
                        color,
                    );
                }
                Items::Nothing => {}
            }
        }
        // The string over the flags' tops (they hang from it).
        if garland.items == Items::Flags {
            string(&mut canvas, s, garland, width, |t| (t * width, shape.strand_y(t * width)));
        }
    }
    canvas
}

/// A side tile's image (the left edge's; the right's is it mirrored): the string waving
/// down and its items.
pub fn side(shape: &SideShape, garland: &GarlandSpec) -> Canvas {
    let s = shape.scale;
    let (w, h) = (shape.width.ceil().max(1.0) as u32, shape.length.ceil().max(1.0) as u32);
    let mut canvas = Canvas::new(w, h, garland.wire);
    let length = shape.length;
    string(&mut canvas, s, garland, length, |t| (shape.strand_x(t * length), t * length));
    if garland.has_items() {
        let lights = ui_components::decorations::SIDE_LIGHTS;
        for k in 0..lights {
            let p = shape.light(k);
            let color = garland.colors[(k + 1) % garland.colors.len().max(1)];
            match garland.items {
                Items::Bulbs => {
                    let socket = (shape.strand_x(p.y), p.y - 1.2 * s);
                    bulb(&mut canvas, s, (p.x, p.y), socket, color, garland.glow);
                }
                Items::Beads => bead(&mut canvas, s, (p.x, p.y), color, garland.glow),
                Items::Flags => {
                    let half = (length / lights as f32 * 0.36).min(5.0 * s);
                    let (ya, yb) = (p.y - half, p.y + half);
                    let tip = (shape.strand_x(p.y) + 8.0 * s, p.y);
                    flag(
                        &mut canvas,
                        (shape.strand_x(ya), ya),
                        (shape.strand_x(yb), yb),
                        tip,
                        color,
                    );
                }
                Items::Nothing => {}
            }
        }
        if garland.items == Items::Flags {
            string(&mut canvas, s, garland, length, |t| (shape.strand_x(t * length), t * length));
        }
    }
    canvas
}

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
fn leaf(
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

#[cfg(test)]
#[path = "../tests/unit/art.rs"]
mod tests;

// ---------------------------------------------------------------------------------------
// Plan 42's effects.

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

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
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

// ---------------------------------------------------------------------------------------
// Plan 42i's effects.

impl Canvas {
    /// A shape given by whether a point is inside it, anti-aliased by 4 × 4 samples a pixel,
    /// shaded by `shade(x, y)`.
    fn shape(
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
