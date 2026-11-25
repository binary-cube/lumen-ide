//! What changed between two frames: the areas of the surface a renderer must draw again.
//!
//! A frame's pixels are a function of the scene: the clear color, the window's corners and,
//! in order, each layer's quads, images and glyphs (each clipped by its layer). A pixel that
//! no changed primitive covers — in the previous frame or in this one — comes out the same,
//! so a renderer that kept the previous frame's pixels only draws again where something
//! changed: the [`Damage`]. [`DamageTracker`] keeps a compact record of the last frame
//! drawn (no pixels, no image data) and compares the next scene with it, primitive by
//! primitive; anything it can't vouch for (another size, another background, another
//! emphasis, glyph keys reissued, a glyph whose box it doesn't know) is a full redraw.

use std::sync::atomic::{AtomicU64, Ordering};

use kernel::{PhysicalSize, Point, Rect, Rgba};

use crate::{
    BackdropBlur, Emphasis, GlyphInstance, GlyphKey, ImageInstance, Quad, RasterizedGlyph, Scene,
};

/// At most this many separate damaged rectangles; beyond, the two closest are merged.
const MAX_RECTS: usize = 8;
/// Past this many changes in a frame (an animation, never typing), they're no longer kept
/// apart: one rectangle holds them all (merging each would cost more than the frame's
/// drawing — measured: 1.7 ms of 15 000 moving dots, against 0.2 ms to draw them).
const COARSE_AFTER: u32 = 256;

/// Damage covering more than this share of the surface is drawn as a full frame (cheaper
/// than many scissored passes over most of it).
const FULL_SHARE: f64 = 0.5;

/// Bumped whenever glyph keys may be reissued for other glyphs (the text system and the
/// renderers' glyph caches cleared together): a key equal to the last frame's no longer
/// proves the same pixels.
static GLYPH_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The glyph keys were reset (see [`GLYPH_EPOCH`]): every tracker redraws everything next.
pub fn glyph_keys_reset() {
    GLYPH_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Whether renderers draw only what changed: yes, unless `EDITOR_FULL_REDRAW` is set (every
/// frame whole, as before damage tracking: to compare, or if a driver shows stale areas).
pub fn partial_redraw() -> bool {
    static PARTIAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PARTIAL.get_or_init(|| std::env::var_os("EDITOR_FULL_REDRAW").is_none())
}

fn glyph_epoch() -> u64 {
    GLYPH_EPOCH.load(Ordering::Relaxed)
}

/// A rectangle of whole pixels: `x0..x1` × `y0..y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl PixelRect {
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// The whole surface.
    pub fn of_size(size: PhysicalSize) -> Self {
        let side = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        Self::new(0, 0, side(size.width), side(size.height))
    }

    /// Every pixel `r` touches, even partly.
    pub fn covering(r: &Rect) -> Self {
        let c = |v: f32| v.clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        Self::new(
            c(r.min_x().floor()),
            c(r.min_y().floor()),
            c(r.max_x().ceil()),
            c(r.max_y().ceil()),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn width(&self) -> u32 {
        (self.x1 - self.x0).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (self.y1 - self.y0).max(0) as u32
    }

    pub fn area(&self) -> i64 {
        i64::from(self.width()) * i64::from(self.height())
    }

    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let r = Self::new(
            self.x0.max(other.x0),
            self.y0.max(other.y0),
            self.x1.min(other.x1),
            self.y1.min(other.y1),
        );
        (!r.is_empty()).then_some(r)
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Self) -> Self {
        Self::new(
            self.x0.min(other.x0),
            self.y0.min(other.y0),
            self.x1.max(other.x1),
            self.y1.max(other.y1),
        )
    }

    pub fn contains(&self, other: &Self) -> bool {
        self.x0 <= other.x0 && self.y0 <= other.y0 && self.x1 >= other.x1 && self.y1 >= other.y1
    }

    /// Overlapping or sharing an edge (their union wastes nothing between them).
    fn touches(&self, other: &Self) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    pub fn to_rect(&self) -> Rect {
        Rect::new(self.x0 as f32, self.y0 as f32, self.width() as f32, self.height() as f32)
    }
}

/// Where a glyph's bitmap lies relative to its pen position (as [`RasterizedGlyph`] places
/// it). A glyph with no pixels (a space) has an empty box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlyphBox {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl GlyphBox {
    pub const EMPTY: Self = Self { left: 0, top: 0, width: 0, height: 0 };

    pub fn of(glyph: Option<&RasterizedGlyph>) -> Self {
        glyph.map_or(Self::EMPTY, |g| Self {
            left: g.left,
            top: g.top,
            width: g.width,
            height: g.height,
        })
    }

    /// The pixels it covers drawn at `origin` (rounded to a whole pixel, as the renderers
    /// draw it).
    pub fn at(&self, origin: Point) -> PixelRect {
        let x0 = origin.x.round() as i32 + self.left;
        let y0 = origin.y.round() as i32 - self.top;
        let side = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        PixelRect::new(
            x0,
            y0,
            x0.saturating_add(side(self.width)),
            y0.saturating_add(side(self.height)),
        )
    }
}

/// The areas of a frame to draw again: none, a few rectangles (merged, within the surface)
/// or everything.
#[derive(Debug, Clone, Default)]
pub struct Damage {
    full: bool,
    rects: Vec<PixelRect>,
    /// The lowest layer whose pixels changed (`Some(0)`: the whole frame is new).
    first_layer: Option<usize>,
    /// Rectangles added (a layer that adds one changed something).
    changes: u32,
    /// The damaged area past which the frame is whole ([`FULL_SHARE`] of the surface; 0: no
    /// limit known): reached, nothing more is compared.
    full_beyond: i64,
    /// Past [`COARSE_AFTER`] changes: the one rectangle holding them all.
    coarse: Option<PixelRect>,
    /// Every live image of the scene, with the frame each had when the damage was found
    /// (its id, its generation; noted on every path, a full frame's too): what the frame
    /// drawn is recorded with ([`DamageTracker::commit_drawn`]), so a frame that came after
    /// is drawn by the next one (and what blurs it made again then).
    seen: Vec<(u64, u64)>,
}

impl Damage {
    pub fn full() -> Self {
        Self {
            full: true,
            rects: Vec::new(),
            first_layer: Some(0),
            changes: 0,
            full_beyond: 0,
            coarse: None,
            seen: Vec::new(),
        }
    }

    /// The lowest layer whose primitives changed from the last frame (`Some(0)` when the
    /// whole frame is new, `None` when nothing changed): a backdrop blur over layers below
    /// it still holds.
    pub fn first_layer(&self) -> Option<usize> {
        self.first_layer
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Nothing changed: the frame kept is already this one.
    pub fn is_empty(&self) -> bool {
        !self.full && self.rects.is_empty()
    }

    /// The damaged rectangles (none when full: everything is).
    pub fn rects(&self) -> &[PixelRect] {
        &self.rects
    }

    /// The rectangle holding all the damaged ones.
    pub fn bounds(&self) -> Option<PixelRect> {
        self.rects.iter().copied().reduce(|a, b| a.union(&b))
    }

    pub fn clear(&mut self) {
        self.full = false;
        self.rects.clear();
        self.first_layer = None;
        self.changes = 0;
        self.coarse = None;
        self.seen.clear();
    }

    /// The frame live image `id` had when this damage was found (`None`: not compared).
    fn seen_generation(&self, id: u64) -> Option<u64> {
        let at = self.seen.binary_search_by_key(&id, |(seen, _)| *seen).ok()?;
        self.seen.get(at).map(|(_, generation)| *generation)
    }

    /// Everything of `scene` drawn again, as new: the damage of a frame not compared with
    /// the last one (none kept, or assumed whole), its live images' frames noted as they are
    /// now.
    pub fn set_full_of(&mut self, scene: &Scene) {
        self.clear();
        self.set_full();
        self.note_live(scene);
    }

    /// Notes the frame each live image of `scene` has now (what it was compared with), once
    /// per image: by id, for [`Self::seen_generation`]'s search (an image drawn twice keeps
    /// the frame read first). Called on a damage just cleared.
    fn note_live(&mut self, scene: &Scene) {
        let live = scene.layers().iter().flat_map(|l| &l.images).filter(|i| i.image.is_live());
        self.seen.extend(live.map(|i| (i.image.id, i.image.generation())));
        // Stable: of an image's entries, the first read stays first.
        self.seen.sort_by_key(|(id, _)| *id);
        self.seen.dedup_by_key(|(id, _)| *id);
    }

    /// Everything is drawn again, as new (what was under a blur too). The live images'
    /// frames noted stay: they're the ones drawn.
    pub fn set_full(&mut self) {
        self.full = true;
        self.rects.clear();
        self.coarse = None;
        self.first_layer = Some(0);
    }

    /// Everything is drawn again, the layers that didn't change as they were (cheaper than
    /// many areas).
    fn widen_to_full(&mut self) {
        self.full = true;
        self.rects.clear();
        self.coarse = None;
    }

    /// Adds a damaged rectangle, merged with those it touches when their union wastes little
    /// (two rectangles meeting at a corner — a frame's top and its side — stay apart: their
    /// union would be the whole area between them); past [`MAX_RECTS`], the two whose union
    /// wastes least are merged.
    pub fn add(&mut self, rect: PixelRect) {
        if self.full || rect.is_empty() {
            return;
        }
        self.changes += 1;
        // Many changes: one rectangle holding them all, whole once it's past the share.
        if self.changes > COARSE_AFTER {
            let all = self.rects.drain(..).fold(self.coarse.unwrap_or(rect), |a, r| a.union(&r));
            let all = all.union(&rect);
            self.coarse = Some(all);
            if self.full_beyond > 0 && all.area() > self.full_beyond {
                self.widen_to_full();
            }
            return;
        }
        let mut rect = rect;
        if self.rects.iter().any(|r| r.contains(&rect)) {
            return;
        }
        while let Some(i) =
            self.rects.iter().position(|r| r.touches(&rect) && cheap_union(r, &rect))
        {
            rect = rect.union(&self.rects.swap_remove(i));
        }
        self.rects.push(rect);
        if self.rects.len() > MAX_RECTS {
            self.merge_closest();
        }
        // Past the share that makes it whole (as [`Self::finish`] would): whole now, the rest
        // of the frame not compared (an animation over the window changes thousands).
        if self.full_beyond > 0
            && self.rects.iter().map(PixelRect::area).sum::<i64>() > self.full_beyond
        {
            self.widen_to_full();
        }
    }

    fn merge_closest(&mut self) {
        let mut best = (0, 1, i64::MAX);
        for i in 0..self.rects.len() {
            for j in i + 1..self.rects.len() {
                let (a, b) = (self.rects[i], self.rects[j]);
                let waste = a.union(&b).area() - a.area() - b.area();
                if waste < best.2 {
                    best = (i, j, waste);
                }
            }
        }
        let (i, j, _) = best;
        let b = self.rects.swap_remove(j);
        let merged = self.rects.swap_remove(i).union(&b);
        // The merged one may now touch others.
        self.add(merged);
    }

    /// Within `surface`; a full frame if most of it is damaged anyway.
    fn finish(&mut self, surface: PixelRect) {
        if self.full {
            return;
        }
        if let Some(all) = self.coarse.take() {
            self.rects.push(all);
        }
        self.rects.retain_mut(|r| match r.intersect(&surface) {
            Some(inside) => {
                *r = inside;
                true
            }
            None => false,
        });
        let damaged: i64 = self.rects.iter().map(PixelRect::area).sum();
        if damaged as f64 > surface.area() as f64 * FULL_SHARE {
            self.widen_to_full();
        }
    }
}

/// Whether two touching rectangles are better drawn as one: their union covers little more
/// than they do (at most a quarter more).
fn cheap_union(a: &PixelRect, b: &PixelRect) -> bool {
    let overlap = a.intersect(b).map_or(0, |r| r.area());
    let parts = a.area() + b.area() - overlap;
    a.union(b).area() * 4 <= parts * 5
}

/// An image as the damage sees it: which one, where, how opaque, and which of its frames
/// (its pixels never change for an id, but a live one's frame does).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageRecord {
    id: u64,
    generation: u64,
    bounds: Rect,
    opacity: f32,
    corners: crate::ImageCorners,
}

impl ImageRecord {
    /// The same image drawn the same, only another of its frames in it.
    fn only_frame_differs(&self, other: &Self) -> bool {
        self.id == other.id
            && self.generation != other.generation
            && self.bounds == other.bounds
            && self.opacity == other.opacity
            && self.corners == other.corners
    }
}

impl From<&ImageInstance> for ImageRecord {
    fn from(i: &ImageInstance) -> Self {
        Self {
            id: i.image.id,
            generation: i.image.generation(),
            bounds: i.bounds,
            opacity: i.opacity,
            corners: i.corners,
        }
    }
}

/// A layer of the last frame drawn.
#[derive(Debug, Clone, Default)]
struct LayerRecord {
    clip: Option<Rect>,
    blurs: Vec<BackdropBlur>,
    quads: Vec<Quad>,
    images: Vec<ImageRecord>,
    glyphs: Vec<GlyphInstance>,
}

/// The last frame drawn, to tell the next one's [`Damage`].
///
/// The renderer asks for the damage before drawing ([`damage`](Self::damage)) and, once the
/// frame's pixels are where it keeps them, records it ([`commit`](Self::commit)). Until the
/// first commit (and after [`invalidate`](Self::invalidate)) every frame is full.
#[derive(Debug, Default)]
pub struct DamageTracker {
    valid: bool,
    size: PhysicalSize,
    clear_color: Rgba,
    window_corners: f32,
    /// A change of the emphasis changes every pixel it mutes: a full frame.
    emphasis: Option<Emphasis>,
    epoch: u64,
    layers: Vec<LayerRecord>,
    active: usize,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pixels kept no longer match the last frame recorded: the next one is full.
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Forgets the last frame and gives its memory back (the window hidden).
    pub fn release(&mut self) {
        *self = Self::default();
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// What `scene` changes from the last frame recorded, on a surface of `size`.
    /// `glyph_box` tells where a glyph's pixels lie (`None`: unknown, which makes the frame
    /// full); it's asked only for glyphs that changed.
    pub fn damage(
        &self,
        scene: &Scene,
        size: PhysicalSize,
        glyph_box: &mut dyn FnMut(GlyphKey) -> Option<GlyphBox>,
        out: &mut Damage,
    ) {
        out.clear();
        // Before anything is compared: the frames drawn are at least these, whichever way
        // this returns.
        out.note_live(scene);
        if !self.valid
            || self.size != size
            || self.epoch != glyph_epoch()
            || self.clear_color != scene.clear_color
            || self.window_corners != scene.window_corners
            || self.emphasis.as_ref() != scene.emphasis()
        {
            out.set_full();
            return;
        }
        let surface = PixelRect::of_size(size);
        out.full_beyond = (surface.area() as f64 * FULL_SHARE) as i64;
        let old = &self.layers[..self.active];
        let new = scene.layers();
        let mut first = None;
        for i in 0..old.len().max(new.len()) {
            let before = out.changes;
            match (old.get(i), new.get(i)) {
                (Some(o), Some(n)) if o.clip == n.clip => {
                    let Some(clip) = layer_clip(o.clip, surface) else { continue };
                    diff_layer(o, n, clip, glyph_box, out);
                }
                (o, n) => {
                    if let Some(o) = o
                        && let Some(clip) = layer_clip(o.clip, surface)
                    {
                        add_blurs(&o.blurs, clip, out);
                        add_all(&o.quads, &o.images, &o.glyphs, clip, glyph_box, out);
                    }
                    if let Some(n) = n
                        && let Some(clip) = layer_clip(n.clip, surface)
                    {
                        add_blurs(&n.blurs, clip, out);
                        let images = n.images.iter().map(ImageRecord::from);
                        add_all(&n.quads, images, &n.glyphs, clip, glyph_box, out);
                    }
                }
            }
            if out.changes != before || out.is_full() {
                first.get_or_insert(i);
            }
            if out.is_full() || out.rects.iter().any(|r| r.contains(&surface)) {
                out.widen_to_full();
                out.first_layer = first;
                return;
            }
        }
        out.first_layer = first;
        out.finish(surface);
    }

    /// `scene` was drawn on a surface of `size`: the next frame is compared with it.
    pub fn commit(&mut self, scene: &Scene, size: PhysicalSize) {
        self.commit_drawn(scene, size, &Damage::full());
    }

    /// [`Self::commit`] of a frame drawn where `damage` said: a live image is recorded with
    /// the frame it had when `damage` was found (one put in since — outside the areas drawn,
    /// or under a blur not made again — is drawn by the next frame).
    pub fn commit_drawn(&mut self, scene: &Scene, size: PhysicalSize, damage: &Damage) {
        self.valid = true;
        self.size = size;
        self.epoch = glyph_epoch();
        self.clear_color = scene.clear_color;
        self.window_corners = scene.window_corners;
        self.emphasis = scene.emphasis().copied();
        let layers = scene.layers();
        if self.layers.len() < layers.len() {
            self.layers.resize_with(layers.len(), LayerRecord::default);
        }
        for (record, layer) in self.layers.iter_mut().zip(layers) {
            record.clip = layer.clip;
            record.blurs.clone_from(&layer.blurs);
            record.quads.clone_from(&layer.quads);
            record.glyphs.clone_from(&layer.glyphs);
            record.images.clear();
            record.images.extend(layer.images.iter().map(|i| {
                let mut image = ImageRecord::from(i);
                // A whole frame too: a blur over a page whose layer didn't change isn't made
                // again, so the frame put in since must still count as new.
                if i.image.is_live()
                    && let Some(generation) = damage.seen_generation(image.id)
                {
                    image.generation = generation;
                }
                image
            }));
        }
        self.active = layers.len();
    }
}

/// The pixels a layer can touch (`None`: none on the surface).
fn layer_clip(clip: Option<Rect>, surface: PixelRect) -> Option<PixelRect> {
    match clip {
        Some(c) => PixelRect::covering(&c).intersect(&surface),
        None => Some(surface),
    }
}

/// Glyph damage at `origin`; `false` if the glyph's box is unknown (the frame becomes full).
fn add_glyph(
    g: &GlyphInstance,
    clip: PixelRect,
    glyph_box: &mut dyn FnMut(GlyphKey) -> Option<GlyphBox>,
    out: &mut Damage,
) -> bool {
    match glyph_box(g.key) {
        Some(b) => {
            if let Some(r) = b.at(g.origin).intersect(&clip) {
                out.add(r);
            }
            true
        }
        None => {
            out.set_full();
            false
        }
    }
}

/// A layer's blurs (appearing, going or changed): all they cover.
fn add_blurs(blurs: &[BackdropBlur], clip: PixelRect, out: &mut Damage) {
    for b in blurs {
        add_rect(&b.rect, clip, out);
    }
}

fn add_rect(bounds: &Rect, clip: PixelRect, out: &mut Damage) {
    if let Some(r) = PixelRect::covering(bounds).intersect(&clip) {
        out.add(r);
    }
}

/// Where `change` (in an image's frames' pixels) lies once the image is drawn in `bounds`,
/// with a texel more around it: the filtering mixes a pixel with the texels beside it.
fn frame_area(bounds: &Rect, change: crate::live::FrameChange) -> Rect {
    let (width, height) = change.size;
    let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else { return *bounds };
    if w == 0 || h == 0 {
        return *bounds;
    }
    let area = change.area;
    let x0 = area.x0.saturating_sub(1).clamp(0, w);
    let y0 = area.y0.saturating_sub(1).clamp(0, h);
    let x1 = area.x1.saturating_add(1).clamp(0, w);
    let y1 = area.y1.saturating_add(1).clamp(0, h);
    let sx = bounds.size.width / width as f32;
    let sy = bounds.size.height / height as f32;
    Rect::new(
        bounds.origin.x + x0 as f32 * sx,
        bounds.origin.y + y0 as f32 * sy,
        (x1 - x0) as f32 * sx,
        (y1 - y0) as f32 * sy,
    )
}

/// Everything a layer draws (a layer appearing, going or clipped elsewhere).
fn add_all(
    quads: &[Quad],
    images: impl IntoIterator<Item = impl std::borrow::Borrow<ImageRecord>>,
    glyphs: &[GlyphInstance],
    clip: PixelRect,
    glyph_box: &mut dyn FnMut(GlyphKey) -> Option<GlyphBox>,
    out: &mut Damage,
) {
    for q in quads {
        add_rect(&q.bounds, clip, out);
    }
    for i in images {
        add_rect(&i.borrow().bounds, clip, out);
    }
    for g in glyphs {
        if !add_glyph(g, clip, glyph_box, out) {
            return;
        }
    }
}

/// The primitives that differ, index by index (a primitive inserted shifts the rest: they
/// all count as changed, which is more than needed but never less).
fn diff_layer(
    old: &LayerRecord,
    new: &crate::Layer,
    clip: PixelRect,
    glyph_box: &mut dyn FnMut(GlyphKey) -> Option<GlyphBox>,
    out: &mut Damage,
) {
    if old.blurs != new.blurs {
        add_blurs(&old.blurs, clip, out);
        add_blurs(&new.blurs, clip, out);
    }
    if old.quads != new.quads {
        for k in 0..old.quads.len().max(new.quads.len()) {
            let (o, n) = (old.quads.get(k), new.quads.get(k));
            if o == n {
                continue;
            }
            if out.is_full() {
                return;
            }
            for q in [o, n].into_iter().flatten() {
                add_rect(&q.bounds, clip, out);
            }
        }
    }
    for k in 0..old.images.len().max(new.images.len()) {
        let instance = new.images.get(k);
        let n = instance.map(|i| {
            let mut n = ImageRecord::from(i);
            // As noted before comparing: what the frame drawn is recorded with.
            if i.image.is_live()
                && let Some(generation) = out.seen_generation(n.id)
            {
                n.generation = generation;
            }
            n
        });
        let o = old.images.get(k).copied();
        if o == n {
            continue;
        }
        if out.is_full() {
            return;
        }
        // Another frame of a live image, drawn the same: only where its frames changed it.
        if let (Some(o), Some(n), Some(i)) = (o, n, instance)
            && o.only_frame_differs(&n)
            && let Some(change) = i.image.changed_between(o.generation, n.generation)
        {
            add_rect(&frame_area(&n.bounds, change), clip, out);
            continue;
        }
        for i in [o, n].into_iter().flatten() {
            add_rect(&i.bounds, clip, out);
        }
    }
    if old.glyphs != new.glyphs {
        for k in 0..old.glyphs.len().max(new.glyphs.len()) {
            let (o, n) = (old.glyphs.get(k), new.glyphs.get(k));
            if o == n {
                continue;
            }
            if out.is_full() {
                return;
            }
            for g in [o, n].into_iter().flatten() {
                if !add_glyph(g, clip, glyph_box, out) {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/damage.rs"]
mod tests;
