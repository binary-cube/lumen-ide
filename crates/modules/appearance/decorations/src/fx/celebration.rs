//! Bubbles, fireworks, shooting stars, waves and neon.

use super::*;

/// A bubble: where it rises, how big (0..3), how fast, how it sways, its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bubble {
    pub x: f32,
    pub y: f32,
    pub size: u8,
    pub rise: f32,
    pub sway: f32,
    pub phase: f32,
    pub color: u8,
}

pub fn bubble_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Bubble> {
    let n = ((e.density * 50.0 * area(window, scale)).round() as usize).min(MAX_BUBBLES);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Bubble {
            x: hash(i * 9 + 1),
            y: hash(i * 9 + 2),
            size: match hash(i * 9 + 3) {
                v if v < 0.5 => 0,
                v if v < 0.85 => 1,
                _ => 2,
            },
            rise: 0.7 + 0.6 * hash(i * 9 + 4),
            sway: (3.0 + 8.0 * hash(i * 9 + 5)) * scale,
            phase: hash(i * 9 + 6) * std::f32::consts::TAU,
            color: (i % colors) as u8,
        })
        .collect()
}

/// A bubble at `time`: its middle, how much it has swollen (popping) and how visible.
pub fn bubble_at(
    b: &Bubble,
    e: &EffectSpec,
    window: Rect,
    scale: f32,
    time: f32,
) -> (Point, f32, f32) {
    let margin = 20.0 * scale;
    let travel = window.size.height + 2.0 * margin;
    let speed = (14.0 + 60.0 * e.speed) * scale * b.rise;
    let up = (b.y * travel + time * speed).rem_euclid(travel);
    let y = window.max_y() + margin - up;
    let x =
        window.min_x() + b.x * window.size.width + b.sway * (time * 0.9 * b.rise + b.phase).sin();
    // The last part of the way: it swells and fades (popping).
    let left = up / travel;
    let (swell, alpha) = if left > 0.93 {
        let t = (left - 0.93) / 0.07;
        (1.0 + 0.45 * t, 1.0 - t)
    } else {
        (1.0, (left / 0.08).min(1.0))
    };
    (Point::new(x, y), swell, alpha)
}

#[derive(Debug, Default)]
pub struct BubblePainter {
    bubbles: Vec<Bubble>,
    /// Per color, one per size.
    sprites: Vec<ImageData>,
    art_key: Option<(u32, u32)>,
}

impl BubblePainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.bubbles = bubble_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let base = ((4.0 + 8.0 * e.size) * scale).round() as u32;
        let key = (base, e.colors.len() as u32);
        if self.art_key == Some(key) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e
            .colors
            .iter()
            .flat_map(|c| [1.0, 1.6, 2.4].map(|k| image(art::bubble(base as f32 * k, *c))))
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        for b in &self.bubbles {
            let Some(sprite) = self.sprites.get(usize::from(b.color) * 3 + usize::from(b.size))
            else {
                continue;
            };
            let (p, swell, alpha) = bubble_at(b, e, f.window, f.scale, f.time);
            let (w, h) = (sprite.width as f32 * swell, sprite.height as f32 * swell);
            let r = Rect::new(p.x - w / 2.0, p.y - h / 2.0, w, h);
            if hidden(&r, f.covered) {
                continue;
            }
            push(layer, sprite, r, (e.opacity * 2.0 * alpha).min(1.0));
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.bubbles.capacity() * std::mem::size_of::<Bubble>()
    }
}

// ---------------------------------------------------------------------------------------
// Fireworks.

/// How long a burst lasts (seconds).
pub const BURST_LIFE: f32 = 2.4;

/// The time between bursts (seconds), for a density.
pub fn burst_period(density: f32) -> f32 {
    6.5 - 5.0 * density.clamp(0.0, 1.0)
}

/// Burst `k`'s start (seconds): every period, a little early or late; placed so a still
/// frame catches one in bloom.
pub fn burst_start(k: i64, period: f32) -> f32 {
    let shift = (ui_components::decorations::STATIC_TIME - 0.55).rem_euclid(period);
    k as f32 * period + shift
}

/// Burst `k`'s middle (fractions of the window: away from the edges, in the upper part).
pub fn burst_center(k: i64) -> (f32, f32) {
    let k = k.rem_euclid(1 << 20) as u32;
    (0.15 + 0.7 * hash(k * 5 + 2), 0.12 + 0.38 * hash(k * 5 + 3))
}

/// Fireworks: their flash images, one per color.
#[derive(Debug, Default)]
pub struct FireworkPainter {
    flashes: Vec<ImageData>,
    art_key: Option<u32>,
}

impl FireworkPainter {
    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let radius = ((18.0 + 22.0 * e.size) * scale).round();
        if self.art_key == Some(radius as u32) && !self.flashes.iter().any(ImageData::is_lost) {
            return;
        }
        self.flashes = e.colors.iter().map(|c| image(art::glow_dot(radius, *c))).collect();
        self.art_key = Some(radius as u32);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        if e.density <= 0.0 || e.colors.is_empty() {
            return;
        }
        let period = burst_period(e.density);
        let shift = burst_start(0, period);
        let now = ((f.time - shift) / period).floor() as i64;
        for k in [now - 1, now] {
            let age = f.time - burst_start(k, period);
            if !(0.0..BURST_LIFE).contains(&age) {
                continue;
            }
            let (cx, cy) = burst_center(k);
            let c = Point::new(
                f.window.min_x() + cx * f.window.size.width,
                f.window.min_y() + cy * f.window.size.height,
            );
            let color_at = k.rem_euclid(e.colors.len() as i64) as usize;
            let color = e.colors[color_at];
            let life = age / BURST_LIFE;
            // The flash, early on.
            if age < 0.35
                && let Some(flash) = self.flashes.get(color_at)
            {
                let (w, h) = (flash.width as f32, flash.height as f32);
                let r = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
                if !hidden(&r, f.covered) {
                    push(layer, flash, r, e.opacity * 1.6 * (1.0 - age / 0.35));
                }
            }
            let reach = (70.0 + 110.0 * e.size) * f.scale;
            let spread = 1.0 - (-3.2 * age * (0.6 + 0.8 * e.speed)).exp();
            let gravity = 26.0 * f.scale * age * age;
            let fade = (1.0 - life).powf(1.5);
            let d = (1.6 + 1.4 * e.size) * f.scale * (1.0 - 0.4 * life);
            let n = (SPARKS as f32 * (0.6 + 0.4 * e.size)) as usize;
            let kk = k.rem_euclid(1 << 20) as u32;
            for i in 0..n {
                let a =
                    (i as f32 + 0.5 * hash(kk * 131 + i as u32)) / n as f32 * std::f32::consts::TAU;
                let v = reach * (0.55 + 0.45 * hash(kk * 977 + i as u32 * 3));
                let p =
                    Point::new(c.x + a.cos() * v * spread, c.y + a.sin() * v * spread + gravity);
                let r = Rect::new(p.x - d, p.y - d, 2.0 * d, 2.0 * d);
                if hidden(&r, f.covered) {
                    continue;
                }
                // Near its end, a spark glints.
                let glint =
                    if life > 0.6 { 0.6 + 0.4 * (age * 30.0 + i as f32).sin().abs() } else { 1.0 };
                let tint = if i % 3 == 0 { color.mix(Rgba::WHITE, 0.5) } else { color };
                dot(layer, p, d, tint.with_alpha((e.opacity * 2.0 * fade * glint).min(1.0)));
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.flashes.iter()
    }
}

// ---------------------------------------------------------------------------------------
// Shooting stars.

/// The time between shooting stars (seconds), for a density.
pub fn streak_period(density: f32) -> f32 {
    9.0 - 7.0 * density.clamp(0.0, 1.0)
}

/// How long one crosses (seconds), for a speed.
pub fn streak_life(speed: f32) -> f32 {
    1.3 - 0.8 * speed.clamp(0.0, 1.0)
}

/// Shooting star `k`: where it starts (fractions of the window), its direction (radians).
pub fn streak_path(k: i64) -> (f32, f32, f32) {
    let k = k.rem_euclid(1 << 20) as u32;
    let left = hash(k * 7 + 1) < 0.5;
    let x = if left { 0.08 + 0.4 * hash(k * 7 + 2) } else { 0.52 + 0.4 * hash(k * 7 + 2) };
    let y = 0.03 + 0.25 * hash(k * 7 + 3);
    let tilt = (0.32 + 0.3 * hash(k * 7 + 4)) * if left { 1.0 } else { -1.0 };
    let angle = std::f32::consts::FRAC_PI_2 * if left { 0.0 } else { 2.0 } + tilt;
    (x, y, angle)
}

/// A shooting star's head and its streak (a row of fading dots), drawn as quads.
pub fn paint_streaks(e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
    if e.density <= 0.0 || e.colors.is_empty() {
        return;
    }
    let period = streak_period(e.density);
    let life = streak_life(e.speed);
    // Placed so a still frame catches one half way.
    let shift = (ui_components::decorations::STATIC_TIME - life * 0.5).rem_euclid(period);
    let now = ((f.time - shift) / period).floor() as i64;
    for k in [now - 1, now] {
        let age = f.time - (k as f32 * period + shift);
        if !(0.0..life).contains(&age) {
            continue;
        }
        let (x, y, angle) = streak_path(k);
        let start = Point::new(
            f.window.min_x() + x * f.window.size.width,
            f.window.min_y() + y * f.window.size.height,
        );
        let travel = f.window.size.width * 0.42 * (age / life);
        let (dx, dy) = (angle.cos(), angle.sin().abs());
        let head = Point::new(start.x + dx * travel, start.y + dy * travel);
        let length = (60.0 + 200.0 * e.size) * f.scale;
        let fade = (age / life * std::f32::consts::PI).sin();
        let color = e.colors[k.rem_euclid(e.colors.len() as i64) as usize];
        let step = 1.5 * f.scale;
        let n = ((length / step) as usize).min(180);
        for i in 0..n {
            let t = i as f32 / n as f32;
            let p = Point::new(head.x - dx * t * length, head.y - dy * t * length);
            let d = (2.2 - 1.6 * t) * f.scale;
            let r = Rect::new(p.x - d, p.y - d, 2.0 * d, 2.0 * d);
            if hidden(&r, f.covered) {
                continue;
            }
            let alpha = e.opacity * 2.0 * fade * (1.0 - t).powf(1.6);
            dot(layer, p, d, color.mix(Rgba::WHITE, 0.6 * (1.0 - t)).with_alpha(alpha.min(1.0)));
        }
    }
}

// ---------------------------------------------------------------------------------------
// Waves.

/// The waves are drawn at half the screen's resolution and stretched into place.
const WAVE_DETAIL: f32 = 0.5;

/// The waves along the bottom: each an image a period wide, repeated, moving.
#[derive(Debug, Default)]
pub struct WavePainter {
    tiles: Vec<ImageData>,
    /// Each wave's size on screen (physical pixels).
    sizes: Vec<(f32, f32)>,
    art_key: Option<(u32, u32, usize)>,
}

impl WavePainter {
    /// How many waves (1–3), for a density.
    pub fn count(e: &EffectSpec) -> usize {
        1 + (e.density.clamp(0.0, 1.0) * 2.0).round() as usize
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let n = Self::count(e);
        let key = ((scale * 100.0) as u32, (e.size * 100.0) as u32, n);
        if self.art_key == Some(key) && !self.tiles.iter().any(ImageData::is_lost) {
            return;
        }
        self.sizes = (0..n)
            .map(|i| {
                let w = ((420.0 + 160.0 * i as f32) * scale).round();
                let h = ((26.0 + 64.0 * e.size) * scale * (1.0 - 0.18 * i as f32)).round();
                (w, h)
            })
            .collect();
        self.tiles = self
            .sizes
            .iter()
            .enumerate()
            .map(|(i, (w, h))| {
                let c = e.colors[i % e.colors.len().max(1)];
                let (tw, th) = ((w * WAVE_DETAIL).round() as u32, (h * WAVE_DETAIL).round() as u32);
                image(art::wave_tile(tw, th, c, i as u32 + 1))
            })
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        // The back wave first (the widest, the palest), the front last.
        for (i, (tile, (w, h))) in self.tiles.iter().zip(&self.sizes).enumerate().rev() {
            let pace = (8.0 + 40.0 * e.speed) * f.scale * (1.0 - 0.25 * i as f32);
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let offset = (f.time * pace * dir + i as f32 * w * 0.41).rem_euclid(*w);
            let bob = (f.time * (0.5 + 0.2 * i as f32) + i as f32).sin() * 2.0 * f.scale;
            let y = (f.window.max_y() - h - i as f32 * h * 0.35 + bob).round();
            let opacity = e.opacity * 2.0 * (1.0 - 0.25 * i as f32);
            let mut x = f.window.min_x() - offset;
            while x < f.window.max_x() {
                let r = Rect::new(x, y, *w, *h);
                if !hidden(&r, f.covered) {
                    push(layer, tile, r, opacity.min(1.0));
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
// Neon.

/// The neon edge's color at `time`: pulsing in its first color, or turning through all of
/// them.
pub fn neon_color(e: &EffectSpec, time: f32) -> (Rgba, f32) {
    let first = e.colors.first().copied().unwrap_or(Rgba::WHITE);
    let rate = 0.15 + 0.85 * e.speed;
    if e.style == "hue" && e.colors.len() > 1 {
        let n = e.colors.len() as f32;
        let t = (time * rate * 0.35).rem_euclid(n);
        let (i, k) = (t.floor() as usize, t.fract());
        let next = (i + 1) % e.colors.len();
        let k = k * k * (3.0 - 2.0 * k);
        return (e.colors[i].mix(e.colors[next], k), 1.0);
    }
    let level = if e.speed > 0.0 {
        0.72 + 0.28 * (time * rate * std::f32::consts::TAU * 0.4).sin()
    } else {
        1.0
    };
    (first, level)
}

/// The neon edge: a bright line round the window and its halo (a few fainter, wider lines),
/// each side its own quad (so what changes is only the edges).
pub fn paint_neon(e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
    let (color, level) = neon_color(e, f.time);
    let w = f.window;
    let core = ((1.0 + 2.0 * e.size) * f.scale).round().max(1.0);
    // The halo: rings outward-in, each wider and fainter.
    let rings = [(core * 5.0, 0.10), (core * 3.0, 0.18), (core * 1.8, 0.32), (core, 1.0)];
    for (width, strength) in rings {
        let strength = if width > core { strength * e.glow } else { strength };
        let c = color.mix(Rgba::WHITE, if width <= core { 0.35 } else { 0.0 });
        let alpha = (e.opacity * strength * level).min(1.0);
        if alpha < 0.005 {
            continue;
        }
        let c = c.with_alpha(alpha);
        let sides = [
            Rect::new(w.min_x(), w.min_y(), w.size.width, width),
            Rect::new(w.min_x(), w.max_y() - width, w.size.width, width),
            Rect::new(w.min_x(), w.min_y() + width, width, w.size.height - 2.0 * width),
            Rect::new(w.max_x() - width, w.min_y() + width, width, w.size.height - 2.0 * width),
        ];
        for r in sides {
            layer.quads.push(Quad::filled(r, c));
        }
    }
}

// ---------------------------------------------------------------------------------------
// Constellation.
