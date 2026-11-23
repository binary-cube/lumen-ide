//! Constellations, the synthwave horizon, lightning, rain, clouds and fog.

use super::*;

/// The synthwave sun's own colors when the effect gives none: amber at its top, pink at
/// its bottom.
const SUN_TOP: Rgba = Rgba::rgb(0xffb347);
const SUN_BOTTOM: Rgba = Rgba::rgb(0xff4f8b);

/// A star of a constellation: its home, how it drifts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub x: f32,
    pub y: f32,
    pub drift: f32,
    pub f1: f32,
    pub f2: f32,
    pub phase: f32,
}

/// The most nodes, and the most links each draws.
pub const MAX_NODES: usize = 56;
pub const LINKS_PER_NODE: usize = 2;

pub fn node_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Node> {
    let n = ((e.density * 60.0 * area(window, scale).sqrt()).round() as usize).min(MAX_NODES);
    (0..n as u32)
        .map(|i| Node {
            x: hash(i * 11 + 1),
            y: hash(i * 11 + 2),
            drift: (16.0 + 34.0 * hash(i * 11 + 3)) * scale,
            f1: 0.03 + 0.06 * hash(i * 11 + 4),
            f2: 0.025 + 0.05 * hash(i * 11 + 5),
            phase: hash(i * 11 + 6) * std::f32::consts::TAU,
        })
        .collect()
}

pub fn node_at(n: &Node, e: &EffectSpec, window: Rect, time: f32) -> Point {
    let t = time * (0.3 + 1.7 * e.speed) * std::f32::consts::TAU;
    Point::new(
        window.min_x() + n.x * window.size.width + n.drift * (t * n.f1 + n.phase).sin(),
        window.min_y()
            + n.y * window.size.height
            + n.drift * 0.8 * (t * n.f2 + n.phase * 1.3).cos(),
    )
}

/// A constellation: its nodes and where they are this frame (kept: no allocation).
#[derive(Debug, Default)]
pub struct ConstellationPainter {
    nodes: Vec<Node>,
    at: Vec<Point>,
}

impl ConstellationPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.nodes = node_layout(e, window, scale);
        self.at = Vec::with_capacity(self.nodes.len());
    }

    pub fn paint(&mut self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        self.at.clear();
        self.at.extend(self.nodes.iter().map(|n| node_at(n, e, f.window, f.time)));
        let reach = (70.0 + 130.0 * e.size) * f.scale;
        let color = e.colors.first().copied().unwrap_or(Rgba::WHITE);
        let step = 3.0 * f.scale;
        let line = (0.8 * f.scale).max(1.0);
        for (i, a) in self.at.iter().enumerate() {
            // Links: to the next nodes within reach, at most a couple from each.
            let mut links = 0;
            for b in &self.at[i + 1..] {
                if links >= LINKS_PER_NODE {
                    break;
                }
                let d = (b.x - a.x).hypot(b.y - a.y);
                if d >= reach || d < 1.0 {
                    continue;
                }
                links += 1;
                let alpha = (e.opacity * 1.4 * (1.0 - d / reach)).min(1.0);
                let n = (d / step) as usize;
                for k in 1..n {
                    let t = k as f32 / n as f32;
                    let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                    let r = Rect::new(p.x - line, p.y - line, 2.0 * line, 2.0 * line);
                    if hidden(&r, f.covered) {
                        continue;
                    }
                    layer.quads.push(Quad::filled(
                        Rect::new(p.x - line / 2.0, p.y - line / 2.0, line, line),
                        color.with_alpha(alpha),
                    ));
                }
            }
            let d = 2.6 * f.scale;
            let r = Rect::new(a.x - d, a.y - d, 2.0 * d, 2.0 * d);
            if !hidden(&r, f.covered) {
                dot(
                    layer,
                    *a,
                    d,
                    color.mix(Rgba::WHITE, 0.3).with_alpha((e.opacity * 2.0).min(1.0)),
                );
            }
        }
    }

    pub fn memory(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.at.capacity() * std::mem::size_of::<Point>()
    }
}

// ---------------------------------------------------------------------------------------
// Synthwave.

/// The grid's lines converging to the horizon are drawn at a third of the resolution.
const GRID_DETAIL: f32 = 1.0 / 3.0;

/// The sun is drawn at half the resolution.
const SUN_DETAIL: f32 = 0.5;

/// The synthwave grid: its fan of lines, its sun; the horizontal lines are quads.
#[derive(Debug, Default)]
pub struct SynthwavePainter {
    fan: Option<ImageData>,
    sun: Option<ImageData>,
    city: Option<ImageData>,
    /// The window's width and the scale the art is for, and the band's height.
    art_key: Option<(u32, u32, u32)>,
}

impl SynthwavePainter {
    /// The band's height (physical pixels).
    pub fn band(e: &EffectSpec, scale: f32) -> f32 {
        ((110.0 + 180.0 * e.size) * scale).round()
    }

    pub fn sync_art(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        let band = Self::band(e, scale);
        let key = (window.size.width as u32, (scale * 100.0) as u32, band as u32);
        let lost = self.images().any(ImageData::is_lost);
        if self.art_key == Some(key) && !lost {
            return;
        }
        let grid = e.colors.first().copied().unwrap_or(Rgba::WHITE);
        let ground = band * 0.55;
        let (w, h) = (
            (window.size.width * GRID_DETAIL).round() as u32,
            (ground * GRID_DETAIL).round() as u32,
        );
        self.fan = Some(image(art::grid_fan(w, h, 26, grid, (1.1 * scale * GRID_DETAIL).max(0.6))));
        self.sun = (e.style == "sun").then(|| {
            let top = e.colors.get(1).copied().unwrap_or(SUN_TOP);
            let bottom = e.colors.get(2).copied().unwrap_or(SUN_BOTTOM);
            // Half the resolution, stretched into place (its edge stays soft and smooth).
            image(art::sun(band * 0.42 * SUN_DETAIL, top, bottom))
        });
        self.city = (e.style == "city").then(|| {
            // Dark buildings tinted by the second color, their windows lit in the first.
            let body = e.colors.get(1).copied().unwrap_or(grid).mix(Rgba::BLACK, 0.8);
            image(art::skyline(
                w,
                ((Self::city_height(band)) * GRID_DETAIL).round() as u32,
                body,
                grid,
                17,
            ))
        });
        self.art_key = Some(key);
    }

    /// The skyline's height (physical pixels), for the band's.
    pub fn city_height(band: f32) -> f32 {
        (band * 0.34).round()
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let band = Self::band(e, f.scale);
        let w = f.window;
        let horizon = (w.max_y() - band * 0.55).round();
        if let Some(city) = &self.city {
            let h = Self::city_height(band);
            let r = Rect::new(w.min_x(), horizon - h, w.size.width, h);
            if !hidden(&r, f.covered) {
                push(layer, city, r, (e.opacity * 2.4).min(1.0));
            }
        }
        let ground = w.max_y() - horizon;
        if let Some(sun) = &self.sun {
            let (sw, sh) = (sun.width as f32 / SUN_DETAIL, sun.height as f32 / SUN_DETAIL);
            // Sitting on the horizon, its lower part behind it.
            let r = Rect::new(
                (w.min_x() + (w.size.width - sw) / 2.0).round(),
                horizon - sh * 0.62,
                sw,
                sh,
            );
            if !hidden(&r, f.covered) {
                push(layer, sun, r, (e.opacity * 1.8).min(1.0));
            }
        }
        if let Some(fan) = &self.fan {
            let r = Rect::new(w.min_x(), horizon, w.size.width, ground);
            push(layer, fan, r, (e.opacity * 2.0).min(1.0));
        }
        // The horizon, and lines coming toward you from it.
        let grid = e.colors.first().copied().unwrap_or(Rgba::WHITE);
        let line = (1.0 * f.scale).round().max(1.0);
        layer.quads.push(Quad::filled(
            Rect::new(w.min_x(), horizon, w.size.width, line),
            grid.mix(Rgba::WHITE, 0.3 * e.glow).with_alpha((e.opacity * 2.0).min(1.0)),
        ));
        // Its glow: a few wider, fainter bands round the line (still, so never redrawn).
        if e.glow > 0.0 {
            for (reach, strength) in [(3.0, 0.22), (7.0, 0.1), (14.0, 0.05)] {
                let h = (reach * f.scale).round();
                let a = (e.opacity * 2.0 * strength * e.glow).min(1.0);
                if a < 0.004 {
                    continue;
                }
                layer.quads.push(Quad::filled(
                    Rect::new(w.min_x(), horizon - h, w.size.width, 2.0 * h + line),
                    grid.with_alpha(a),
                ));
            }
        }
        const LINES: usize = 9;
        let flow = f.time * (0.08 + 0.5 * e.speed);
        for k in 0..LINES {
            let z = ((k as f32 + flow) / LINES as f32).rem_euclid(1.0);
            let y = horizon + ground * z.powf(2.2);
            let r = Rect::new(w.min_x(), y.round(), w.size.width, line);
            let alpha = (e.opacity * 2.0 * (0.25 + 0.75 * z)).min(1.0);
            layer.quads.push(Quad::filled(r, grid.with_alpha(alpha)));
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.fan.iter().chain(&self.sun).chain(&self.city)
    }
}

// ---------------------------------------------------------------------------------------
// Lightning.

/// How long a strike lights the sky (seconds).
pub const STRIKE_LIFE: f32 = 0.8;

/// The time between strikes (seconds), for a density.
pub fn strike_period(density: f32) -> f32 {
    15.0 - 11.0 * density.clamp(0.0, 1.0)
}

/// Strike `k`'s start (seconds): placed so a still frame catches one lit.
pub fn strike_start(k: i64, period: f32) -> f32 {
    let shift = (ui_components::decorations::STATIC_TIME - 0.36).rem_euclid(period);
    k as f32 * period + shift
}

/// How bright a strike is `age` seconds in: two or three flickers, each fading fast.
pub fn strike_level(age: f32) -> f32 {
    if !(0.0..STRIKE_LIFE).contains(&age) {
        return 0.0;
    }
    [(0.0, 1.0), (0.16, 0.55), (0.34, 0.8)]
        .iter()
        .filter(|(at, _)| age >= *at)
        .map(|(at, peak)| peak * (-(age - at) / 0.07).exp())
        .fold(0.0, f32::max)
}

/// The sky's fade is this many texels tall (stretched over most of the window).
const SKY_TEXELS: u32 = 64;
/// A strike's halo is drawn this many texels across, stretched.
const HALO_TEXELS: f32 = 40.0;

/// Lightning: the sky's fade and the halo where the bolt starts.
#[derive(Debug, Default)]
pub struct LightningPainter {
    sky: Option<ImageData>,
    halo: Option<ImageData>,
    art_key: Option<u32>,
}

impl LightningPainter {
    pub fn sync_art(&mut self, e: &EffectSpec) {
        let flash = e.colors.get(1).or(e.colors.first()).copied().unwrap_or(Rgba::WHITE);
        // The same at every scale (stretched): made once.
        let key = 1;
        let lost = self.sky.iter().chain(&self.halo).any(ImageData::is_lost);
        if self.art_key == Some(key) && !lost {
            return;
        }
        self.sky = Some(image(art::sky_fade(SKY_TEXELS, flash)));
        self.halo = Some(image(art::glow_dot(HALO_TEXELS / 2.0, flash)));
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        if e.density <= 0.0 || e.colors.is_empty() {
            return;
        }
        let period = strike_period(e.density);
        let now = ((f.time - strike_start(0, period)) / period).floor() as i64;
        let age = f.time - strike_start(now, period);
        let level = strike_level(age);
        if level < 0.01 {
            return;
        }
        let w = f.window;
        let kk = now.rem_euclid(1 << 20) as u32;
        let x0 = w.min_x() + (0.12 + 0.76 * hash(kk * 13 + 1)) * w.size.width;
        // The sky, lit from the top.
        if let Some(sky) = &self.sky {
            let r = Rect::new(w.min_x(), w.min_y(), w.size.width, w.size.height * 0.8);
            push(layer, sky, r, (e.opacity * (0.35 + 0.6 * e.glow) * level).min(1.0));
        }
        if let Some(halo) = &self.halo {
            let d = (160.0 + 200.0 * e.size) * f.scale;
            let r = Rect::new(x0 - d / 2.0, w.min_y() - d * 0.3, d, d);
            if !hidden(&r, f.covered) {
                push(layer, halo, r, (e.opacity * 1.4 * e.glow * level).min(1.0));
            }
        }
        if e.style == "flash" {
            return;
        }
        // The bolt: a walk down from the top, two branches off it.
        let color = e.colors[0].mix(Rgba::WHITE, 0.45);
        let alpha = (e.opacity * 2.2 * level).min(1.0);
        let depth = (0.32 + 0.4 * e.size) * w.size.height;
        let core = (1.1 + 1.6 * e.size) * f.scale;
        const SEGMENTS: u32 = 18;
        let mut at = Point::new(x0, w.min_y());
        let lean = if hash(kk * 13 + 2) < 0.5 { -1.0 } else { 1.0 };
        let mut forks = [(Point::default(), 0.0f32); 2];
        for j in 0..SEGMENTS {
            let step = depth / SEGMENTS as f32;
            let dy = step * (0.7 + 0.6 * hash(kk * 97 + j * 3));
            let dx = (hash(kk * 97 + j * 3 + 1) - 0.5) * step * 1.7 + lean * step * 0.15;
            let next = Point::new(at.x + dx, at.y + dy);
            let taper = 1.0 - 0.55 * j as f32 / SEGMENTS as f32;
            bolt_segment(layer, f, (at, next), core * taper, color, alpha, e.glow);
            if j == 4 || j == 9 {
                forks[(j / 9) as usize] = (next, -lean * (0.6 + 0.6 * hash(kk * 97 + j)));
            }
            at = next;
        }
        for (n, (start, bend)) in forks.into_iter().enumerate() {
            let mut at = start;
            for j in 0..7u32 {
                let step = depth / SEGMENTS as f32 * 0.8;
                let seed = kk * 131 + n as u32 * 17 + j * 3;
                let dy = step * (0.6 + 0.5 * hash(seed));
                let dx = bend * step * 0.8 + (hash(seed + 1) - 0.5) * step * 1.2;
                let next = Point::new(at.x + dx, at.y + dy);
                let taper = 0.55 * (1.0 - j as f32 / 8.0);
                bolt_segment(layer, f, (at, next), core * taper, color, alpha * 0.8, e.glow);
                at = next;
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sky.iter().chain(&self.halo)
    }
}

/// A piece of a bolt: dots along it, a faint wider glow every few.
fn bolt_segment(
    layer: &mut Layer,
    f: &FxFrame<'_>,
    (a, b): (Point, Point),
    d: f32,
    color: Rgba,
    alpha: f32,
    glow: f32,
) {
    let d = d.max(1.0);
    let len = (b.x - a.x).hypot(b.y - a.y);
    let step = (d * 0.6).max(1.0);
    let n = ((len / step).ceil() as usize).clamp(1, 64);
    for i in 0..n {
        let t = i as f32 / n as f32;
        let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        let r = Rect::new(p.x - 3.0 * d, p.y - 3.0 * d, 6.0 * d, 6.0 * d);
        if hidden(&r, f.covered) {
            continue;
        }
        if i % 3 == 0 && glow > 0.0 {
            dot(layer, p, d * 5.0, color.with_alpha(alpha * 0.1 * glow));
        }
        dot(layer, p, d, color.with_alpha(alpha));
    }
}

// ---------------------------------------------------------------------------------------
// Raindrops.

/// A drop on the glass: where it sits, how far it slides, its pace, its size (0..3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drop {
    pub x: f32,
    pub y: f32,
    pub travel: f32,
    pub period: f32,
    pub phase: f32,
    pub size: u8,
    pub color: u8,
}

pub const MAX_DROPS: usize = 90;
/// The dots of a drop's trail.
pub const TRAIL: usize = 8;

pub fn drop_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Drop> {
    let n = ((e.density * 46.0 * area(window, scale).sqrt()).round() as usize).min(MAX_DROPS);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Drop {
            x: hash(i * 7 + 1),
            y: 0.05 + 0.6 * hash(i * 7 + 2),
            travel: 0.15 + 0.35 * hash(i * 7 + 3),
            period: 7.0 + 8.0 * hash(i * 7 + 4),
            phase: hash(i * 7 + 5),
            size: match hash(i * 7 + 6) {
                v if v < 0.5 => 0,
                v if v < 0.85 => 1,
                _ => 2,
            },
            color: (i % colors) as u8,
        })
        .collect()
}

/// A drop at `time`: its middle, where its slide began (its trail's top), how visible.
pub fn drop_at(d: &Drop, e: &EffectSpec, window: Rect, scale: f32, time: f32) -> (Point, f32, f32) {
    let period = d.period / (0.4 + 1.2 * e.speed);
    let t = (time / period + d.phase).rem_euclid(1.0);
    let x = window.min_x() + d.x * window.size.width;
    let y0 = window.min_y() + d.y * window.size.height;
    let fade_in = (t / 0.08).min(1.0);
    let fade_out = ((1.0 - t) / 0.12).min(1.0);
    let alpha = fade_in * fade_out;
    if t < 0.45 {
        return (Point::new(x, y0), y0, alpha);
    }
    let p = (t - 0.45) / 0.55;
    let fall = d.travel * window.size.height * p.powf(1.7);
    let wobble = (p * 9.0 + d.phase * 6.0).sin() * 1.5 * scale * p;
    (Point::new(x + wobble, y0 + fall), y0, alpha)
}

#[derive(Debug, Default)]
pub struct RaindropPainter {
    drops: Vec<Drop>,
    /// Per color, one per size.
    sprites: Vec<ImageData>,
    art_key: Option<(u32, u32)>,
}

impl RaindropPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.drops = drop_layout(e, window, scale);
    }

    /// A drop's radius at size 1 (physical pixels).
    fn radius(e: &EffectSpec, scale: f32) -> f32 {
        ((2.2 + 3.6 * e.size) * scale).round()
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let key = (Self::radius(e, scale) as u32, e.colors.len() as u32);
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        let r = Self::radius(e, scale);
        self.sprites = e
            .colors
            .iter()
            .flat_map(|c| [0.7, 1.0, 1.4].map(|k| image(art::raindrop(r * k, *c))))
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let r = Self::radius(e, f.scale);
        for d in &self.drops {
            let Some(sprite) = self.sprites.get(usize::from(d.color) * 3 + usize::from(d.size))
            else {
                continue;
            };
            let (p, top, alpha) = drop_at(d, e, f.window, f.scale, f.time);
            if alpha <= 0.01 {
                continue;
            }
            let (w, h) = (sprite.width as f32, sprite.height as f32);
            // The sprite's round part is near its bottom: the drop's middle there.
            let bounds = Rect::new(p.x - w / 2.0, p.y - h + w / 2.0, w, h);
            // Its trail: small dots from where it started to just above it.
            let slid = p.y - top;
            let c = e.colors.get(usize::from(d.color)).copied().unwrap_or(Rgba::WHITE);
            if slid > 2.0 * r {
                for k in 0..TRAIL {
                    let t = (k as f32 + 1.0) / (TRAIL as f32 + 1.0);
                    let q = Point::new(p.x, top + (slid - r) * t);
                    let size = r * (0.35 + 0.3 * t);
                    let rect = Rect::new(q.x - size, q.y - size, 2.0 * size, 2.0 * size);
                    if hidden(&rect, f.covered) {
                        continue;
                    }
                    dot(layer, q, size, c.with_alpha((e.opacity * 1.3 * t * alpha).min(1.0)));
                }
            }
            if !hidden(&bounds, f.covered) {
                push(layer, sprite, bounds, (e.opacity * 2.0 * alpha).min(1.0));
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.drops.capacity() * std::mem::size_of::<Drop>()
    }
}

// ---------------------------------------------------------------------------------------
// Clouds.

/// A cloud: where along its way it starts, how high, how near (0: far — small, faint,
/// slow), its shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cloud {
    pub x: f32,
    pub y: f32,
    pub depth: f32,
    pub shape: u8,
}

pub const MAX_CLOUDS: usize = 12;
/// A cloud's shapes.
pub const CLOUD_SHAPES: usize = 3;
/// Logical pixels per texel of a cloud's image (soft: a quarter of the resolution at 2×).
const CLOUD_TEXEL: f32 = 2.0;

/// The clouds, the far ones first.
pub fn cloud_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Cloud> {
    if e.density <= 0.0 {
        return Vec::new();
    }
    let wide = window.size.width / scale / 1200.0;
    let n = (((1.0 + 6.0 * e.density) * wide).round() as usize).clamp(1, MAX_CLOUDS);
    let mut clouds: Vec<Cloud> = (0..n as u32)
        .map(|i| Cloud {
            x: (i as f32 + 0.6 * hash(i * 5 + 1)) / n as f32,
            y: hash(i * 5 + 2),
            depth: hash(i * 5 + 3),
            shape: (i % CLOUD_SHAPES as u32) as u8,
        })
        .collect();
    clouds.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    clouds
}

/// A cloud's shape `i`'s size near (logical pixels).
fn cloud_size(e: &EffectSpec, i: usize) -> (f32, f32) {
    let w = (200.0 + 240.0 * e.size) * [1.0, 0.8, 1.2][i % CLOUD_SHAPES];
    (w, w * 0.42)
}

/// Cloud `c` at `time`: its bounds (whole physical pixels: when it moved by less, the
/// frame is the same) and its opacity.
pub fn cloud_at(c: &Cloud, e: &EffectSpec, window: Rect, scale: f32, time: f32) -> (Rect, f32) {
    let (w, h) = cloud_size(e, usize::from(c.shape));
    let k = 0.6 + 0.4 * c.depth;
    let (w, h) = (w * k * scale, h * k * scale);
    let speed = (3.0 + 16.0 * e.speed) * scale * (0.45 + 0.55 * c.depth);
    let span = window.size.width + w;
    let x = window.min_x() - w + (c.x * span + time * speed).rem_euclid(span);
    let y = window.min_y() - h * 0.3 + c.y * window.size.height * 0.12;
    (Rect::new(x.round(), y.round(), w.round(), h.round()), (0.55 + 0.45 * c.depth))
}

#[derive(Debug, Default)]
pub struct CloudPainter {
    clouds: Vec<Cloud>,
    sprites: Vec<ImageData>,
    art_key: Option<u32>,
}

impl CloudPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.clouds = cloud_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec) {
        let top = e.colors.first().copied().unwrap_or(Rgba::WHITE);
        let under = e.colors.get(1).copied().unwrap_or(top);
        let key = (e.size * 100.0) as u32;
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = (0..CLOUD_SHAPES)
            .map(|i| {
                let (w, h) = cloud_size(e, i);
                let (tw, th) = ((w / CLOUD_TEXEL).round() as u32, (h / CLOUD_TEXEL).round() as u32);
                image(art::cloud(tw, th, top, under, i as u32 + 1))
            })
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        for c in &self.clouds {
            let Some(sprite) = self.sprites.get(usize::from(c.shape)) else { continue };
            let (r, level) = cloud_at(c, e, f.window, f.scale, f.time);
            if !hidden(&r, f.covered) {
                push(layer, sprite, r, (e.opacity * 2.0 * level).min(1.0));
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.clouds.capacity() * std::mem::size_of::<Cloud>()
    }
}

// ---------------------------------------------------------------------------------------
// Fog.

/// Logical pixels per texel of the fog's bands (very soft: stretched far).
const FOG_TEXEL: f32 = 2.0;

/// The fog: one to three bands, each an image a period wide, repeated, drifting.
#[derive(Debug, Default)]
pub struct FogPainter {
    tiles: Vec<ImageData>,
    /// Each band's size (logical pixels).
    sizes: Vec<(f32, f32)>,
    art_key: Option<(u32, usize, bool)>,
}

impl FogPainter {
    /// How many bands (1–3), for a density.
    pub fn count(e: &EffectSpec) -> usize {
        1 + (e.density.clamp(0.0, 1.0) * 2.0).round() as usize
    }

    pub fn sync_art(&mut self, e: &EffectSpec) {
        let n = Self::count(e);
        let ground = e.style != "veil";
        let key = ((e.size * 100.0) as u32, n, ground);
        if self.art_key == Some(key) && !self.tiles.iter().any(ImageData::is_lost) {
            return;
        }
        self.sizes = (0..n)
            .map(|i| (520.0 + 140.0 * i as f32, (70.0 + 170.0 * e.size) * (1.0 - 0.15 * i as f32)))
            .collect();
        self.tiles = self
            .sizes
            .iter()
            .enumerate()
            .map(|(i, (w, h))| {
                let c = e.colors[i % e.colors.len().max(1)];
                let (tw, th) = ((w / FOG_TEXEL).round() as u32, (h / FOG_TEXEL).round() as u32);
                image(art::fog_band(tw, th, c, i as u32 + 1, ground))
            })
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let n = self.tiles.len();
        let veil = e.style == "veil";
        for (i, (tile, (w, h))) in self.tiles.iter().zip(&self.sizes).enumerate() {
            let (w, h) = (w * f.scale, h * f.scale);
            let pace = (3.0 + 16.0 * e.speed) * f.scale * (1.0 - 0.2 * i as f32);
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            // Whole pixels: when it drifted by less, the frame is the same.
            let offset = (f.time * pace * dir + i as f32 * w * 0.37).rem_euclid(w).round();
            let y = if veil {
                f.window.min_y() + f.window.size.height * (0.12 + 0.7 * (i as f32 + 0.5) / n as f32)
                    - h / 2.0
            } else {
                f.window.max_y() - h + i as f32 * h * 0.12
            };
            let opacity = (e.opacity * 2.0 * (1.0 - 0.2 * i as f32)).min(1.0);
            let mut x = f.window.min_x() - offset;
            while x < f.window.max_x() {
                let r = Rect::new(x, y.round(), w, h.round());
                if !hidden(&r, f.covered) {
                    push(layer, tile, r, opacity);
                }
                x += w;
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.tiles.iter()
    }
}

// ---------------------------------------------------------------------------------------
// Butterflies.
