//! Night skies: falling code, stars, the aurora, fireflies.

use super::*;

/// A column of falling characters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub x: f32,
    /// Its speed (a factor), its place in its cycle, its trail's length (cells).
    pub rate: f32,
    pub phase: f32,
    pub trail: u16,
    pub color: u8,
    pub seed: u32,
}

/// Where Matrix's columns are, for a window (`cell`: a character's side, physical).
pub fn matrix_layout(e: &EffectSpec, window: Rect, scale: f32) -> (f32, Vec<Column>) {
    let cell = (14.0 * scale).round().max(6.0);
    let pitch = (cell * 0.82).round();
    let n = ((window.size.width / pitch).ceil() as usize).min(MAX_COLUMNS);
    let colors = e.colors.len().max(1) as u32;
    let keep = 0.1 + 0.85 * e.density;
    let columns = (0..n as u32)
        .filter(|c| hash(c * 13 + 5) < keep)
        .map(|c| Column {
            x: window.min_x() + c as f32 * pitch,
            rate: 0.6 + 0.8 * hash(c * 13 + 6),
            phase: hash(c * 13 + 7),
            trail: (6.0 + 22.0 * e.size * (0.7 + 0.6 * hash(c * 13 + 8))).round() as u16,
            color: (c % colors) as u8,
            seed: c,
        })
        .collect();
    (cell, columns)
}

/// Where a column's head is at `time` (in cells from the top; it wraps after the window
/// and a gap), for `rows` cells down the window.
pub fn matrix_head(col: &Column, rows: f32, time: f32, speed: f32) -> f32 {
    let cycle = rows + f32::from(col.trail) + 6.0 + 10.0 * hash(col.seed * 13 + 9);
    let cells_per_sec = (4.0 + 16.0 * speed) * col.rate;
    (col.phase * cycle + time * cells_per_sec).rem_euclid(cycle)
}

/// Matrix: its columns and its characters' glyph keys.
#[derive(Debug, Default)]
pub struct MatrixPainter {
    cell: f32,
    columns: Vec<Column>,
    keys: Vec<GlyphKey>,
    /// The size and the style the keys are for.
    keys_for: Option<(u16, &'static str)>,
}

impl MatrixPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        let (cell, columns) = matrix_layout(e, window, scale);
        self.cell = cell;
        self.columns = columns;
    }

    fn sync_keys(&mut self, e: &EffectSpec, icons: &icons::Icons) {
        let size = self.cell as u16;
        if self.keys_for == Some((size, e.style)) {
            return;
        }
        self.keys = crate::glyphs::of_style(e.style)
            .into_iter()
            .filter_map(|name| icons.id(name))
            .map(|id| icons.key_styled(id, size, icons::IconStyle::Regular))
            .collect();
        self.keys_for = Some((size, e.style));
    }

    pub fn paint(&mut self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let Some(icons) = f.icons else { return };
        self.sync_keys(e, icons);
        if self.keys.is_empty() || self.cell <= 0.0 {
            return;
        }
        let cell = self.cell;
        let rows = (f.window.size.height / cell).ceil();
        let n = self.keys.len() as u32;
        for col in &self.columns {
            let head = matrix_head(col, rows, f.time, e.speed);
            let base = e.colors.get(usize::from(col.color)).copied().unwrap_or(Rgba::WHITE);
            let trail = f32::from(col.trail);
            let head_row = head.floor();
            for k in 0..col.trail {
                let row = head_row - f32::from(k);
                if row < 0.0 || row >= rows {
                    continue;
                }
                let y = f.window.min_y() + row * cell;
                let r = Rect::new(col.x, y, cell, cell);
                if hidden(&r, f.covered) {
                    continue;
                }
                // Each cell changes now and then, on its own clock.
                let tick = (f.time * (0.8 + 2.2 * hash(col.seed * 31 + row as u32))).floor() as u32;
                let which = (hash(col.seed * 7919 + row as u32 * 104_729 + tick * 31) * n as f32)
                    as u32
                    % n;
                let fade = 1.0 - f32::from(k) / trail;
                let (color, alpha) = if k == 0 {
                    (
                        base.mix(Rgba::WHITE, 0.35 + 0.55 * e.glow),
                        e.opacity * (0.7 + 0.3 * e.glow) * 2.0,
                    )
                } else {
                    (base, e.opacity * fade.powf(1.4))
                };
                let alpha = alpha.min(1.0);
                if alpha < 0.01 {
                    continue;
                }
                layer.glyphs.push(GlyphInstance {
                    key: self.keys[which as usize],
                    origin: Point::new(col.x.round(), (y + cell).round()),
                    color: color.with_alpha(alpha),
                });
            }
        }
    }

    pub fn memory(&self) -> usize {
        self.columns.capacity() * std::mem::size_of::<Column>()
            + self.keys.capacity() * std::mem::size_of::<GlyphKey>()
    }
}

// ---------------------------------------------------------------------------------------
// Stars.

/// A star: its direction (warp) or place (drift), its depth, its color, its twinkle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    pub a: f32,
    pub b: f32,
    pub depth: f32,
    pub color: u8,
    pub phase: f32,
}

pub fn star_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Star> {
    let n = ((e.density * 200.0 * area(window, scale)).round() as usize).min(MAX_STARS);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Star {
            a: hash(i * 5 + 1),
            b: hash(i * 5 + 2),
            depth: hash(i * 5 + 3),
            color: (i % colors) as u8,
            phase: hash(i * 5 + 4) * std::f32::consts::TAU,
        })
        .collect()
}

/// A star at `time`: its middle, its diameter (physical) and its brightness (0..1).
pub fn star_at(s: &Star, e: &EffectSpec, window: Rect, scale: f32, time: f32) -> (Point, f32, f32) {
    let size = 0.5 + e.size;
    if e.style == "drift" {
        let speed = (0.004 + 0.03 * e.speed) * (0.4 + s.depth);
        let x = window.min_x() + (s.a + time * speed).rem_euclid(1.0) * window.size.width;
        let y = window.min_y() + s.b * window.size.height;
        let twinkle = 1.0 - e.glow * 0.5 * (1.0 + (time * (0.6 + 1.2 * s.b) + s.phase).sin());
        let d = (0.8 + 1.8 * s.depth) * size * scale;
        return (Point::new(x, y), d, (0.35 + 0.65 * s.depth) * twinkle.max(0.15));
    }
    // Warp: from the middle outward, quicker and bigger as they come.
    let z = (s.depth + time * (0.02 + 0.16 * e.speed)).rem_euclid(1.0);
    let angle = s.a * std::f32::consts::TAU;
    let reach = 0.5 * window.size.width.hypot(window.size.height);
    let r = reach * (0.03 + 0.97 * z.powf(2.2));
    let c = Point::new(
        window.min_x() + window.size.width / 2.0,
        window.min_y() + window.size.height / 2.0,
    );
    let p = Point::new(c.x + angle.cos() * r, c.y + angle.sin() * r);
    let d = (0.5 + 2.6 * z) * size * scale;
    let fade_in = (z / 0.18).min(1.0);
    (p, d, fade_in * (0.3 + 0.7 * z))
}

#[derive(Debug, Default)]
pub struct StarPainter {
    stars: Vec<Star>,
}

impl StarPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.stars = star_layout(e, window, scale);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let warp = e.style != "drift";
        for s in &self.stars {
            let (p, d, level) = star_at(s, e, f.window, f.scale, f.time);
            let r = Rect::new(p.x - d, p.y - d, 2.0 * d, 2.0 * d);
            if hidden(&r, f.covered) || !f.window.contains(p) {
                continue;
            }
            let c = e.colors.get(usize::from(s.color)).copied().unwrap_or(Rgba::WHITE);
            let alpha = e.opacity * 2.0 * level;
            dot(layer, p, d, c.with_alpha(alpha.min(1.0)));
            // A short streak behind the fast ones.
            if warp && e.speed > 0.3 && d > 1.6 * f.scale {
                let cx = f.window.min_x() + f.window.size.width / 2.0;
                let cy = f.window.min_y() + f.window.size.height / 2.0;
                for k in 1..3 {
                    let t = 1.0 - 0.035 * k as f32 * (0.5 + e.speed);
                    let q = Point::new(cx + (p.x - cx) * t, cy + (p.y - cy) * t);
                    dot(layer, q, d * 0.8, c.with_alpha((alpha * (0.45 / k as f32)).min(1.0)));
                }
            }
        }
    }

    pub fn memory(&self) -> usize {
        self.stars.capacity() * std::mem::size_of::<Star>()
    }
}

// ---------------------------------------------------------------------------------------
// Aurora.

/// An aurora band's width (one period, logical pixels).
pub const AURORA_PERIOD: f32 = 460.0;

/// The aurora's bands are drawn at a third of the screen's resolution and stretched into
/// place: soft light loses nothing, and they hold a ninth of the memory.
const AURORA_DETAIL: f32 = 1.0 / 3.0;

/// The aurora's bands: each an image a period wide, repeated.
#[derive(Debug, Default)]
pub struct AuroraPainter {
    bands: Vec<ImageData>,
    /// The bands' size on screen (physical pixels), what the art was made for.
    art_key: Option<(u32, u32)>,
}

impl AuroraPainter {
    fn band_size(e: &EffectSpec, scale: f32) -> (u32, u32) {
        ((AURORA_PERIOD * scale).round() as u32, ((70.0 + 150.0 * e.size) * scale).round() as u32)
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let key = Self::band_size(e, scale);
        if self.art_key == Some(key) && !self.bands.iter().any(ImageData::is_lost) {
            return;
        }
        let n = 3.min(e.colors.len().max(1) + 1);
        let (w, h) = (
            (key.0 as f32 * AURORA_DETAIL).round() as u32,
            (key.1 as f32 * AURORA_DETAIL).round() as u32,
        );
        self.bands = (0..n)
            .map(|b| {
                let c = e.colors[b % e.colors.len().max(1)];
                image(art::aurora_band(w, h, c, b as u32 + 1))
            })
            .collect();
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let Some((w, h)) = self.art_key else { return };
        let (w, h) = (w as f32, h as f32);
        for (b, image) in self.bands.iter().enumerate() {
            let pace = (6.0 + 26.0 * e.speed) * f.scale * (0.6 + 0.35 * b as f32);
            let dir = if b % 2 == 0 { 1.0 } else { -1.0 };
            let offset = (f.time * pace * dir + b as f32 * w * 0.37).rem_euclid(w);
            let y = f.window.min_y() + b as f32 * h * 0.14;
            let breathe = 0.72 + 0.28 * (f.time * (0.18 + 0.06 * b as f32) + b as f32 * 1.7).sin();
            let opacity = e.opacity * breathe * if b == 0 { 1.0 } else { 0.75 };
            let mut x = f.window.min_x() - offset;
            while x < f.window.max_x() {
                let r = Rect::new(x, y, w, h);
                if !hidden(&r, f.covered) {
                    push(layer, image, r, opacity);
                }
                x += w;
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.bands.iter()
    }
}

// ---------------------------------------------------------------------------------------
// Fireflies.

/// A firefly: its home, how it wanders, how it pulses, its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fly {
    pub x: f32,
    pub y: f32,
    pub wander: f32,
    pub f1: f32,
    pub f2: f32,
    pub phase: f32,
    pub color: u8,
}

pub fn fly_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Fly> {
    let n = ((e.density * 44.0 * area(window, scale).sqrt()).round() as usize).min(MAX_FLIES);
    let colors = e.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| Fly {
            x: hash(i * 7 + 1),
            y: hash(i * 7 + 2),
            wander: (30.0 + 70.0 * hash(i * 7 + 3)) * scale,
            f1: 0.05 + 0.12 * hash(i * 7 + 4),
            f2: 0.04 + 0.1 * hash(i * 7 + 5),
            phase: hash(i * 7 + 6) * std::f32::consts::TAU,
            color: (i % colors) as u8,
        })
        .collect()
}

/// A firefly at `time`: its middle and how brightly it glows (0..1).
pub fn fly_at(fl: &Fly, e: &EffectSpec, window: Rect, time: f32) -> (Point, f32) {
    let t = time * (0.4 + 1.4 * e.speed);
    let x = window.min_x()
        + fl.x * window.size.width
        + fl.wander
            * ((t * fl.f1 * std::f32::consts::TAU + fl.phase).sin()
                + 0.5 * (t * fl.f2 * 9.0).sin());
    let y = window.min_y()
        + fl.y * window.size.height
        + fl.wander * 0.7 * ((t * fl.f2 * std::f32::consts::TAU + fl.phase * 1.3).cos());
    let pulse = (time * (0.35 + 0.5 * fl.f1 * 6.0) + fl.phase).sin().max(0.0).powi(2);
    (Point::new(x, y), 0.2 + 0.8 * (pulse * e.glow + (1.0 - e.glow) * 0.6))
}

#[derive(Debug, Default)]
pub struct FlyPainter {
    flies: Vec<Fly>,
    sprites: Vec<ImageData>,
    art_key: Option<u32>,
}

impl FlyPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.flies = fly_layout(e, window, scale);
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let radius = ((5.0 + 9.0 * e.size) * scale).round();
        if self.art_key == Some(radius as u32) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        self.sprites = e.colors.iter().map(|c| image(art::glow_dot(radius, *c))).collect();
        self.art_key = Some(radius as u32);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        for fl in &self.flies {
            let Some(sprite) = self.sprites.get(usize::from(fl.color)) else { continue };
            let (p, level) = fly_at(fl, e, f.window, f.time);
            let (w, h) = (sprite.width as f32, sprite.height as f32);
            let r = Rect::new(p.x - w / 2.0, p.y - h / 2.0, w, h);
            if hidden(&r, f.covered) {
                continue;
            }
            push(layer, sprite, r, (e.opacity * 2.0 * level).min(1.0));
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.sprites.iter()
    }

    pub fn memory(&self) -> usize {
        self.flies.capacity() * std::mem::size_of::<Fly>()
    }
}

// ---------------------------------------------------------------------------------------
// Bubbles.
