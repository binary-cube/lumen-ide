//! A theme's decorations (`theming::decorations`), as the app draws them: the contract a
//! module contributes ([`DecorationsProvider`] → a [`WindowDecorations`] per window), the
//! pure layout of each kind of layer (where a garland's swags, items and ornaments, the
//! corners and the particles go, from the window's geometry), the animation (each light's
//! twinkle and each particle's place at a time, deterministic) and the frames' clock
//! ([`DecorationClock`]: a low frame rate, only while the window is shown and focused,
//! stopped after a while without input).
//!
//! Everything is in physical pixels. Nothing here draws: the module rasterizes its art once
//! (small images at the device's scale) and places it where the layout says. The layout is
//! worked out again only when the geometry or the decorations change; a frame computes a few
//! sines and pushes the images (no allocation).

use std::sync::Arc;
use std::time::{Duration, Instant};

use kernel::{Point, Rect, Rgba};
use render_api::Layer;
use theming::decorations::{
    Decorations, GarlandEdges, GarlandSpec, Items, ParticleShape, ParticleSpec,
};

/// What the user chose (`[appearance] decorations`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecorationMode {
    /// Drawn, and moving (lights twinkle, snow falls) while the window is in use.
    #[default]
    On,
    /// Drawn as one still frame.
    Static,
    /// Not drawn (nothing made).
    Off,
}

impl DecorationMode {
    pub const ALL: [Self; 3] = [Self::On, Self::Static, Self::Off];

    /// As saved in the settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Static => "static",
            Self::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == s)
    }
}

/// Where the decorations go: the window and what they keep clear of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry<'a> {
    /// The whole window.
    pub window: Rect,
    /// The device's scale (physical pixels per logical one).
    pub scale: f32,
    /// What's left free at the top-left (the system's window buttons, macOS).
    pub top_inset: f32,
    /// The top bar's height (the side garlands start below it when the top-left is taken).
    pub top_bar: f32,
    /// What the top bar shows (the project, the branch, the buttons): ornaments and the
    /// corners never hang over them.
    pub keep_clear: &'a [Rect],
}

/// A frame's decorations: where, what covers them (the cards over the window's background:
/// what's fully under them isn't drawn), the animation's time in seconds, and the app's
/// icons (a corner may hold one).
#[derive(Debug, Clone, Copy)]
pub struct DecorationFrame<'a> {
    pub geometry: Geometry<'a>,
    pub covered: &'a [Rect],
    /// The editor's card alone (what hides a layer shown behind the panels but not the
    /// editor).
    pub editor: &'a [Rect],
    pub time: f32,
    pub icons: Option<&'a icons::Icons>,
}

/// A window's decorations, made by a [`DecorationsProvider`] for a theme: drawn into the
/// layer right above the window's background, behind everything else.
pub trait WindowDecorations: std::fmt::Debug + Send {
    /// Draws the frame. Called on every frame: in steady state it allocates nothing.
    fn paint(&mut self, frame: &DecorationFrame<'_>, layer: &mut Layer);
    /// Whether anything moves (else one frame is enough).
    fn moves(&self) -> bool;
    /// The bytes it holds in memory (its layout, its art not yet handed to the GPU).
    fn memory(&self) -> usize;
    /// The bytes its art takes on the GPU.
    fn gpu_memory(&self) -> u64;
    /// How the panels let it show through them (none: it stays around them).
    fn see_through(&self) -> Option<theming::decorations::SeeThrough> {
        None
    }
}

/// The contrast the text keeps over what shows through a card (the themes' own rule): the
/// main text 4.5:1, the muted text 2.8:1 — or what each had on the opaque card, if less.
pub const TEXT_CONTRAST: f32 = 4.5;
pub const MUTED_CONTRAST: f32 = 2.8;

/// How opaque a card's background must be over what's drawn behind it: at least `wanted`
/// (the effects' glass), more if needed so that every text color keeps its contrast with
/// what shows through (each with its least: [`TEXT_CONTRAST`], [`MUTED_CONTRAST`] — or what
/// it had on the opaque card, if that was less). `under` is the window's background,
/// `behind` the colors drawn on it with their opacities.
pub fn glass_alpha(
    card: Rgba,
    texts: &[(Rgba, f32)],
    under: Rgba,
    behind: &[(Rgba, f32)],
    wanted: f32,
) -> f32 {
    use crate::severity::contrast;
    let mut alpha = wanted.clamp(0.0, 1.0);
    while alpha < 1.0 {
        let holds = texts.iter().all(|&(t, least)| {
            let target = contrast(t, card).min(least) - 1e-3;
            let over = |seen: Rgba| contrast(t, seen.with_alpha(1.0)) >= target;
            over(under.mix(card, alpha))
                && behind
                    .iter()
                    .all(|(c, o)| over(under.mix(*c, o.clamp(0.0, 1.0)).mix(card, alpha)))
        });
        if holds {
            break;
        }
        alpha = (alpha + 0.01).min(1.0);
    }
    alpha
}

/// What a module contributes to draw the themes' decorations.
pub trait DecorationsProvider: Send + Sync + std::fmt::Debug {
    /// The window's decorations for a theme's, with the user's own over or instead of them
    /// (`None`: nothing to draw, or the user turned them off).
    fn create(&self, decorations: &Arc<Decorations>) -> Option<Box<dyn WindowDecorations>>;
    /// Exactly these decorations, whatever the user chose (a preview's: the studio's).
    fn preview(&self, decorations: &Arc<Decorations>) -> Option<Box<dyn WindowDecorations>> {
        self.create(decorations)
    }
    /// What the user chose.
    fn mode(&self) -> DecorationMode;
    /// Changes whenever the choice does (the windows make theirs again).
    fn version(&self) -> u64;
    /// The most frames a second while they move.
    fn fps(&self) -> u32 {
        FPS
    }
    /// How long without input before they stop (`None`: never).
    fn idle(&self) -> Option<Duration> {
        Some(IDLE_STOP)
    }
    /// Whether they stand still when the system asks for less motion or saves power.
    fn respects_system(&self) -> bool {
        true
    }
}

/// The provider the app found among the modules' contributions, for the views that preview
/// decorations (the studio): registered once at startup (as the services' providers are).
static PROVIDER: std::sync::RwLock<Option<Arc<dyn DecorationsProvider>>> =
    std::sync::RwLock::new(None);

/// Registers the decorations' provider (the app, at startup).
pub fn register(provider: Arc<dyn DecorationsProvider>) {
    if let Ok(mut p) = PROVIDER.write() {
        *p = Some(provider);
    }
}

/// The decorations' provider, if a module contributed one.
pub fn provider() -> Option<Arc<dyn DecorationsProvider>> {
    PROVIDER.read().ok().and_then(|p| p.clone())
}

/// The system asks for less motion (told by the app, which asks it every few seconds),
/// for the views animating on their own (Relax mode's scenes).
static REDUCE_MOTION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the system says of motion (the app, when it asks).
pub fn set_reduce_motion(reduce: bool) {
    REDUCE_MOTION.store(reduce, std::sync::atomic::Ordering::Relaxed);
}

/// The system asks for less motion.
pub fn reduce_motion() -> bool {
    REDUCE_MOTION.load(std::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------------------
// The layout.

/// A swag's target width, in logical pixels (the top is cut into equal swags).
pub const SWAG_WIDTH: f32 = 150.0;
/// A swag's image height, in logical pixels (the strand and the bulbs hanging from it).
pub const SWAG_HEIGHT: f32 = 18.0;
/// Where a swag's strand is fixed at both ends, and how far it sags (logical pixels).
pub const SWAG_ANCHOR: f32 = 1.5;
pub const SWAG_SAG: f32 = 7.5;
/// A side run's tile: its width and length (logical pixels).
pub const SIDE_WIDTH: f32 = 14.0;
pub const SIDE_TILE: f32 = 90.0;
/// Where a side strand runs from the window's edge, and how far it waves (logical).
pub const SIDE_X: f32 = 4.5;
pub const SIDE_WAVE: f32 = 1.6;
/// Lights in a side tile.
pub const SIDE_LIGHTS: usize = 4;
/// An ornament's image (thread, cap and globe), logical pixels.
pub const ORNAMENT_WIDTH: f32 = 10.0;
pub const ORNAMENT_HEIGHT: f32 = 15.0;
/// A corner ornament's side, logical pixels.
pub const CORNER: f32 = 16.0;
/// A sparkle's (a light's halo while it twinkles) side, logical pixels.
pub const SPARKLE: f32 = 13.0;
/// The flakes' radii by size, logical pixels.
pub const FLAKE_RADII: [f32; 3] = [1.2, 1.7, 2.5];
/// The most flakes, whatever the window's size.
pub const MAX_FLAKES: usize = 180;

/// A swag's shape (physical pixels, in its image): the strand's curve and its items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwagShape {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    /// Items along it (even: none at its middle, where an ornament hangs).
    pub lights: usize,
    /// How far below the strand an item's middle is (logical pixels).
    pub hang: f32,
}

/// How far below its string an item's middle hangs (logical pixels).
pub fn hang(items: Items) -> f32 {
    match items {
        Items::Bulbs => 2.6,
        Items::Beads => 0.3,
        Items::Flags | Items::Nothing => 0.0,
    }
}

impl SwagShape {
    /// The strand's middle at `x` (0..width): a parabola from the anchors to the sag.
    pub fn strand_y(&self, x: f32) -> f32 {
        let u = (x / self.width.max(1.0)) * 2.0 - 1.0;
        (SWAG_ANCHOR + SWAG_SAG * (1.0 - u * u)) * self.scale
    }

    /// The strand's slope at `x`.
    pub fn slope(&self, x: f32) -> f32 {
        let u = (x / self.width.max(1.0)) * 2.0 - 1.0;
        -2.0 * u * SWAG_SAG * self.scale * 2.0 / self.width.max(1.0)
    }

    /// Where item `k` hangs from the strand (its middle).
    pub fn light(&self, k: usize) -> Point {
        let x = self.width * (k as f32 + 0.5) / self.lights.max(1) as f32;
        Point::new(x, self.strand_y(x) + self.hang * self.scale)
    }

    /// The point the ornament hangs from (the swag's lowest).
    pub fn middle(&self) -> Point {
        Point::new(self.width / 2.0, self.strand_y(self.width / 2.0))
    }
}

/// A side tile's strand (physical pixels, in its image): a gentle wave down the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideShape {
    pub width: f32,
    pub length: f32,
    pub scale: f32,
    /// How far beside the strand an item's middle is (logical pixels).
    pub hang: f32,
}

impl SideShape {
    /// The strand's middle at `y` (0..length): one whole wave a tile, so tiles join.
    pub fn strand_x(&self, y: f32) -> f32 {
        let phase = y / self.length.max(1.0) * std::f32::consts::TAU;
        (SIDE_X + SIDE_WAVE * phase.sin()) * self.scale
    }

    /// Where item `k` hangs (its middle, beside the strand, toward the inside).
    pub fn light(&self, k: usize) -> Point {
        let y = self.length * (k as f32 + 0.5) / SIDE_LIGHTS as f32;
        Point::new(self.strand_x(y) + self.hang * self.scale, y + 1.2 * self.scale)
    }
}

/// An item on a garland: where its middle is, its color (index into the layer's), and its
/// twinkle's phase and rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub center: Point,
    pub color: u8,
    pub phase: f32,
    pub rate: f32,
}

/// An ornament hanging from the garland: the point it hangs from and its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ornament {
    pub hook: Point,
    pub color: u8,
}

/// A side tile: its top-left corner; mirrored on the right edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideTile {
    pub origin: Point,
    pub right: bool,
}

/// A particle: where it starts (fractions of the window), its size, how fast it falls (a
/// factor), how it sways, and its color (index into the layer's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub size: u8,
    pub fall: f32,
    pub sway: f32,
    pub period: f32,
    pub phase: f32,
    pub color: u8,
}

/// Where a garland's parts go, for a geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GarlandLayout {
    pub swag: Option<SwagShape>,
    /// Each swag's top-left corner.
    pub swags: Vec<Point>,
    pub side: Option<SideShape>,
    pub sides: Vec<SideTile>,
    pub lights: Vec<Light>,
    pub ornaments: Vec<Ornament>,
}

/// A small deterministic hash (the same layout every run: the snapshots).
pub fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn overlaps(r: &Rect, rects: &[Rect]) -> bool {
    rects.iter().any(|k| r.intersect(k).is_some())
}

/// Where a garland goes (see the module's notes).
pub fn garland_layout(garland: &GarlandSpec, g: &Geometry<'_>) -> GarlandLayout {
    let s = g.scale.max(0.5);
    let w = g.window;
    let mut out = GarlandLayout::default();
    if w.is_empty() {
        return out;
    }
    let colors = garland.colors.len().max(1) as u8;
    let ornament_colors = garland.ornaments.len() as u8;
    let items = garland.has_items();
    // The top: equal swags from the inset to the right edge.
    let start = w.min_x() + g.top_inset;
    let span = (w.max_x() - start).max(0.0);
    let count = (span / (SWAG_WIDTH * s)).round().max(1.0);
    let width = span / count;
    // 4..8 items a swag, always even.
    let lights = 2 * (2.0 + 2.0 * garland.lights).round() as usize;
    let shape = SwagShape {
        width: width.ceil(),
        height: (SWAG_HEIGHT * s).ceil(),
        scale: s,
        lights,
        hang: hang(garland.items),
    };
    let mut index = 0u32;
    for i in 0..count as usize {
        let origin = Point::new((start + width * i as f32).floor(), w.min_y());
        out.swags.push(origin);
        if items {
            for k in 0..lights {
                let p = shape.light(k);
                out.lights.push(light(origin, p, index, colors));
                index += 1;
            }
        }
        if ornament_colors > 0 {
            let m = shape.middle();
            let hook = Point::new(origin.x + m.x, origin.y + m.y);
            let r = ornament_rect(hook, s);
            if !overlaps(&r, g.keep_clear) {
                out.ornaments.push(Ornament { hook, color: i as u8 % ornament_colors });
            }
        }
    }
    out.swag = Some(shape);
    // The sides: whole tiles down from the top corners.
    if garland.edges == GarlandEdges::TopSides {
        let side = SideShape {
            width: (SIDE_WIDTH * s).ceil(),
            length: (SIDE_TILE * s).round(),
            scale: s,
            hang: hang(garland.items),
        };
        let reach = w.size.height * garland.drop;
        let tiles = (reach / side.length).floor() as usize;
        for right in [false, true] {
            // On the left, the system's buttons take the corner: the run hangs from
            // below the top bar.
            let top = if !right && g.top_inset > 0.0 { g.top_bar } else { 0.0 };
            let x = if right { w.max_x() - side.width } else { w.min_x() };
            let fits = ((reach - top) / side.length).floor().max(0.0) as usize;
            for t in 0..tiles.min(fits) {
                let origin = Point::new(x, w.min_y() + top + side.length * t as f32);
                out.sides.push(SideTile { origin, right });
                if items {
                    for k in 0..SIDE_LIGHTS {
                        let mut p = side.light(k);
                        if right {
                            p.x = side.width - p.x;
                        }
                        out.lights.push(light(origin, p, index, colors));
                        index += 1;
                    }
                }
            }
            // An ornament at the end of each run.
            let n = tiles.min(fits);
            if n > 0 && ornament_colors > 0 {
                let end = w.min_y() + top + side.length * n as f32;
                let sx = side.strand_x(side.length);
                let hx = if right { x + side.width - sx } else { x + sx };
                out.ornaments.push(Ornament {
                    hook: Point::new(hx, end),
                    color: (n as u8 + u8::from(right)) % ornament_colors,
                });
            }
        }
        out.side = Some(side);
    }
    out
}

/// Where the top corners' ornaments go: each one's top-left corner (mirrored on the
/// right), none over what the top bar shows.
pub fn corners_layout(g: &Geometry<'_>) -> Vec<(Point, bool)> {
    let s = g.scale.max(0.5);
    let w = g.window;
    let mut out = Vec::new();
    if w.is_empty() {
        return out;
    }
    let side = (CORNER * s).round();
    for right in [false, true] {
        let x = if right { w.max_x() - side } else { w.min_x() + g.top_inset };
        let r = Rect::new(x, w.min_y(), side, side);
        if !overlaps(&r, g.keep_clear) {
            out.push((r.origin, right));
        }
    }
    out
}

/// How many particles of `shape` a layer has at full density on a 1200 × 700 window.
fn particles_at_full(shape: ParticleShape) -> f32 {
    match shape {
        ParticleShape::Stars => 70.0,
        ParticleShape::Sparkles => 50.0,
        ParticleShape::Rain => 170.0,
        _ => 150.0,
    }
}

/// Where a particles layer's particles start (see the module's notes).
pub fn particles_layout(p: &ParticleSpec, g: &Geometry<'_>) -> Vec<Particle> {
    let s = g.scale.max(0.5);
    let w = g.window;
    if w.is_empty() {
        return Vec::new();
    }
    // About 150 flakes on a 1200 × 700 window at full density, fewer on a smaller one;
    // most fall behind the cards (not drawn), a few show around them.
    let area = (w.size.width / s) * (w.size.height / s) / (1200.0 * 700.0);
    let n = ((p.density * particles_at_full(p.shape) * area).round() as usize).min(MAX_FLAKES);
    let colors = p.colors.len().max(1) as u32;
    (0..n as u32)
        .map(|i| {
            let size = match hash(i * 7 + 1) {
                v if v < 0.55 => 0,
                v if v < 0.88 => 1,
                _ => 2,
            };
            Particle {
                x: hash(i * 7 + 2),
                y: hash(i * 7 + 3),
                size,
                fall: 0.65 + 0.7 * hash(i * 7 + 4) + 0.15 * f32::from(size),
                sway: 4.0 + 9.0 * hash(i * 7 + 5),
                period: 5.0 + 5.0 * hash(i * 7 + 6),
                phase: hash(i * 7 + 7) * std::f32::consts::TAU,
                color: (i % colors) as u8,
            }
        })
        .collect()
}

fn light(origin: Point, local: Point, index: u32, colors: u8) -> Light {
    Light {
        center: Point::new(origin.x + local.x, origin.y + local.y),
        color: (index % u32::from(colors)) as u8,
        phase: hash(index * 3 + 11),
        rate: 0.6 + 0.8 * hash(index * 3 + 12),
    }
}

/// An ornament's image rect, hanging from `hook` (its thread's top at the hook).
pub fn ornament_rect(hook: Point, scale: f32) -> Rect {
    let (w, h) = ((ORNAMENT_WIDTH * scale).ceil(), (ORNAMENT_HEIGHT * scale).ceil());
    Rect::new((hook.x - w / 2.0).round(), hook.y.round(), w, h)
}

/// A sparkle's image rect around a light.
pub fn sparkle_rect(center: Point, scale: f32) -> Rect {
    let side = (SPARKLE * scale).ceil();
    Rect::new((center.x - side / 2.0).round(), (center.y - side / 2.0).round(), side, side)
}

/// How far in a glow reaches from the edges (physical pixels), for its width 0..1.
pub fn glow_reach(width: f32, scale: f32) -> f32 {
    ((10.0 + 54.0 * width.clamp(0.0, 1.0)) * scale.max(0.5)).round().max(1.0)
}

// ---------------------------------------------------------------------------------------
// The animation.

/// How bright a light's twinkle is at `time` (0: just its steady glow, 1: brightest):
/// brief, rare swells, each light on its own rhythm; `speed` 0..1 (0: none).
pub fn twinkle(light: &Light, time: f32, speed: f32) -> f32 {
    swell(light.phase, light.rate, time, speed)
}

/// A brief, rare swell (0..1) on a rhythm of its own (`phase` 0..1, `rate` ~0.6..1.4).
fn swell(phase: f32, rate: f32, time: f32, speed: f32) -> f32 {
    if speed <= 0.0 {
        return 0.0;
    }
    // Every 3.5–10 s it swells, for about a sixth of its cycle.
    let cycle = (10.0 - 6.5 * speed) / rate;
    const SWELL: f32 = 0.16;
    let t = (time / cycle + phase).fract();
    if t >= SWELL {
        return 0.0;
    }
    let level = (t / SWELL * std::f32::consts::PI).sin().powi(2);
    if level < 0.02 { 0.0 } else { level }
}

/// How a falling shape falls: its speed at rest and its gain (logical pixels a second),
/// and how much it sways.
fn fall_of(shape: ParticleShape) -> (f32, f32, f32) {
    match shape {
        ParticleShape::Snow => (6.0, 22.0, 1.0),
        ParticleShape::Leaves => (5.0, 15.0, 2.2),
        ParticleShape::Petals => (5.0, 15.0, 1.7),
        ParticleShape::Confetti => (8.0, 24.0, 1.2),
        ParticleShape::Rain => (110.0, 260.0, 0.0),
        ParticleShape::Stars | ParticleShape::Sparkles => (0.0, 0.0, 0.0),
    }
}

/// A flake's middle at `time`, falling through `window` (and wrapping to the top), at
/// `speed` 0..1 (snow's motion: [`particle_at`] for any shape).
pub fn flake_at(flake: &Particle, window: Rect, scale: f32, time: f32, speed: f32) -> Point {
    particle_at(flake, ParticleShape::Snow, window, scale, time, speed)
}

/// A particle's middle at `time`: a falling one through `window` (wrapping to the top),
/// at `speed` 0..1; stars and sparkles stay where they are.
pub fn particle_at(
    p: &Particle,
    shape: ParticleShape,
    window: Rect,
    scale: f32,
    time: f32,
    speed: f32,
) -> Point {
    if !shape.falls() {
        return Point::new(
            window.min_x() + p.x * window.size.width,
            window.min_y() + p.y * window.size.height,
        );
    }
    let s = scale.max(0.5);
    let (rest, gain, sway) = fall_of(shape);
    let margin = 8.0 * s * if shape == ParticleShape::Rain { 2.0 } else { 1.0 };
    let height = window.size.height + 2.0 * margin;
    let fall = (rest + gain * speed) * s * p.fall;
    let y = (p.y * height + fall * time).rem_euclid(height) - margin;
    let sway = sway * p.sway * s * (time / p.period * std::f32::consts::TAU + p.phase).sin();
    let x = (p.x * window.size.width + sway).rem_euclid(window.size.width);
    Point::new(window.min_x() + x, window.min_y() + y)
}

/// Which of its few drawn orientations a tumbling particle shows at `time` (0..4).
pub fn tumble(p: &Particle, time: f32) -> usize {
    ((time / p.period * 4.0 + p.phase).floor() as i64).rem_euclid(4) as usize
}

/// How bright a particle is at `time` (0: not drawn): stars glow softly in turn, sparkles
/// swell briefly and rarely, the rest steady.
pub fn particle_level(p: &Particle, shape: ParticleShape, time: f32, speed: f32) -> f32 {
    match shape {
        ParticleShape::Stars if speed > 0.0 => {
            let rate = (0.25 + 0.75 * speed) * (0.7 + 0.6 * p.x);
            0.6 + 0.4 * (time * rate * std::f32::consts::TAU / p.period * 2.0 + p.phase).sin()
        }
        ParticleShape::Sparkles => {
            swell(p.phase / std::f32::consts::TAU, 0.6 + 0.8 * p.y, time, speed.max(0.2))
        }
        _ => 1.0,
    }
}

/// Whether `r` lies wholly under one of `covered` (then it isn't drawn).
pub fn hidden(r: &Rect, covered: &[Rect]) -> bool {
    covered.iter().any(|c| {
        r.min_x() >= c.min_x()
            && r.max_x() <= c.max_x()
            && r.min_y() >= c.min_y()
            && r.max_y() <= c.max_y()
    })
}

// ---------------------------------------------------------------------------------------
// The frames' clock.

/// Frames a second while the decorations move, unless the user chose otherwise.
pub const FPS: u32 = 20;
/// The frame rates the user may choose.
pub const FPS_CHOICES: [u32; 4] = [10, 15, 20, 30];
/// The most frames a second, whatever is asked.
pub const MAX_FPS: u32 = 30;
/// No input for this long: they stop moving (a still frame, no more frames asked).
pub const IDLE_STOP: Duration = Duration::from_secs(20);
/// The pauses the user may choose ("Pause theme effects when idle", seconds; 0: never —
/// still paused while the window is hidden and under a view covering it).
pub const IDLE_CHOICES: [i64; 4] = [20, 60, 300, 0];

/// A pause's name (`Never` for 0).
pub fn idle_label(secs: i64) -> String {
    match secs {
        0 => i18n::t("settings.decorations_idle_never"),
        s if s >= 60 && s % 60 == 0 => i18n::tr!("settings.decorations_idle_minutes", n = s / 60),
        s => i18n::tr!("settings.decorations_idle_secs", n = s),
    }
}
/// The time a still frame shows (static mode, reduced motion): a moment where a few lights
/// glow.
pub const STATIC_TIME: f32 = 7.3;

/// Whether the decorations may move now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moving {
    /// The user chose them moving (not static), the system doesn't ask for less motion and
    /// isn't saving power.
    pub allowed: bool,
    /// Anything in them moves.
    pub moves: bool,
    /// The window has the keyboard.
    pub focused: bool,
    /// The window is shown (not hidden, minimized, on another desktop).
    pub visible: bool,
    /// The most frames a second (at most [`MAX_FPS`]).
    pub fps: u32,
    /// How long without input before they stop (`None`: never).
    pub idle: Option<Duration>,
}

impl Moving {
    /// Moving as allowed, at the default rate and idle time.
    pub fn new(allowed: bool, moves: bool, focused: bool, visible: bool) -> Self {
        Self { allowed, moves, focused, visible, fps: FPS, idle: Some(IDLE_STOP) }
    }
}

/// The animation's time and when its next frame is due.
#[derive(Debug, Clone, Copy)]
pub struct DecorationClock {
    time: f32,
    last: Option<Instant>,
    last_input: Instant,
}

impl DecorationClock {
    pub fn new(now: Instant) -> Self {
        Self { time: 0.0, last: None, last_input: now }
    }

    /// Input came (a key, the mouse): they move again (from where they stopped).
    pub fn input(&mut self, now: Instant) {
        self.last_input = now;
    }

    /// Whether they're moving now.
    pub fn running(&self, now: Instant, m: Moving) -> bool {
        m.allowed
            && m.moves
            && m.focused
            && m.visible
            && m.idle.is_none_or(|idle| now.saturating_duration_since(self.last_input) < idle)
    }

    /// The time to draw at `now`: advanced while running (by at most a quarter of a second
    /// a frame: no jump after a pause), frozen otherwise; static: [`STATIC_TIME`].
    pub fn advance(&mut self, now: Instant, m: Moving) -> f32 {
        if !m.allowed {
            self.last = None;
            return STATIC_TIME;
        }
        if self.running(now, m) {
            if let Some(last) = self.last {
                let step = now.saturating_duration_since(last).as_secs_f32().min(0.25);
                self.time += step;
            }
            self.last = Some(now);
        } else {
            self.last = None;
        }
        self.time
    }

    /// When the next frame is due (`None`: none — still, hidden, unfocused or idle).
    pub fn next_frame(&self, now: Instant, m: Moving) -> Option<Instant> {
        self.running(now, m).then(|| now + Duration::from_secs(1) / m.fps.clamp(1, MAX_FPS))
    }

    /// The time it shows (tests).
    pub fn time(&self) -> f32 {
        self.time
    }
}

#[cfg(test)]
#[path = "../tests/unit/decorations.rs"]
mod tests;
