//! Declarative description of a frame.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kernel::{Point, Rect, Rgba};

use crate::{BackdropBlur, Emphasis, GlyphKey};

/// A complete frame. Coordinates are in **physical pixels**, with the origin at the
/// top-left corner of the surface.
///
/// The scene is designed for reuse: [`Scene::reset`] keeps the allocated memory,
/// so in steady state building a frame doesn't allocate.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub clear_color: Rgba,
    /// The window's corners rounded by this radius (physical pixels; 0: square): what's
    /// outside them is left transparent, where the surface shows what's behind the window
    /// (Windows: the app rounds its own window, whatever the system does).
    pub window_corners: f32,
    /// The layers drawn before it muted, but in its kept areas ([`Scene::emphasize`]).
    emphasis: Option<Emphasis>,
    layers: Vec<Layer>,
    /// How many layers in `layers` are used in the current frame.
    active: usize,
}

impl Scene {
    pub fn new(clear_color: Rgba) -> Self {
        Self { clear_color, window_corners: 0.0, emphasis: None, layers: Vec::new(), active: 0 }
    }

    /// Clears the scene for a new frame, keeping the allocated capacity.
    pub fn reset(&mut self, clear_color: Rgba) {
        self.clear_color = clear_color;
        self.window_corners = 0.0;
        self.emphasis = None;
        self.active = 0;
    }

    /// The layers pushed so far muted by `emphasis` (but in its kept areas); those pushed
    /// after it (overlays) are drawn as they are. Once a frame: a second call replaces it.
    /// One that changes nothing (no gray, no dim, no layer under it) is left out.
    pub fn emphasize(&mut self, mut emphasis: Emphasis) {
        emphasis.below = self.active;
        self.emphasis = (emphasis.below > 0 && emphasis.is_visible()).then_some(emphasis);
    }

    /// The frame's emphasis, if any ([`Self::emphasize`]).
    pub fn emphasis(&self) -> Option<&Emphasis> {
        self.emphasis.as_ref()
    }

    /// Adds a new layer on top of the existing ones. `clip` bounds everything drawn in it.
    pub fn push_layer(&mut self, clip: Option<Rect>) -> &mut Layer {
        if self.active == self.layers.len() {
            self.layers.push(Layer::default());
        }
        let layer = &mut self.layers[self.active];
        self.active += 1;
        layer.clip = clip;
        layer.blurs.clear();
        layer.quads.clear();
        layer.images.clear();
        layer.glyphs.clear();
        layer
    }

    /// A layer of the current frame, to draw again in place (a frame where only it
    /// changes: the rest of the scene kept as it was).
    pub fn layer_mut(&mut self, index: usize) -> Option<&mut Layer> {
        self.layers[..self.active].get_mut(index)
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers[..self.active]
    }

    pub fn quad_count(&self) -> usize {
        self.layers.iter().map(|l| l.quads.len()).sum()
    }

    pub fn glyph_count(&self) -> usize {
        self.layers.iter().map(|l| l.glyphs.len()).sum()
    }
}

/// A scene layer. Draw order: layers in the order they were added; within
/// a layer, the backdrop blurs first (what the layers before it drew, blurred), then quads
/// (backgrounds), then images, then glyphs.
#[derive(Debug, Clone, Default)]
pub struct Layer {
    pub clip: Option<Rect>,
    pub blurs: Vec<BackdropBlur>,
    pub quads: Vec<Quad>,
    pub images: Vec<ImageInstance>,
    pub glyphs: Vec<GlyphInstance>,
}

/// An image's pixels (RGBA8, non-premultiplied — or a [`mask`](Self::mask): one byte of
/// coverage per pixel, drawn in one color), shared: the scene doesn't copy them, and the
/// renderer uploads them to the GPU only once (the key is `id`).
///
/// A [`gpu_only`](Self::gpu_only) image gives up its pixels once they are on the GPU (only
/// the texture holds the image); if the renderer later frees the texture (unused for a
/// while), the image is [lost](Self::is_lost) and its owner decodes it again.
#[derive(Clone)]
pub struct ImageData {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    /// A mask's color (its pixels are coverage, one byte each); `None`: RGBA8 pixels.
    pub tint: Option<crate::Rgba>,
    shared: Arc<Pixels>,
}

struct Pixels {
    /// Its owner gave it up: the renderer frees its texture (a budget kept by the owner).
    released: AtomicBool,
    /// Taken (dropped) after the upload of a `gpu_only` image.
    data: Mutex<Option<Vec<u8>>>,
    gpu_only: bool,
    /// A texture with these pixels exists on the GPU.
    on_gpu: AtomicBool,
}

impl ImageData {
    /// A new image, with a process-unique `id`; its pixels stay in memory.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self::make(width, height, pixels, false)
    }

    /// A new image whose pixels are dropped from memory once uploaded to the GPU.
    pub fn gpu_only(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self::make(width, height, pixels, true)
    }

    /// A mask: `width * height` bytes of coverage, drawn in `color` (a quarter of an RGBA
    /// image's memory, for what's one color: a diagram's shapes of that color). Its pixels
    /// are dropped once uploaded.
    pub fn mask(width: u32, height: u32, coverage: Vec<u8>, color: crate::Rgba) -> Self {
        let mut image = Self::make(width, height, coverage, true);
        image.tint = Some(color);
        image
    }

    /// The bytes one of its pixels takes.
    pub fn bytes_per_pixel(&self) -> u64 {
        if self.tint.is_some() { 1 } else { 4 }
    }

    /// Its owner gives it up: the renderer frees its texture at its next frame (it's then
    /// [lost](Self::is_lost)).
    pub fn release(&self) {
        self.shared.released.store(true, Ordering::Release);
    }

    /// Its owner gave it up ([`Self::release`]).
    pub fn released(&self) -> bool {
        self.shared.released.load(Ordering::Acquire)
    }

    fn make(width: u32, height: u32, pixels: Vec<u8>, gpu_only: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            tint: None,
            shared: Arc::new(Pixels {
                released: AtomicBool::new(false),
                data: Mutex::new(Some(pixels)),
                gpu_only,
                on_gpu: AtomicBool::new(false),
            }),
        }
    }

    /// Runs `f` on the pixels, if they are still in memory.
    pub fn with_pixels<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let data = self.shared.data.lock().ok()?;
        data.as_deref().map(f)
    }

    /// The renderer uploaded the pixels: a `gpu_only` image drops them.
    pub fn uploaded(&self) {
        self.shared.on_gpu.store(true, Ordering::Release);
        if self.shared.gpu_only
            && let Ok(mut data) = self.shared.data.lock()
            && let Some(pixels) = data.take()
        {
            let large = pixels.len() >= 1 << 20;
            drop(pixels);
            // A large buffer's pages would otherwise stay with the allocator.
            if large {
                kernel::memory::release_free();
            }
        }
    }

    /// The renderer freed the texture.
    pub fn evicted(&self) {
        self.shared.on_gpu.store(false, Ordering::Release);
    }

    /// Neither in memory nor on the GPU any more: it must be decoded again.
    pub fn is_lost(&self) -> bool {
        !self.shared.on_gpu.load(Ordering::Acquire)
            && self.shared.data.lock().map_or(true, |d| d.is_none())
    }

    /// The bytes held in memory (0 once given up to the GPU).
    pub fn memory(&self) -> usize {
        self.shared.data.lock().ok().and_then(|d| d.as_ref().map(Vec::len)).unwrap_or(0)
    }

    /// The bytes its texture takes on the GPU (RGBA8, or R8 for a mask), while it's there.
    pub fn gpu_memory(&self) -> u64 {
        if self.shared.on_gpu.load(Ordering::Acquire) {
            u64::from(self.width) * u64::from(self.height) * self.bytes_per_pixel()
        } else {
            0
        }
    }
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageData#{} {}x{}", self.id, self.width, self.height)
    }
}

/// An image drawn (scaled) into `bounds`.
#[derive(Debug, Clone)]
pub struct ImageInstance {
    pub image: ImageData,
    pub bounds: Rect,
    pub opacity: f32,
}

impl Layer {
    /// What the layers before this one drew, blurred by `radius` (physical pixels), shown
    /// in `rect` at `opacity` — under this layer's own primitives. A renderer that can't
    /// blur leaves it out ([`Renderer::backdrop_blur`](crate::Renderer::backdrop_blur)).
    pub fn backdrop_blur(&mut self, rect: Rect, radius: f32, opacity: f32) -> &mut Self {
        if opacity > 0.0 && radius >= 1.0 {
            self.blurs.push(BackdropBlur { rect, radius, opacity, corner: 0.0 });
        }
        self
    }

    pub fn quad(&mut self, quad: Quad) -> &mut Self {
        self.quads.push(quad);
        self
    }

    /// [`Self::backdrop_blur`] with its corners rounded by `corner` (a pane of glass).
    pub fn backdrop_blur_rounded(
        &mut self,
        rect: Rect,
        radius: f32,
        opacity: f32,
        corner: f32,
    ) -> &mut Self {
        if opacity > 0.0 && radius >= 1.0 {
            self.blurs.push(BackdropBlur { rect, radius, opacity, corner });
        }
        self
    }

    pub fn image(&mut self, image: &ImageData, bounds: Rect) -> &mut Self {
        self.images.push(ImageInstance { image: image.clone(), bounds, opacity: 1.0 });
        self
    }

    pub fn glyph(&mut self, glyph: GlyphInstance) -> &mut Self {
        self.glyphs.push(glyph);
        self
    }
}

/// Filled rectangle, optionally with rounded corners and an inner border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub bounds: Rect,
    pub color: Rgba,
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: Rgba,
}

impl Quad {
    pub fn filled(bounds: Rect, color: Rgba) -> Self {
        Self {
            bounds,
            color,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: Rgba::TRANSPARENT,
        }
    }

    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn bordered(mut self, width: f32, color: Rgba) -> Self {
        self.border_width = width;
        self.border_color = color;
        self
    }
}

/// A glyph to draw. `origin` is the position on the baseline (pen position), aligned
/// to a whole pixel by the text system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphInstance {
    pub key: GlyphKey,
    pub origin: Point,
    pub color: Rgba,
}

#[cfg(test)]
#[path = "../tests/unit/scene.rs"]
mod tests;
