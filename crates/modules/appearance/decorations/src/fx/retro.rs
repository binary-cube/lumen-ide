//! The CRT, glitches, neon rain and holograms.

use super::*;

/// A glitch's length (seconds).
pub const GLITCH_LIFE: f32 = 0.3;

/// The effects' own colors when they're given none: a screen's red and cyan fringes, a
/// glitch's two.
const CRT_RED: Rgba = Rgba::rgb(0xff3b6b);
const CRT_CYAN: Rgba = Rgba::rgb(0x3bd6ff);
const GLITCH_FIRST: Rgba = Rgba::rgb(0xff2b6b);
const GLITCH_SECOND: Rgba = Rgba::rgb(0x2bd6ff);

/// The time between glitches (seconds), for a density.
pub fn glitch_period(density: f32) -> f32 {
    18.0 - 12.0 * density.clamp(0.0, 1.0)
}

/// The scanlines' period and dark line (physical pixels), for a size.
pub fn scanline_pitch(e: &EffectSpec, scale: f32) -> (u32, u32) {
    let period = ((1.5 + 1.5 * e.size) * scale).round().max(2.0) as u32;
    (period, ((period as f32 * 0.35).round() as u32).max(1))
}

/// A retro CRT: its scanlines (one tile, repeated down the window), the curved screen's
/// shade at the edges, a rare glitch.
#[derive(Debug, Default)]
pub struct CrtPainter {
    lines: Option<ImageData>,
    strips: [Option<ImageData>; 4],
    art_key: Option<(u32, u32, bool)>,
}

impl CrtPainter {
    /// How far the curved screen's shade reaches (physical pixels).
    pub fn reach(e: &EffectSpec, scale: f32) -> f32 {
        ((50.0 + 90.0 * e.size) * scale).round()
    }

    pub fn sync_art(&mut self, e: &EffectSpec, scale: f32) {
        let curved = e.style == "curved";
        let (period, _) = scanline_pitch(e, scale);
        let key = (period, Self::reach(e, scale) as u32, curved);
        let lost = self.lines.iter().chain(self.strips.iter().flatten()).any(ImageData::is_lost);
        if self.art_key == Some(key) && !lost {
            return;
        }
        let color = e.colors.first().copied().unwrap_or(Rgba::BLACK);
        let (period, thick) = scanline_pitch(e, scale);
        self.lines = Some(image(art::scanlines(period, thick, (480 / period).max(1), color)));
        self.strips = if curved {
            let shade = theming::decorations::GlowSpec {
                style: theming::decorations::GlowStyle::Vignette,
                color,
                opacity: 1.0,
                width: 1.0,
            };
            art::glow_strips(&shade, Self::reach(e, scale)).map(|c| Some(image(c)))
        } else {
            [None, None, None, None]
        };
        self.art_key = Some(key);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let w = f.window;
        let curved = e.style == "curved";
        let reach = Self::reach(e, f.scale).min(w.size.width / 3.0);
        // Where the lines and the glitch show: the whole window, or the edges' shade.
        let sides = [
            Rect::new(w.min_x(), w.min_y(), reach, w.size.height),
            Rect::new(w.max_x() - reach, w.min_y(), reach, w.size.height),
        ];
        if curved {
            let rects = [
                Rect::new(w.min_x(), w.min_y(), w.size.width, reach),
                Rect::new(w.min_x(), w.max_y() - reach, w.size.width, reach),
                sides[0],
                sides[1],
            ];
            for (strip, r) in self.strips.iter().zip(rects) {
                if let Some(image) = strip {
                    push(layer, image, r, (e.opacity * 2.5).min(1.0));
                }
            }
        }
        if let Some(lines) = &self.lines {
            let tile = lines.height as f32;
            let opacity = (e.opacity * if curved { 1.6 } else { 2.0 }).min(1.0);
            let mut y = w.min_y();
            while y < w.max_y() {
                if curved {
                    for side in sides {
                        push(
                            layer,
                            lines,
                            Rect::new(side.min_x(), y, side.size.width, tile),
                            opacity,
                        );
                    }
                } else {
                    push(layer, lines, Rect::new(w.min_x(), y, w.size.width, tile), opacity);
                }
                y += tile;
            }
        }
        // The glitch: now and then, for a moment, a few bands split in two colors and a
        // brighter bar rolling down.
        if e.density <= 0.0 {
            return;
        }
        let period = glitch_period(e.density);
        let shift = (ui_components::decorations::STATIC_TIME - 0.08).rem_euclid(period);
        let k = ((f.time - shift) / period).floor() as i64;
        let age = f.time - (k as f32 * period + shift);
        if !(0.0..GLITCH_LIFE).contains(&age) {
            return;
        }
        let level =
            (1.0 - age / GLITCH_LIFE) * if (age * 40.0) as i32 % 2 == 0 { 1.0 } else { 0.6 };
        let kk = k.rem_euclid(1 << 20) as u32;
        let red = e.colors.get(1).copied().unwrap_or(CRT_RED);
        let cyan = e.colors.get(2).copied().unwrap_or(CRT_CYAN);
        let a = (e.opacity * 1.2 * level).min(1.0);
        let band = |layer: &mut Layer, y: f32, h: f32, c: Rgba| {
            if curved {
                for side in sides {
                    layer
                        .quads
                        .push(Quad::filled(Rect::new(side.min_x(), y, side.size.width, h), c));
                }
            } else {
                layer.quads.push(Quad::filled(Rect::new(w.min_x(), y, w.size.width, h), c));
            }
        };
        for j in 0..3 {
            let y = (w.min_y() + w.size.height * hash(kk * 7 + j)).round();
            let h = ((3.0 + 10.0 * hash(kk * 7 + j + 3)) * f.scale).round();
            band(layer, y, h, red.with_alpha(a));
            band(layer, y + (2.0 * f.scale).round(), h, cyan.with_alpha(a));
        }
        let roll = (w.min_y() + w.size.height * (age / GLITCH_LIFE)).round();
        band(layer, roll, (24.0 * f.scale).round(), Rgba::WHITE.with_alpha(a * 0.35));
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.lines.iter().chain(self.strips.iter().flatten())
    }
}

// ---------------------------------------------------------------------------------------
// Glitch (plan 45).

/// How long a glitch lasts (seconds).
pub const GLITCH_FLASH: f32 = 0.25;

/// The time between glitches (seconds), for a density: every 30 s at the least, every 4 s
/// at the most.
pub fn glitch_every(density: f32) -> f32 {
    30.0 - 26.0 * density.clamp(0.0, 1.0)
}

/// The glitch happening at `time`, if one is: its count (what makes it its own) and its
/// age. One is half way through in the still frame.
pub fn glitch_at(density: f32, time: f32) -> Option<(u32, f32)> {
    if density <= 0.0 {
        return None;
    }
    let period = glitch_every(density);
    let shift = (STATIC_TIME - GLITCH_FLASH * 0.5).rem_euclid(period);
    let k = ((time - shift) / period).floor();
    let age = time - (k * period + shift);
    (0.0..GLITCH_FLASH).contains(&age).then(|| ((k as i64).rem_euclid(1 << 20) as u32, age))
}

/// A glitch: bands (or blocks) across the zone, each in its first color with its copy in the
/// second a few pixels off (the colors split), a thin bright line through some, all
/// jumping 30 times a second; nothing between glitches.
pub fn paint_glitch(e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
    let Some((k, age)) = glitch_at(e.density, f.time) else { return };
    let w = f.window;
    let step = (age * 30.0) as u32;
    let level = (1.0 - 0.5 * age / GLITCH_FLASH) * if step.is_multiple_of(2) { 1.0 } else { 0.6 };
    let a = (e.opacity * 1.6 * level).min(1.0);
    if a < 0.005 {
        return;
    }
    let first = e.colors.first().copied().unwrap_or(GLITCH_FIRST);
    let second = e.colors.get(1).copied().unwrap_or(GLITCH_SECOND);
    let bright = e.colors.get(2).copied().unwrap_or(Rgba::WHITE);
    let bands = 2 + (3.0 * e.size).round() as u32;
    let split = ((2.0 + 4.0 * e.size) * f.scale).round();
    let thin = f.scale.round().max(1.0);
    for j in 0..bands {
        let seed = k * 37 + j * 5;
        let y = (w.min_y() + w.size.height * hash(seed)).round();
        let h = ((3.0 + 16.0 * e.size * hash(seed + 1)) * f.scale).round().max(1.0);
        let jump = (hash(seed + 2 + step * 101) - 0.5) * 2.0 * (8.0 + 40.0 * e.size) * f.scale;
        let jump = jump.round();
        let mut bar = |x: f32, width: f32| {
            layer.quads.push(Quad::filled(Rect::new(x, y, width, h), first.with_alpha(a)));
            layer.quads.push(Quad::filled(
                Rect::new(x + split, y + (split / 2.0).round(), width, h),
                second.with_alpha(a),
            ));
            if hash(seed + 3) > 0.5 {
                layer.quads.push(Quad::filled(
                    Rect::new(x, (y + h / 2.0).round(), width, thin),
                    bright.with_alpha(a * 0.6),
                ));
            }
        };
        if e.style == "blocks" {
            for b in 0..3 {
                let s = seed * 11 + b * 3 + step * 7;
                let width = ((40.0 + 260.0 * hash(s)) * f.scale).round();
                let x = (w.min_x() + (w.size.width - width) * hash(s + 1) + jump).round();
                bar(x, width);
            }
        } else {
            bar(w.min_x() + jump, w.size.width);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Neon rain.

/// A neon streak: its place across, its moment, its pace, its length (a factor), its color,
/// the pause before it falls again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Streak {
    pub x: f32,
    pub phase: f32,
    pub rate: f32,
    pub length: f32,
    pub gap: f32,
    pub color: u8,
}

/// The most streaks.
pub const MAX_STREAKS: usize = 180;
/// How long a splash lasts (seconds).
pub const SPLASH_LIFE: f32 = 0.35;

pub fn streak_layout(e: &EffectSpec, window: Rect, scale: f32) -> Vec<Streak> {
    let n = (((20.0 + 160.0 * e.density) * area(window, scale)).round() as usize).min(MAX_STREAKS);
    let colors = e.colors.len().max(1);
    (0..n as u32)
        .map(|i| Streak {
            x: hash(i * 17 + 1),
            phase: hash(i * 17 + 2),
            rate: 0.7 + 0.6 * hash(i * 17 + 3),
            length: 0.6 + 0.8 * hash(i * 17 + 4),
            gap: 0.1 + 0.6 * hash(i * 17 + 5),
            color: (i as usize % colors) as u8,
        })
        .collect()
}

/// Where a streak is at `time`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RainAt {
    /// Falling: its head, its length (physical pixels).
    Falling { head: Point, length: f32 },
    /// Splashing at the bottom, this long since it landed.
    Splash { at: Point, age: f32 },
    /// Waiting to fall again.
    Gone,
}

pub fn streak_at(s: &Streak, e: &EffectSpec, window: Rect, scale: f32, time: f32) -> RainAt {
    let speed = (450.0 + 1100.0 * e.speed) * scale * s.rate;
    let length = ((24.0 + 90.0 * e.size) * scale * s.length).round();
    let fall = window.size.height / speed;
    let cycle = fall + SPLASH_LIFE + s.gap;
    let u = (s.phase * cycle + time).rem_euclid(cycle);
    let x = (window.min_x() + s.x * window.size.width).round();
    if u < fall {
        RainAt::Falling { head: Point::new(x, (window.min_y() + u * speed).round()), length }
    } else if u < fall + SPLASH_LIFE {
        RainAt::Splash { at: Point::new(x, window.max_y()), age: u - fall }
    } else {
        RainAt::Gone
    }
}

/// Neon rain: its streaks (no art: thin quads and dots).
#[derive(Debug, Default)]
pub struct NeonRainPainter {
    streaks: Vec<Streak>,
}

impl NeonRainPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec, window: Rect, scale: f32) {
        self.streaks = streak_layout(e, window, scale);
    }

    pub fn paint(&self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let line = (0.9 * f.scale).round().max(1.0);
        let floor = (3.0 * f.scale).round();
        for s in &self.streaks {
            let base = e.colors.get(usize::from(s.color)).copied().unwrap_or(Rgba::WHITE);
            match streak_at(s, e, f.window, f.scale, f.time) {
                RainAt::Falling { head, length } => {
                    let r = Rect::new(head.x - 2.0 * line, head.y - length, 5.0 * line, length);
                    if hidden(&r, f.covered) {
                        continue;
                    }
                    // The tail in three steps, fainter upward; the head bright, its halo.
                    let third = (length / 3.0).round().max(1.0);
                    for (k, strength) in [(2.0, 0.2), (1.0, 0.45), (0.0, 0.8)] {
                        let y = head.y - third * (k + 1.0);
                        let a = (e.opacity * 2.2 * strength).min(1.0);
                        layer.quads.push(Quad::filled(
                            Rect::new(head.x, y, line, third),
                            base.with_alpha(a),
                        ));
                    }
                    if e.glow > 0.0 {
                        let a = (e.opacity * 0.5 * e.glow).min(1.0);
                        layer.quads.push(Quad::filled(
                            Rect::new(head.x - line, head.y - third, 3.0 * line, third),
                            base.with_alpha(a),
                        ));
                    }
                    let a = (e.opacity * (1.4 + 1.2 * e.glow)).min(1.0);
                    layer.quads.push(Quad::filled(
                        Rect::new(head.x, head.y - 2.0 * line, line, 2.0 * line),
                        base.mix(Rgba::WHITE, 0.3 + 0.4 * e.glow).with_alpha(a),
                    ));
                }
                RainAt::Splash { at, age } => {
                    let t = age / SPLASH_LIFE;
                    let fade = (1.0 - t) * (1.0 - t);
                    let y = at.y - floor;
                    let wide = ((4.0 + 26.0 * t) * f.scale * (0.6 + 0.8 * e.size)).round();
                    let r = Rect::new(at.x - wide, y - 30.0 * f.scale, 2.0 * wide, 30.0 * f.scale);
                    if hidden(&r, f.covered) {
                        continue;
                    }
                    // The reflection: a short line widening along the bottom.
                    let a = (e.opacity * 1.4 * fade * (0.6 + 0.4 * e.glow)).min(1.0);
                    layer.quads.push(Quad::filled(
                        Rect::new((at.x - wide / 2.0).round(), y, wide, line),
                        base.with_alpha(a),
                    ));
                    // A few drops jumping out and falling back.
                    for d in 0..3u32 {
                        let side = d as f32 - 1.0;
                        let seed = (s.x * 9973.0) as u32 * 3 + d;
                        let vx = side * (30.0 + 40.0 * hash(seed)) * f.scale;
                        let vy = (70.0 + 70.0 * hash(seed + 1)) * f.scale;
                        let g = 700.0 * f.scale;
                        let px = at.x + vx * age;
                        let py = y - (vy * age - 0.5 * g * age * age);
                        if py > y {
                            continue;
                        }
                        let c = base
                            .mix(Rgba::WHITE, 0.3)
                            .with_alpha((e.opacity * 1.6 * fade).min(1.0));
                        dot(layer, Point::new(px, py), 1.6 * line, c);
                    }
                }
                RainAt::Gone => {}
            }
        }
    }

    pub fn memory(&self) -> usize {
        self.streaks.capacity() * std::mem::size_of::<Streak>()
    }
}

// ---------------------------------------------------------------------------------------
// Holographic signs.

/// A sign: its corner (0 top-right, 1 bottom-left, 2 top-left, 3 bottom-right), its glyphs
/// (indices into the module's), how many, its color, when it flickers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sign {
    pub corner: u8,
    pub glyphs: [u8; 3],
    pub count: u8,
    pub color: u8,
    pub period: f32,
    pub phase: f32,
}

/// The most signs (one a corner).
pub const MAX_SIGNS: usize = 4;
/// How long a flicker lasts (seconds).
pub const FLICKER_LIFE: f32 = 0.45;

pub fn sign_layout(e: &EffectSpec) -> Vec<Sign> {
    let n = (1 + (3.0 * e.density).round() as usize).min(MAX_SIGNS);
    let colors = e.colors.len().max(1);
    let glyphs = crate::glyphs::HOLO.len() as f32;
    (0..n as u32)
        .map(|i| {
            let g = |j: u32| (hash(i * 23 + j) * glyphs) as u8 % glyphs as u8;
            Sign {
                corner: i as u8,
                glyphs: [g(1), g(2), g(3)],
                count: 2 + u8::from(hash(i * 23 + 4) > 0.5),
                color: (i as usize % colors) as u8,
                period: (2.5 + 10.0 * (1.0 - e.speed)) * (0.7 + 0.6 * hash(i * 23 + 5)),
                phase: hash(i * 23 + 6),
            }
        })
        .collect()
}

/// A sign's glyph's side (physical pixels).
pub fn sign_cell(e: &EffectSpec, scale: f32) -> f32 {
    ((14.0 + 18.0 * e.size) * scale).round()
}

/// Where a sign stands: in its corner, clear of the top bar, the status bar and the tool
/// strips' buttons (behind the side panels, where it shows through them).
pub fn sign_rect(s: &Sign, e: &EffectSpec, window: Rect, scale: f32) -> Rect {
    let cell = sign_cell(e, scale);
    let pad = (cell * 0.35).round();
    let gap = (cell * 0.15).round();
    let count = f32::from(s.count);
    let (w, h) = (cell + 2.0 * pad, cell * count + gap * (count - 1.0) + 2.0 * pad);
    let inset = (64.0 * scale).round();
    let right = matches!(s.corner, 0 | 3);
    let bottom = matches!(s.corner, 1 | 3);
    let x = if right { window.max_x() - inset - w } else { window.min_x() + inset };
    let y = if bottom {
        window.max_y() - (60.0 * scale).round() - h
    } else {
        window.min_y() + (110.0 * scale).round()
    };
    Rect::new(x.round(), y.round(), w, h)
}

/// How lit a sign is at `time`: steady, but now and then a short flicker.
pub fn sign_level(s: &Sign, e: &EffectSpec, time: f32) -> f32 {
    if e.speed <= 0.0 {
        return 1.0;
    }
    let u = (s.phase * s.period + time).rem_euclid(s.period);
    if u >= FLICKER_LIFE {
        return 1.0;
    }
    const PATTERN: [f32; 14] =
        [0.15, 0.9, 0.05, 0.7, 0.3, 1.0, 0.1, 0.85, 0.4, 1.0, 0.2, 0.95, 0.6, 1.0];
    PATTERN[(u * 30.0) as usize % PATTERN.len()]
}

/// The holographic signs: their places, their glyphs' keys, a halo per color.
#[derive(Debug, Default)]
pub struct HoloPainter {
    signs: Vec<Sign>,
    keys: Vec<GlyphKey>,
    keys_for: Option<u16>,
    halos: Vec<ImageData>,
    lost_check: bool,
}

impl HoloPainter {
    pub fn sync_layout(&mut self, e: &EffectSpec) {
        self.signs = sign_layout(e);
    }

    pub fn sync_art(&mut self, e: &EffectSpec) {
        if self.lost_check && !self.halos.iter().any(ImageData::is_lost) {
            return;
        }
        // Small and soft: stretched round each sign.
        self.halos = e.colors.iter().map(|c| image(art::halo(32, *c))).collect();
        self.lost_check = true;
    }

    fn sync_keys(&mut self, size: u16, icons: &icons::Icons) {
        if self.keys_for == Some(size) {
            return;
        }
        self.keys = crate::glyphs::HOLO
            .iter()
            .filter_map(|g| icons.id(g.name))
            .map(|id| icons.key_styled(id, size, icons::IconStyle::Regular))
            .collect();
        self.keys_for = Some(size);
    }

    pub fn paint(&mut self, e: &EffectSpec, f: &FxFrame<'_>, layer: &mut Layer) {
        let Some(icons) = f.icons else { return };
        let cell = sign_cell(e, f.scale);
        self.sync_keys(cell as u16, icons);
        if self.keys.is_empty() {
            return;
        }
        let pad = (cell * 0.35).round();
        let gap = (cell * 0.15).round();
        let tube = (1.2 * f.scale).round().max(1.0);
        for s in &self.signs {
            let r = sign_rect(s, e, f.window, f.scale);
            if hidden(&r, f.covered) {
                continue;
            }
            let level = sign_level(s, e, f.time);
            let color = e.colors.get(usize::from(s.color)).copied().unwrap_or(Rgba::WHITE);
            let a = e.opacity * level;
            if let Some(halo) = self.halos.get(usize::from(s.color)) {
                let spread = cell * 0.9;
                let bounds = Rect::new(
                    r.min_x() - spread,
                    r.min_y() - spread,
                    r.size.width + 2.0 * spread,
                    r.size.height + 2.0 * spread,
                );
                push(layer, halo, bounds, (a * 0.9 * e.glow).min(1.0));
            }
            // The frame: a neon tube round a faint pane.
            layer.quads.push(
                Quad::filled(r, color.with_alpha((a * 0.12).min(1.0)))
                    .rounded(pad * 0.6)
                    .bordered(tube, color.mix(Rgba::WHITE, 0.2).with_alpha((a * 1.8).min(1.0))),
            );
            // The glyphs, a pixel or two off while it flickers (a hologram losing hold).
            let dx =
                if level < 1.0 { ((level * 7.0) as i32 % 3 - 1) as f32 * f.scale } else { 0.0 };
            let glyph = color.mix(Rgba::WHITE, 0.25 * e.glow).with_alpha((a * 2.2).min(1.0));
            for g in 0..usize::from(s.count) {
                let key = self.keys[usize::from(s.glyphs[g]) % self.keys.len()];
                let y = r.min_y() + pad + g as f32 * (cell + gap) + cell;
                layer.glyphs.push(GlyphInstance {
                    key,
                    origin: Point::new((r.min_x() + pad + dx).round(), y.round()),
                    color: glyph,
                });
            }
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.halos.iter()
    }

    pub fn memory(&self) -> usize {
        self.signs.capacity() * std::mem::size_of::<Sign>()
            + self.keys.capacity() * std::mem::size_of::<GlyphKey>()
            + self.halos.capacity() * std::mem::size_of::<ImageData>()
    }
}

// ---------------------------------------------------------------------------------------
