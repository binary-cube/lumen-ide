//! A garland drawn: its strand (needles, a wire or a ribbon), its bulbs and their glow,
//! its beads or flags; the swag between two hooks and the side tile.

use super::*;

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
