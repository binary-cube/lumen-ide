//! Butterflies, balloons, hearts, bats and lava.

use super::*;

/// A balloon's string: a light grey, whatever the theme.
const STRING: Rgba = Rgba::rgb(0x9aa0aa);

/// A butterfly: its home, how far it roams, how long it rests and flies, its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Butterfly {
    pub x: f32,
    pub y: f32,
    pub rest: f32,
    pub flight: f32,
    pub phase: f32,
    pub color: u8,
    pub seed: u32,
}

pub const MAX_BUTTERFLIES: usize = 28;
/// The frames of a flap: wings open, three steps closing.
pub const WING_FRAMES: usize = 4;

pub fn butterfly_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Butterfly> {
    let n = ((e.density * 16.0 * area(window, scale).sqrt()).round() as usize).min(MAX_BUTTERFLIES);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Butterfly {
            x: 0.08 + 0.84 * hash(i * 11 + 1),
            y: 0.08 + 0.84 * hash(i * 11 + 2),
            rest: 2.0 + 3.0 * hash(i * 11 + 3),
            flight: 1.4 + 1.4 * hash(i * 11 + 4),
            phase: hash(i * 11 + 5),
            color: (i % colors) as u8,
            seed: i,
        })
        .collect()
}

/// A butterfly at `time`: its middle and the frame of its wings.
pub fn butterfly_at(
    b: &Butterfly,
    e: &EffectSpec,
    window: Rect,
    scale: f32,
    time: f32,
) -> (Point, usize) {
    let pace = 0.5 + 1.2 * e.speed;
    let cycle = b.rest + b.flight;
    let t = time * pace + b.phase * cycle;
    let c = (t / cycle).floor();
    let local = t - c * cycle;
    let roam = (0.12 * window.size.width).min(260.0 * scale);
    let spot = |c: f32| {
        let k = (c as i64).rem_euclid(1 << 16) as u32;
        let x = b.x * window.size.width + roam * (2.0 * hash(b.seed * 7919 + k * 2) - 1.0);
        let y =
            b.y * window.size.height + roam * 0.7 * (2.0 * hash(b.seed * 7919 + k * 2 + 1) - 1.0);
        Point::new(
            window.min_x() + x.clamp(0.0, window.size.width),
            window.min_y() + y.clamp(0.0, window.size.height),
        )
    };
    if local < b.rest {
        // Resting: now and then its wings close slowly.
        let frame = if (time * 1.6 + b.phase * 9.0).sin() > 0.55 { 1 } else { 0 };
        return (spot(c), frame);
    }
    let p = (local - b.rest) / b.flight;
    let ease = p * p * (3.0 - 2.0 * p);
    let (a, z) = (spot(c), spot(c + 1.0));
    let arc = (p * std::f32::consts::PI).sin() * 40.0 * scale;
    let flutter = (time * 9.0 + b.phase * 20.0).sin() * 3.0 * scale;
    let at = Point::new(a.x + (z.x - a.x) * ease, a.y + (z.y - a.y) * ease - arc + flutter);
    const FLAP: [usize; 6] = [0, 1, 2, 3, 2, 1];
    let k = ((time * 18.0 + b.phase * 6.0).floor() as i64).rem_euclid(6) as usize;
    (at, FLAP[k])
}

#[derive(Debug, Default)]
pub struct ButterflyPainter {
    flock: Vec<Butterfly>,
    /// Per color, its frames.
    sprites: Vec<ImageData>,
    art_key: Option<(u32, u32)>,
}

impl ButterflyPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.flock = butterfly_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let span = ((16.0 + 18.0 * e.size) * scale).round();
        let key = (span as u32, e.colors.len() as u32);
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e
            .colors
            .iter()
            .flat_map(|c| [1.0, 0.7, 0.4, 0.12].map(|open| image(art::butterfly(span, *c, open))))
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        for b in &self.flock {
            let (p, frame) = butterfly_at(b, e, f.window, f.scale, f.time);
            let Some(sprite) = self.sprites.get(usize::from(b.color) * WING_FRAMES + frame) else {
                continue;
            };
            let (w, h) = (sprite.width as f32, sprite.height as f32);
            let r = Rect::new((p.x - w / 2.0).round(), (p.y - h / 2.0).round(), w, h);
            if !hidden(&r, f.covered) {
                push(layer, sprite, r, (e.opacity * 2.0).min(1.0));
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.flock.capacity() * std::mem::size_of::<Butterfly>()
    }
}

// ---------------------------------------------------------------------------------------
// Balloons.

/// A balloon: where it rises, how fast, how it sways, its color and size (0, 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Balloon {
    pub x: f32,
    pub y: f32,
    pub rise: f32,
    pub sway: f32,
    pub phase: f32,
    pub color: u8,
    pub size: u8,
}

pub const MAX_BALLOONS: usize = 36;

pub fn balloon_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Balloon> {
    let n = ((e.density * 24.0 * area(window, scale).sqrt()).round() as usize).min(MAX_BALLOONS);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Balloon {
            x: 0.04 + 0.92 * hash(i * 9 + 1),
            y: hash(i * 9 + 2),
            rise: 0.7 + 0.6 * hash(i * 9 + 3),
            sway: (6.0 + 12.0 * hash(i * 9 + 4)) * scale,
            phase: hash(i * 9 + 5) * std::f32::consts::TAU,
            color: (hash(i * 9 + 6) * colors as f32) as u8 % colors as u8,
            size: u8::from(hash(i * 9 + 7) < 0.4),
        })
        .collect()
}

/// A balloon's body radius at size 0 (physical pixels).
fn balloon_radius(e: &EffectSpec, scale: f32) -> f32 {
    ((10.0 + 10.0 * e.size) * scale).round()
}

/// A balloon at `time`: its body's middle and how far its string swings.
pub fn balloon_at(
    b: &Balloon,
    e: &EffectSpec,
    window: Rect,
    scale: f32,
    time: f32,
) -> (Point, f32) {
    let r = balloon_radius(e, scale);
    let margin = r * 4.0;
    let travel = window.size.height + 2.0 * margin;
    let speed = (10.0 + 40.0 * e.speed) * scale * b.rise;
    let up = (b.y * travel + time * speed).rem_euclid(travel);
    let swing = (time * 0.55 * b.rise + b.phase).sin();
    let x = window.min_x() + b.x * window.size.width + b.sway * swing;
    (Point::new(x, window.max_y() + margin - up), swing)
}

#[derive(Debug, Default)]
pub struct BalloonPainter {
    balloons: Vec<Balloon>,
    /// Per color, the big one and the small one.
    sprites: Vec<ImageData>,
    art_key: Option<(u32, u32)>,
}

impl BalloonPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.balloons = balloon_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let r = balloon_radius(e, scale);
        let key = (r as u32, e.colors.len() as u32);
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e
            .colors
            .iter()
            .flat_map(|c| [1.0, 0.78].map(|k| image(art::balloon((r * k).round(), *c))))
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let alpha = (e.opacity * 2.0).min(1.0);
        let string = STRING.with_alpha((e.opacity * 1.4).min(1.0));
        let r0 = balloon_radius(e, f.scale);
        for b in &self.balloons {
            let Some(sprite) = self.sprites.get(usize::from(b.color) * 2 + usize::from(b.size))
            else {
                continue;
            };
            let (p, swing) = balloon_at(b, e, f.window, f.scale, f.time);
            let r = if b.size == 0 { r0 } else { (r0 * 0.78).round() };
            let (w, h) = (sprite.width as f32, sprite.height as f32);
            let body = Rect::new((p.x - w / 2.0).round(), (p.y - r * 1.2 - 1.5).round(), w, h);
            // The string, swinging behind the balloon's sway.
            let knot = body.max_y() - 1.0;
            let length = r * 2.6;
            let step = 1.6 * f.scale;
            let n = ((length / step) as usize).min(60);
            let thin = (0.8 * f.scale).max(1.0);
            for k in 0..n {
                let t = k as f32 / n as f32;
                let wave = (t * 5.0 + f.time * 2.0 + b.phase).sin() * 2.5 * f.scale * t;
                let q = Point::new(p.x - swing * t * r * 0.5 + wave, knot + t * length);
                let rect = Rect::new(q.x - thin, q.y - thin, 2.0 * thin, 2.0 * thin);
                if !hidden(&rect, f.covered) {
                    layer
                        .quads
                        .push(Quad::filled(Rect::new(q.x - thin / 2.0, q.y, thin, step), string));
                }
            }
            if !hidden(&body, f.covered) {
                push(layer, sprite, body, alpha);
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.balloons.capacity() * std::mem::size_of::<Balloon>()
    }
}

// ---------------------------------------------------------------------------------------
// Hearts.

/// A heart: where it floats up, how fast, how it sways, its beat, its color and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heart {
    pub x: f32,
    pub y: f32,
    pub rise: f32,
    pub sway: f32,
    pub phase: f32,
    pub beat: f32,
    pub color: u8,
    pub size: u8,
}

pub const MAX_HEARTS: usize = 40;

pub fn heart_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Heart> {
    let n = ((e.density * 22.0 * area(window, scale).sqrt()).round() as usize).min(MAX_HEARTS);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Heart {
            x: 0.04 + 0.92 * hash(i * 13 + 1),
            y: hash(i * 13 + 2),
            rise: 0.6 + 0.8 * hash(i * 13 + 3),
            sway: (5.0 + 14.0 * hash(i * 13 + 4)) * scale,
            phase: hash(i * 13 + 5),
            beat: hash(i * 13 + 6),
            color: (i % colors) as u8,
            size: u8::from(hash(i * 13 + 7) < 0.5),
        })
        .collect()
}

/// How a heart beats at `time` (0..1, two quick swells a beat).
pub fn heart_beat(h: &Heart, time: f32) -> f32 {
    let p = (time * (0.6 + 0.4 * h.beat) + h.phase).rem_euclid(1.0);
    let bump = |at: f32| (-((p - at) / 0.06).powi(2)).exp();
    bump(0.05).max(0.65 * bump(0.24))
}

/// A heart at `time`: its middle, its size now (1: its full size, smaller between beats)
/// and how visible (fading in at the bottom, out near the top).
pub fn heart_at(
    h: &Heart,
    e: &EffectSpec,
    window: Rect,
    scale: f32,
    time: f32,
) -> (Point, f32, f32) {
    let margin = 40.0 * scale;
    let travel = window.size.height + 2.0 * margin;
    let speed = (6.0 + 26.0 * e.speed) * scale * h.rise;
    let up = (h.y * travel + time * speed).rem_euclid(travel);
    let left = up / travel;
    let x = window.min_x()
        + h.x * window.size.width
        + h.sway * (time * 0.7 * h.rise + h.phase * std::f32::consts::TAU).sin();
    let alpha = (left / 0.1).min(1.0) * ((1.0 - left) / 0.3).min(1.0);
    let k = 1.0 - 0.14 * (0.4 + 0.6 * e.glow) * (1.0 - heart_beat(h, time));
    (Point::new(x, window.max_y() + margin - up), k, alpha)
}

#[derive(Debug, Default)]
pub struct HeartPainter {
    hearts: Vec<Heart>,
    /// Per color, the big one and the small one.
    sprites: Vec<ImageData>,
    art_key: Option<(u32, u32, u32)>,
}

impl HeartPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.hearts = heart_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let size = ((16.0 + 20.0 * e.size) * scale).round();
        let key = (size as u32, e.colors.len() as u32, (e.glow * 100.0) as u32);
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e
            .colors
            .iter()
            .flat_map(|c| [1.0, 0.7].map(|k| image(art::heart(size * k, *c, e.glow))))
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        for h in &self.hearts {
            let Some(sprite) = self.sprites.get(usize::from(h.color) * 2 + usize::from(h.size))
            else {
                continue;
            };
            let (p, k, alpha) = heart_at(h, e, f.window, f.scale, f.time);
            if alpha <= 0.01 {
                continue;
            }
            let (w, hh) = (sprite.width as f32 * k, sprite.height as f32 * k);
            let r = Rect::new(p.x - w / 2.0, p.y - hh / 2.0, w, hh);
            if !hidden(&r, f.covered) {
                push(layer, sprite, r, (e.opacity * 2.0 * alpha).min(1.0));
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.hearts.capacity() * std::mem::size_of::<Heart>()
    }
}

// ---------------------------------------------------------------------------------------
// Bats.

/// A bat: its crossings (how often, which way), how it zigzags, its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bat {
    pub every: f32,
    pub phase: f32,
    pub zig: f32,
    pub left: bool,
    pub color: u8,
    pub seed: u32,
}

pub const MAX_BATS: usize = 20;
/// The frames of a flap: wings up, level, down.
pub const BAT_FRAMES: usize = 3;

pub fn bat_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Bat> {
    if e.density <= 0.0 {
        return Vec::new();
    }
    let n = ((e.density * 16.0 * area(window, scale).sqrt()).round() as usize).clamp(1, MAX_BATS);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Bat {
            every: 1.4 + 1.2 * hash(i * 17 + 1),
            phase: hash(i * 17 + 2),
            zig: 0.6 + 0.8 * hash(i * 17 + 3),
            left: hash(i * 17 + 4) < 0.5,
            color: (i % colors) as u8,
            seed: i,
        })
        .collect()
}

/// How long a bat takes to cross (seconds), for a speed.
pub fn bat_crossing(speed: f32) -> f32 {
    9.0 - 5.0 * speed.clamp(0.0, 1.0)
}

/// A bat at `time`: its middle and its wings' frame (none: between its crossings).
pub fn bat_at(
    b: &Bat,
    e: &EffectSpec,
    window: Rect,
    scale: f32,
    time: f32,
) -> Option<(Point, usize)> {
    let cross = bat_crossing(e.speed);
    let cycle = cross * b.every;
    let t = time + b.phase * cycle;
    let c = (t / cycle).floor();
    let local = t - c * cycle;
    if local > cross {
        return None;
    }
    let p = local / cross;
    let margin = 60.0 * scale;
    let span = window.size.width + 2.0 * margin;
    let x = if b.left { -margin + span * p } else { window.size.width + margin - span * p };
    let k = (c as i64).rem_euclid(1 << 16) as u32;
    let lane = 0.08 + 0.55 * hash(b.seed * 31 + k);
    let amp = 50.0 * scale * b.zig;
    let tau = std::f32::consts::TAU;
    // Flitting: a wavy path with sudden jinks (a triangle wave on top).
    let tri = 2.0 * ((p * 6.0 * b.zig + b.phase).rem_euclid(1.0) - 0.5).abs() - 0.5;
    let y =
        lane * window.size.height + amp * (p * tau * 1.4 + b.phase * tau).sin() + amp * 0.6 * tri;
    const FLAP: [usize; 4] = [0, 1, 2, 1];
    let frame = ((time * (9.0 + 6.0 * e.speed) + b.phase * 4.0).floor() as i64).rem_euclid(4);
    Some((Point::new(window.min_x() + x, window.min_y() + y), FLAP[frame as usize]))
}

#[derive(Debug, Default)]
pub struct BatPainter {
    bats: Vec<Bat>,
    /// Per color, its frames.
    sprites: Vec<ImageData>,
    art_key: Option<(u32, u32)>,
}

impl BatPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.bats = bat_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let span = ((22.0 + 26.0 * e.size) * scale).round();
        let key = (span as u32, e.colors.len() as u32);
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e
            .colors
            .iter()
            .flat_map(|c| [0.9, 0.1, -0.7].map(|lift| image(art::bat(span, *c, lift))))
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        for b in &self.bats {
            let Some((p, frame)) = bat_at(b, e, f.window, f.scale, f.time) else { continue };
            let Some(sprite) = self.sprites.get(usize::from(b.color) * BAT_FRAMES + frame) else {
                continue;
            };
            let (w, h) = (sprite.width as f32, sprite.height as f32);
            let r = Rect::new((p.x - w / 2.0).round(), (p.y - h / 2.0).round(), w, h);
            if !hidden(&r, f.covered) {
                push(layer, sprite, r, (e.opacity * 2.0).min(1.0));
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.bats.capacity() * std::mem::size_of::<Bat>()
    }
}

// ---------------------------------------------------------------------------------------
// Lava lamp.

/// A blob of wax: where it rises and sinks, how big, how fast, its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blob {
    pub x: f32,
    pub r: f32,
    pub pace: f32,
    pub phase: f32,
    pub drift: f32,
    pub color: u8,
}

pub const MAX_BLOBS: usize = 16;
/// The blob's image, in texels (stretched to every size: soft and round at any).
const BLOB_TEXELS: u32 = 48;

pub fn blob_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Blob> {
    if e.density <= 0.0 {
        return Vec::new();
    }
    let wide = (window.size.width / scale / 1200.0).sqrt();
    let n = (((3.0 + 9.0 * e.density) * wide).round() as usize).clamp(2, MAX_BLOBS);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Blob {
            x: (i as f32 + 0.2 + 0.6 * hash(i * 19 + 1)) / n as f32,
            r: 0.5 + 0.6 * hash(i * 19 + 2),
            pace: 0.7 + 0.6 * hash(i * 19 + 3),
            phase: hash(i * 19 + 4) * std::f32::consts::TAU,
            drift: 40.0 + 60.0 * hash(i * 19 + 5),
            // Neighbors share a color in pairs: they meet, merge and part.
            color: ((i / 2) % colors) as u8,
        })
        .collect()
}

/// A blob at `time`: its middle and radius (whole pixels: a frame where it moved by less
/// is the same frame).
pub fn blob_at(b: &Blob, e: &EffectSpec, window: Rect, scale: f32, time: f32) -> (Point, f32) {
    let w = (0.008 + 0.04 * e.speed) * std::f32::consts::TAU * b.pace;
    let r = (40.0 + 70.0 * e.size) * scale * b.r * (1.0 + 0.08 * (time * w * 1.7 + b.phase).sin());
    let y = window.min_y() + window.size.height * (0.5 - 0.42 * (time * w + b.phase).sin());
    let x = window.min_x()
        + window.size.width * b.x
        + b.drift * scale * (time * w * 0.6 + b.phase * 2.0).sin();
    (Point::new(x.round(), y.round()), r.round())
}

/// A lava lamp: its blobs, where they are this frame (kept: no allocation), one image per
/// color.
#[derive(Debug, Default)]
pub struct LavaPainter {
    blobs: Vec<Blob>,
    at: Vec<(Point, f32)>,
    sprites: Vec<ImageData>,
    art_key: Option<u32>,
}

impl LavaPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.blobs = blob_layout(e, window, scale);
        self.at = Vec::with_capacity(self.blobs.len());
    }

    pub fn sync_art(&mut self, e: &EffectSpec) {
        let key = e.colors.len() as u32;
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e.colors.iter().map(|c| image(art::blob(BLOB_TEXELS, *c))).collect();
        self.art_key = Some(key);
    }

    fn blob(&self, layer: &mut Layer, f: &FxFrame<'_>, color: u8, p: Point, r: f32, alpha: f32) {
        let Some(sprite) = self.sprites.get(usize::from(color)) else { return };
        let rect = Rect::new(p.x - r, p.y - r, 2.0 * r, 2.0 * r);
        if !hidden(&rect, f.covered) {
            push(layer, sprite, rect, alpha);
        }
    }

    pub fn paint(&mut self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        self.at.clear();
        self.at.extend(self.blobs.iter().map(|b| blob_at(b, e, f.window, f.scale, f.time)));
        let alpha = (e.opacity * 2.0).min(1.0);
        // Where two blobs of a color come near, a neck between them (they merge, then part).
        for (i, &(a, ra)) in self.at.iter().enumerate() {
            for (j, &(b, rb)) in self.at.iter().enumerate().skip(i + 1) {
                if self.blobs[i].color != self.blobs[j].color {
                    continue;
                }
                let d = (b.x - a.x).hypot(b.y - a.y);
                let reach = (ra + rb) * 1.3;
                if d >= reach || d < 1.0 {
                    continue;
                }
                let near = 1.0 - d / reach;
                let t = ra / (ra + rb);
                let mid =
                    Point::new((a.x + (b.x - a.x) * t).round(), (a.y + (b.y - a.y) * t).round());
                let r = (ra.min(rb) * (0.3 + 0.6 * near)).round();
                self.blob(layer, f, self.blobs[i].color, mid, r, alpha);
            }
        }
        for (k, &(p, r)) in self.at.iter().enumerate() {
            self.blob(layer, f, self.blobs[k].color, p, r, alpha);
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.blobs.capacity() * std::mem::size_of::<Blob>()
            + self.at.capacity() * std::mem::size_of::<(Point, f32)>()
    }
}

// ---------------------------------------------------------------------------------------
// Retro CRT.
