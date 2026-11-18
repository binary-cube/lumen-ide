//! A window's decorations: one painter per layer, in the theme's order. The layouts are
//! worked out again only when the window's geometry changes, the art rasterized only when
//! the scale (or a swag's width, in steps of [`WIDTH_STEP`]) does or the GPU let it go; a
//! frame then only places the images it has — the swags, the side tiles, the ornaments and
//! corners, the sparkles of the items twinkling now, the particles not hidden under a card,
//! the glow's strips — allocating nothing.

use std::sync::Arc;

use kernel::{Point, Rect};
use render_api::{GlyphInstance, ImageData, ImageInstance, Layer};
use theming::decorations::{
    CornerSpec, CornerStyle, Decorations, GarlandSpec, GlowSpec, LayerSpec, ParticleShape,
    ParticleSpec,
};
use ui_components::decorations::{
    CORNER, DecorationFrame, GarlandLayout, Geometry, Particle, WindowDecorations, corners_layout,
    garland_layout, glow_reach, hidden, ornament_rect, particle_at, particle_level,
    particles_layout, sparkle_rect, tumble, twinkle,
};

use crate::art;
use crate::fx::{FxFrame, FxPainter};
use theming::decorations::Zone;

/// A swag's image is made for widths rounded up to this many pixels (then stretched by less
/// than that into place), so resizing the window doesn't draw it again every pixel.
pub const WIDTH_STEP: f32 = 8.0;

/// What the layouts were worked out for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LayoutKey {
    window: Rect,
    scale: f32,
    top_inset: f32,
    top_bar: f32,
}

/// An image whose pixels leave memory once on the GPU.
fn image(canvas: art::Canvas) -> ImageData {
    let (w, h) = (canvas.width, canvas.height);
    ImageData::gpu_only(w, h, canvas.into_rgba8())
}

fn push(layer: &mut Layer, image: &ImageData, bounds: Rect, opacity: f32) {
    if opacity > 0.0 {
        layer.images.push(ImageInstance { image: image.clone(), bounds, opacity });
    }
}

/// A garland: its layout and its images.
#[derive(Debug, Default)]
struct GarlandPainter {
    layout: GarlandLayout,
    swag: Option<ImageData>,
    side_left: Option<ImageData>,
    side_right: Option<ImageData>,
    ornaments: Vec<ImageData>,
    sparkles: Vec<ImageData>,
    /// The scale and the swag's width the images were drawn for.
    art_key: Option<(f32, f32)>,
}

impl GarlandPainter {
    fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.swag
            .iter()
            .chain(&self.side_left)
            .chain(&self.side_right)
            .chain(&self.ornaments)
            .chain(&self.sparkles)
    }

    fn sync_art(&mut self, g: &GarlandSpec, scale: f32) {
        let swag_width =
            self.layout.swag.map_or(0.0, |s| (s.width / WIDTH_STEP).ceil() * WIDTH_STEP);
        let key = (scale, swag_width);
        if self.art_key == Some(key) && !self.images().any(ImageData::is_lost) {
            return;
        }
        self.swag = self.layout.swag.map(|mut shape| {
            shape.width = swag_width;
            image(art::swag(&shape, g))
        });
        self.side_left = self.layout.side.map(|side| image(art::side(&side, g)));
        self.side_right = self.layout.side.map(|side| image(art::side(&side, g).mirrored()));
        self.ornaments = g.ornaments.iter().map(|c| image(art::ornament(scale, *c))).collect();
        self.sparkles = if g.twinkle > 0.0 && g.has_items() && g.items.glints() {
            g.colors.iter().map(|c| image(art::sparkle(scale, *c))).collect()
        } else {
            Vec::new()
        };
        self.art_key = Some(key);
    }

    fn paint(&self, g: &GarlandSpec, frame: &DecorationFrame<'_>, layer: &mut Layer) {
        let s = frame.geometry.scale;
        let l = &self.layout;
        let opacity = g.opacity;
        if let (Some(image), Some(shape)) = (&self.swag, &l.swag) {
            for origin in &l.swags {
                let bounds = Rect::new(origin.x, origin.y, shape.width.ceil(), shape.height);
                push(layer, image, bounds, opacity);
            }
        }
        if let Some(side) = &l.side {
            for tile in &l.sides {
                let image = if tile.right { &self.side_right } else { &self.side_left };
                if let Some(image) = image {
                    let bounds = Rect::new(tile.origin.x, tile.origin.y, side.width, side.length);
                    push(layer, image, bounds, opacity);
                }
            }
        }
        for o in &l.ornaments {
            if let Some(image) = self.ornaments.get(usize::from(o.color)) {
                push(layer, image, ornament_rect(o.hook, s), opacity);
            }
        }
        // The items swelling now: their sparkle over the steady one.
        if !self.sparkles.is_empty() {
            for light in &l.lights {
                let level = twinkle(light, frame.time, g.twinkle);
                if level > 0.0
                    && let Some(image) = self.sparkles.get(usize::from(light.color))
                {
                    push(layer, image, sparkle_rect(light.center, s), level * opacity);
                }
            }
        }
    }
}

/// How many images a particle shape has per color (its sizes, or its orientations).
fn variants(shape: ParticleShape) -> usize {
    match shape {
        ParticleShape::Snow => 3,
        ParticleShape::Leaves | ParticleShape::Petals | ParticleShape::Confetti => 4,
        ParticleShape::Stars | ParticleShape::Sparkles | ParticleShape::Rain => 1,
    }
}

/// The scale a particle of size 0..=2 is drawn at (snow has an image per size).
fn size_factor(shape: ParticleShape, size: u8) -> f32 {
    if shape == ParticleShape::Snow {
        return 1.0;
    }
    [0.75, 1.0, 1.3][usize::from(size.min(2))]
}

/// A particles layer: where they start and their images (per color, its variants).
#[derive(Debug, Default)]
struct ParticlePainter {
    particles: Vec<Particle>,
    sprites: Vec<ImageData>,
    art_key: Option<f32>,
}

impl ParticlePainter {
    fn sync_art(&mut self, p: &ParticleSpec, scale: f32) {
        if self.art_key == Some(scale) && !self.sprites.iter().any(ImageData::is_lost) {
            return;
        }
        let n = variants(p.shape);
        self.sprites = p
            .colors
            .iter()
            .flat_map(|c| (0..n).map(move |v| image(art::particle(p.shape, scale, v, *c))))
            .collect();
        self.art_key = Some(scale);
    }

    fn paint(
        &self,
        p: &ParticleSpec,
        window: Rect,
        covered: &[Rect],
        frame: &DecorationFrame<'_>,
        layer: &mut Layer,
    ) {
        let s = frame.geometry.scale;
        let n = variants(p.shape);
        for particle in &self.particles {
            let level = particle_level(particle, p.shape, frame.time, p.speed);
            if level <= 0.0 {
                continue;
            }
            let variant = match p.shape {
                ParticleShape::Snow => usize::from(particle.size.min(2)),
                shape if shape.tumbles() => tumble(particle, frame.time),
                _ => 0,
            };
            let Some(image) = self.sprites.get(usize::from(particle.color) * n + variant) else {
                continue;
            };
            let at = particle_at(particle, p.shape, window, s, frame.time, p.speed);
            let k = size_factor(p.shape, particle.size);
            let (w, h) = (image.width as f32 * k, image.height as f32 * k);
            let bounds = Rect::new(at.x - w / 2.0, at.y - h / 2.0, w, h);
            // Behind a card: nothing to draw.
            if hidden(&bounds, covered) {
                continue;
            }
            let faint = 0.8 + 0.1 * f32::from(particle.size);
            push(layer, image, bounds, p.opacity * faint * level);
        }
    }
}

/// The top corners: their places and their two images (or an icon of the app's).
#[derive(Debug, Default)]
struct CornerPainter {
    places: Vec<(Point, bool)>,
    left: Option<ImageData>,
    right: Option<ImageData>,
    art_key: Option<f32>,
    /// The icon found for its name (looked up once).
    icon: Option<icons::IconId>,
    looked_up: bool,
}

impl CornerPainter {
    fn sync_art(&mut self, c: &CornerSpec, scale: f32) {
        if c.style == CornerStyle::Icon {
            self.left = None;
            self.right = None;
            return;
        }
        let lost = self.left.iter().chain(&self.right).any(ImageData::is_lost);
        if self.art_key == Some(scale) && !lost {
            return;
        }
        self.left = Some(image(art::corner(scale, c)));
        self.right = Some(image(art::corner(scale, c).mirrored()));
        self.art_key = Some(scale);
    }

    fn paint(&mut self, c: &CornerSpec, frame: &DecorationFrame<'_>, layer: &mut Layer) {
        if c.style == CornerStyle::Icon {
            let Some(icons) = frame.icons else { return };
            if !self.looked_up {
                self.icon = icons.id(&c.icon);
                self.looked_up = true;
            }
            let Some(id) = self.icon else { return };
            let side = (CORNER * frame.geometry.scale).round();
            let inset = (side * 0.1).round();
            let size = side - 2.0 * inset;
            let color = c.color.with_alpha(c.color.a * c.opacity);
            for (at, _) in &self.places {
                layer.glyphs.push(GlyphInstance {
                    key: icons.key(id, size as u16),
                    origin: Point::new(at.x + inset, at.y + inset + size),
                    color,
                });
            }
            return;
        }
        for (at, right) in &self.places {
            let image = if *right { &self.right } else { &self.left };
            if let Some(image) = image {
                let side = image.width as f32;
                push(layer, image, Rect::new(at.x, at.y, side, side), c.opacity);
            }
        }
    }
}

/// The glow: four strips (the top, the bottom, the left, the right edge), each a gradient
/// stretched along its edge.
#[derive(Debug, Default)]
struct GlowPainter {
    strips: [Option<ImageData>; 4],
    art_key: Option<f32>,
}

impl GlowPainter {
    fn sync_art(&mut self, g: &GlowSpec, scale: f32) {
        let reach = glow_reach(g.width, scale);
        let lost = self.strips.iter().flatten().any(ImageData::is_lost);
        if self.art_key == Some(reach) && !lost {
            return;
        }
        self.strips = art::glow_strips(g, reach).map(|c| Some(image(c)));
        self.art_key = Some(reach);
    }

    fn paint(&self, g: &GlowSpec, window: Rect, scale: f32, layer: &mut Layer) {
        let reach = glow_reach(g.width, scale).min(window.size.height / 2.0);
        let w = window;
        let rects = [
            Rect::new(w.min_x(), w.min_y(), w.size.width, reach),
            Rect::new(w.min_x(), w.max_y() - reach, w.size.width, reach),
            Rect::new(w.min_x(), w.min_y(), reach, w.size.height),
            Rect::new(w.max_x() - reach, w.min_y(), reach, w.size.height),
        ];
        for (strip, bounds) in self.strips.iter().zip(rects) {
            if let Some(image) = strip {
                push(layer, image, bounds, g.opacity);
            }
        }
    }
}

/// One layer's painter (its kind's).
#[derive(Debug)]
enum LayerPainter {
    Garland(GarlandPainter),
    Particles(ParticlePainter),
    Corners(CornerPainter),
    Glow(GlowPainter),
    Effect(FxPainter),
    /// A kind this module doesn't draw (none so far).
    Nothing,
}

/// What hides a layer in its zone: every card (around the panels), only the editor (behind
/// the panels too), nothing (everywhere).
fn covered_in<'a>(zone: Zone, frame: &DecorationFrame<'a>) -> &'a [Rect] {
    match zone {
        Zone::Edges => frame.covered,
        Zone::Panels => frame.editor,
        Zone::Everywhere => &[],
    }
}

impl LayerPainter {
    fn new(spec: &LayerSpec) -> Self {
        match spec {
            LayerSpec::Garland(_) => Self::Garland(GarlandPainter::default()),
            LayerSpec::Particles(_) => Self::Particles(ParticlePainter::default()),
            LayerSpec::Corners(_) => Self::Corners(CornerPainter::default()),
            LayerSpec::Glow(_) => Self::Glow(GlowPainter::default()),
            LayerSpec::Effect(e) => FxPainter::new(e.kind).map_or(Self::Nothing, Self::Effect),
        }
    }

    fn images(&self) -> Box<dyn Iterator<Item = &ImageData> + '_> {
        match self {
            Self::Garland(p) => Box::new(p.images()),
            Self::Particles(p) => Box::new(p.sprites.iter()),
            Self::Corners(p) => Box::new(p.left.iter().chain(&p.right)),
            Self::Glow(p) => Box::new(p.strips.iter().flatten()),
            Self::Effect(p) => p.images(),
            Self::Nothing => Box::new(std::iter::empty()),
        }
    }

    /// The bytes its layout holds.
    fn layout_memory(&self) -> usize {
        use std::mem::size_of;
        use ui_components::decorations::{Light, Ornament, SideTile};
        match self {
            Self::Garland(p) => {
                let l = &p.layout;
                l.swags.capacity() * size_of::<Point>()
                    + l.sides.capacity() * size_of::<SideTile>()
                    + l.lights.capacity() * size_of::<Light>()
                    + l.ornaments.capacity() * size_of::<Ornament>()
                    + (p.ornaments.capacity() + p.sparkles.capacity()) * size_of::<ImageData>()
            }
            Self::Particles(p) => {
                p.particles.capacity() * size_of::<Particle>()
                    + p.sprites.capacity() * size_of::<ImageData>()
            }
            Self::Corners(p) => p.places.capacity() * size_of::<(Point, bool)>(),
            Self::Glow(_) | Self::Nothing => 0,
            Self::Effect(p) => p.memory(),
        }
    }
}

/// A window's decorations (see the module's notes).
#[derive(Debug)]
pub struct Painter {
    decorations: Arc<Decorations>,
    layers: Vec<LayerPainter>,
    layout_key: Option<LayoutKey>,
    /// What the top bar showed when the layouts were worked out.
    clear: Vec<Rect>,
    window: Rect,
}

impl Painter {
    pub fn new(decorations: Arc<Decorations>) -> Self {
        let layers = decorations.layers.iter().map(LayerPainter::new).collect();
        Self { decorations, layers, layout_key: None, clear: Vec::new(), window: Rect::default() }
    }

    /// The first garland's layout (tests).
    pub fn garland(&self) -> Option<&GarlandLayout> {
        self.layers.iter().find_map(|l| match l {
            LayerPainter::Garland(p) => Some(&p.layout),
            _ => None,
        })
    }

    /// How many particles its layers have (tests).
    pub fn particles(&self) -> usize {
        self.layers
            .iter()
            .map(|l| match l {
                LayerPainter::Particles(p) => p.particles.len(),
                _ => 0,
            })
            .sum()
    }

    /// Every image it holds (tests).
    pub fn images(&self) -> impl Iterator<Item = &ImageData> {
        self.layers.iter().flat_map(LayerPainter::images)
    }

    fn sync_layout(&mut self, g: &Geometry<'_>) {
        let key = LayoutKey {
            window: g.window,
            scale: g.scale,
            top_inset: g.top_inset,
            top_bar: g.top_bar,
        };
        if self.layout_key == Some(key) && self.clear.as_slice() == g.keep_clear {
            return;
        }
        for (spec, painter) in self.decorations.layers.iter().zip(&mut self.layers) {
            match (spec, painter) {
                (LayerSpec::Garland(s), LayerPainter::Garland(p)) => {
                    p.layout = garland_layout(s, g)
                }
                (LayerSpec::Particles(s), LayerPainter::Particles(p)) => {
                    p.particles = particles_layout(s, g);
                }
                (LayerSpec::Corners(_), LayerPainter::Corners(p)) => p.places = corners_layout(g),
                (LayerSpec::Effect(e), LayerPainter::Effect(p)) => {
                    p.sync_layout(e, g.window, g.scale);
                }
                _ => {}
            }
        }
        self.window = g.window;
        self.layout_key = Some(key);
        self.clear.clear();
        self.clear.extend_from_slice(g.keep_clear);
    }

    fn sync_art(&mut self, scale: f32) {
        for (spec, painter) in self.decorations.layers.iter().zip(&mut self.layers) {
            match (spec, painter) {
                (LayerSpec::Garland(s), LayerPainter::Garland(p)) => p.sync_art(s, scale),
                (LayerSpec::Particles(s), LayerPainter::Particles(p)) => p.sync_art(s, scale),
                (LayerSpec::Corners(s), LayerPainter::Corners(p)) => p.sync_art(s, scale),
                (LayerSpec::Glow(s), LayerPainter::Glow(p)) => p.sync_art(s, scale),
                (LayerSpec::Effect(e), LayerPainter::Effect(p)) => p.sync_art(e, scale),
                _ => {}
            }
        }
    }
}

impl WindowDecorations for Painter {
    fn paint(&mut self, frame: &DecorationFrame<'_>, layer: &mut Layer) {
        let g = &frame.geometry;
        if g.window.is_empty() {
            return;
        }
        self.sync_layout(g);
        self.sync_art(g.scale);
        let window = self.window;
        for (spec, painter) in self.decorations.layers.iter().zip(&mut self.layers) {
            match (spec, painter) {
                (LayerSpec::Garland(s), LayerPainter::Garland(p)) => p.paint(s, frame, layer),
                (LayerSpec::Particles(s), LayerPainter::Particles(p)) => {
                    p.paint(s, window, covered_in(s.zone, frame), frame, layer);
                }
                (LayerSpec::Corners(s), LayerPainter::Corners(p)) => p.paint(s, frame, layer),
                (LayerSpec::Glow(s), LayerPainter::Glow(p)) => p.paint(s, window, g.scale, layer),
                (LayerSpec::Effect(e), LayerPainter::Effect(p)) => {
                    let f = FxFrame {
                        window,
                        scale: g.scale,
                        time: frame.time,
                        covered: covered_in(spec.zone(), frame),
                        icons: frame.icons,
                    };
                    p.paint(e, &f, layer);
                }
                _ => {}
            }
        }
    }

    fn moves(&self) -> bool {
        self.decorations.moves()
    }

    fn memory(&self) -> usize {
        let layouts: usize = self.layers.iter().map(LayerPainter::layout_memory).sum();
        std::mem::size_of::<Self>()
            + self.layers.capacity() * std::mem::size_of::<LayerPainter>()
            + self.clear.capacity() * std::mem::size_of::<Rect>()
            + layouts
            + self.images().map(ImageData::memory).sum::<usize>()
    }

    fn gpu_memory(&self) -> u64 {
        self.images().map(ImageData::gpu_memory).sum()
    }

    fn see_through(&self) -> Option<theming::decorations::SeeThrough> {
        self.decorations.see_through()
    }
}

#[cfg(test)]
#[path = "../tests/unit/paint.rs"]
mod tests;
