//! Software (CPU) render driver, for headless tests and fallback.
//!
//! Draws into an in-memory buffer. Quads use the same SDF formula as the GPU
//! shader (`render-wgpu/src/shaders/quad.wgsl`), so it serves as the reference
//! implementation in tests.
//!
//! The buffer is kept between frames: a frame draws again only where the scene changed
//! ([`DamageTracker`]), the rest stays as the previous frame left it.
//!
//! Backdrop blurs are made as the GPU makes them ([`BlurPlan`]: reduced, a separable
//! gaussian, scaled back up), on the CPU, and kept while what's under them stays the same.
//!
//! The scene's emphasis ([`Emphasis`]) mutes the pixels drawn by the layers under it once
//! they're drawn, before the layers above it.

use kernel::cache::GenerationalCache;
use std::ops::Range;

use render_api::{
    BlurLayer, BlurPlan, Damage, DamageTracker, Emphasis, FrameStats, GlyphBox, GlyphFormat,
    GlyphKey, GlyphRasterizer, ImageInstance, Layer, PhysicalSize, PixelRect, Quad,
    RasterizedGlyph, Rect, RenderError, Renderer, Rgba, Scene, blur_layers,
};
use tiny_skia::{Pixmap, PremultipliedColorU8};

#[derive(Debug)]
pub struct SoftwareRenderer {
    pixmap: Pixmap,
    /// Rasterized glyphs, bounded (the least used go first).
    glyph_cache: GenerationalCache<GlyphKey, Option<RasterizedGlyph>>,
    /// The frame in `pixmap`, to tell what the next one changes.
    tracker: DamageTracker,
    /// The last frame's damage (kept for its memory).
    damage: Damage,
    /// Draws only the damaged areas (`EDITOR_FULL_REDRAW` turns it off).
    partial: bool,
    /// The backdrop blurs kept (one per layer that blurs, in order).
    blurs: Vec<Blurred>,
    /// This frame's layers that blur (kept for its memory).
    wanted: Vec<BlurLayer>,
    /// Blurs made so far (a test sees them reused).
    blurs_made: u64,
    /// A capture is being made small (one at a time).
    capturing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// A backdrop blur made: what's under `layer` within `bounds`, reduced and blurred
/// (premultiplied RGBA, 0..1).
#[derive(Debug)]
struct Blurred {
    of: BlurLayer,
    plan: BlurPlan,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}

/// Glyphs a generation of the cache holds (two generations at most).
const GLYPHS: usize = 2048;

impl SoftwareRenderer {
    /// `None` if the size is zero or too large.
    pub fn new(size: PhysicalSize) -> Option<Self> {
        Some(Self {
            pixmap: Pixmap::new(size.width, size.height)?,
            glyph_cache: GenerationalCache::new(GLYPHS),
            tracker: DamageTracker::new(),
            damage: Damage::default(),
            partial: render_api::partial_redraw(),
            blurs: Vec::new(),
            wanted: Vec::new(),
            blurs_made: 0,
            capturing: std::sync::Arc::default(),
        })
    }

    /// Backdrop blurs made since it was created.
    pub fn blurs_made(&self) -> u64 {
        self.blurs_made
    }

    /// The memory its kept blurs take (bytes).
    pub fn blur_memory(&self) -> usize {
        self.blurs.iter().map(|b| b.pixels.len() * std::mem::size_of::<[f32; 4]>()).sum()
    }

    /// Every frame drawn whole (`false`) or only where it changed (the default).
    pub fn set_partial(&mut self, partial: bool) {
        self.partial = partial;
        self.tracker.invalidate();
    }

    pub fn size(&self) -> PhysicalSize {
        PhysicalSize::new(self.pixmap.width(), self.pixmap.height())
    }

    /// Color of pixel `(x, y)` as non-premultiplied RGBA8.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        let p = self.pixmap.pixel(x, y)?.demultiply();
        Some([p.red(), p.green(), p.blue(), p.alpha()])
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, RenderError> {
        self.pixmap.encode_png().map_err(|e| RenderError::Surface(e.to_string()))
    }

    fn surface_rect(&self) -> Rect {
        Rect::from_size(self.size().to_size())
    }

    fn draw_quad(&mut self, quad: &Quad, clip: &Rect) {
        let Some(area) = quad.bounds.intersect(clip) else { return };
        let (x0, y0, x1, y1) = pixel_span(&area);
        let width = self.pixmap.width();
        let pixels = self.pixmap.pixels_mut();
        for y in y0..y1 {
            for x in x0..x1 {
                let (fill, border) = quad_coverage(quad, x as f32 + 0.5, y as f32 + 0.5);
                let idx = (y * width + x) as usize;
                blend(&mut pixels[idx], quad.color, fill);
                blend(&mut pixels[idx], quad.border_color, border);
            }
        }
    }

    /// The window's corners cut: what's outside the rounded window is erased (transparent).
    /// Same formula as `fs_window_corner` in `shaders.wgsl`.
    /// Only within `area` (the pixels drawn again this frame: erasing is not idempotent).
    fn cut_corners(&mut self, radius: f32, area: PixelRect) {
        let window = Quad::filled(self.surface_rect(), Rgba::TRANSPARENT).rounded(radius);
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let r = (radius.ceil() as u32 + 1).min(width).min(height);
        let (x0, y0, x1, y1) = (area.x0 as u32, area.y0 as u32, area.x1 as u32, area.y1 as u32);
        let pixels = self.pixmap.pixels_mut();
        let inside = |v: &u32, lo: u32, hi: u32| (lo..hi).contains(v);
        for y in (0..r).chain(height - r..height).filter(|y| inside(y, y0, y1)) {
            for x in (0..r).chain(width - r..width).filter(|x| inside(x, x0, x1)) {
                let (inside, _) = quad_coverage(&window, x as f32 + 0.5, y as f32 + 0.5);
                let p = &mut pixels[(y * width + x) as usize];
                let keep = |c: u8| (f32::from(c) * inside).round() as u8;
                let (r, g, b, a) =
                    (keep(p.red()), keep(p.green()), keep(p.blue()), keep(p.alpha()));
                if let Some(c) = PremultipliedColorU8::from_rgba(r, g, b, a) {
                    *p = c;
                }
            }
        }
    }

    /// The image scaled into `bounds` (bilinear sampling).
    fn draw_image(&mut self, instance: &ImageInstance, clip: &Rect) {
        let image = &instance.image;
        let b = instance.bounds;
        let Some(area) = b.intersect(clip) else { return };
        if image.width == 0 || image.height == 0 || b.size.width <= 0.0 || b.size.height <= 0.0 {
            return;
        }
        let (x0, y0, x1, y1) = pixel_span(&area);
        let width = self.pixmap.width();
        let (iw, ih) = (image.width as f32, image.height as f32);
        // The software renderer keeps no textures: it reads the pixels every frame.
        let pixels = self.pixmap.pixels_mut();
        // A mask: one byte of coverage, in its color.
        let bpp = image.bytes_per_pixel() as u32;
        let tint = image.tint;
        image.with_pixels(|source| {
            if source.len() < (image.width * image.height * bpp) as usize {
                return;
            }
            let texel = |x: u32, y: u32| {
                let i = ((y.min(image.height - 1) * image.width + x.min(image.width - 1)) * bpp)
                    as usize;
                match tint {
                    Some(c) => {
                        let a = f32::from(source[i]) * c.a;
                        [c.r * 255.0, c.g * 255.0, c.b * 255.0, a]
                    }
                    None => {
                        let p = &source[i..i + 4];
                        [f32::from(p[0]), f32::from(p[1]), f32::from(p[2]), f32::from(p[3])]
                    }
                }
            };
            for y in y0..y1 {
                for x in x0..x1 {
                    let u = ((x as f32 + 0.5 - b.min_x()) / b.size.width * iw - 0.5).max(0.0);
                    let v = ((y as f32 + 0.5 - b.min_y()) / b.size.height * ih - 0.5).max(0.0);
                    let (ux, vy) = (u.floor() as u32, v.floor() as u32);
                    let (fx, fy) = (u.fract(), v.fract());
                    let (a, bb, c, d) = (
                        texel(ux, vy),
                        texel(ux + 1, vy),
                        texel(ux, vy + 1),
                        texel(ux + 1, vy + 1),
                    );
                    let mut out = [0.0f32; 4];
                    for k in 0..4 {
                        let top = a[k] * (1.0 - fx) + bb[k] * fx;
                        let bottom = c[k] * (1.0 - fx) + d[k] * fx;
                        out[k] = (top * (1.0 - fy) + bottom * fy) / 255.0;
                    }
                    let color = Rgba::new(out[0], out[1], out[2], out[3]);
                    blend(&mut pixels[(y * width + x) as usize], color, instance.opacity);
                }
            }
        });
    }
}

/// Draws `glyph` at `(ox, oy)` into `pixmap` (the cache keeps it: no copy per frame).
fn draw_glyph(
    pixmap: &mut Pixmap,
    glyph: &RasterizedGlyph,
    (ox, oy): (f32, f32),
    color: Rgba,
    clip: &Rect,
) {
    let left = ox.round() as i64 + i64::from(glyph.left);
    let top = oy.round() as i64 - i64::from(glyph.top);
    let (cx0, cy0, cx1, cy1) = pixel_span(clip);
    let width = pixmap.width();
    let pixels = pixmap.pixels_mut();
    for gy in 0..glyph.height {
        let y = top + i64::from(gy);
        if y < i64::from(cy0) || y >= i64::from(cy1) {
            continue;
        }
        for gx in 0..glyph.width {
            let x = left + i64::from(gx);
            if x < i64::from(cx0) || x >= i64::from(cx1) {
                continue;
            }
            let idx = (y as u32 * width + x as u32) as usize;
            let src = (gy * glyph.width + gx) as usize;
            match glyph.format {
                GlyphFormat::Mask => {
                    blend(&mut pixels[idx], color, f32::from(glyph.data[src]) / 255.0);
                }
                GlyphFormat::Color => {
                    let c = &glyph.data[src * 4..src * 4 + 4];
                    let rgba = Rgba::new(
                        f32::from(c[0]) / 255.0,
                        f32::from(c[1]) / 255.0,
                        f32::from(c[2]) / 255.0,
                        f32::from(c[3]) / 255.0,
                    );
                    // Its own colors, faded by the instance color's alpha.
                    blend(&mut pixels[idx], rgba, color.a);
                }
            }
        }
    }
}

impl Renderer for SoftwareRenderer {
    fn description(&self) -> String {
        "software (CPU)".into()
    }

    fn resize(&mut self, size: PhysicalSize) {
        if let Some(pixmap) = Pixmap::new(size.width, size.height) {
            self.pixmap = pixmap;
            self.tracker.invalidate();
        }
    }

    fn purge_caches(&mut self) {
        self.glyph_cache.clear();
        render_api::glyph_keys_reset();
    }

    fn backdrop_blur(&self) -> bool {
        true
    }

    fn capture(
        &mut self,
        max_width: u32,
        done: Box<dyn FnOnce(render_api::Captured) + Send>,
    ) -> bool {
        // One at a time: the copy is as large as the window.
        if self.capturing.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return false;
        }
        // The frame copied now (the next one draws over it), made small in the background.
        // Its pixels are premultiplied, which an opaque window's are as they are.
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let data = self.pixmap.data().to_vec();
        let capturing = std::sync::Arc::clone(&self.capturing);
        kernel::work::run("frame-capture", move || {
            let layout = (width, height, width as usize * 4);
            let picture = render_api::shrink(&data, layout, false, max_width);
            drop(data);
            capturing.store(false, std::sync::atomic::Ordering::Release);
            done(picture);
        });
        true
    }

    fn render(
        &mut self,
        scene: &Scene,
        glyphs: &mut dyn GlyphRasterizer,
    ) -> Result<FrameStats, RenderError> {
        let size = self.size();
        let surface = PixelRect::of_size(size);
        let mut damage = std::mem::take(&mut self.damage);
        if self.partial {
            let cache = &mut self.glyph_cache;
            let mut glyph_box = |key: GlyphKey| {
                if cache.get(&key).is_none() {
                    cache.insert(key, glyphs.rasterize(key));
                }
                cache.get(&key).map(|g| GlyphBox::of(g.as_ref()))
            };
            self.tracker.damage(scene, size, &mut glyph_box, &mut damage);
        } else {
            damage.set_full();
        }
        // The blurs that no longer hold (something under them changed, or they're new) are
        // made again, from a whole frame.
        let mut wanted = std::mem::take(&mut self.wanted);
        blur_layers(scene, surface, &mut wanted);
        let first = damage.first_layer();
        let holds = |w: &BlurLayer| {
            self.blurs.iter().any(|b| b.of == *w) && first.is_none_or(|f| f >= w.layer)
        };
        let stale: Vec<usize> = wanted.iter().filter(|w| !holds(w)).map(|w| w.layer).collect();
        self.blurs.retain(|b| wanted.contains(&b.of) && !stale.contains(&b.of.layer));
        if !stale.is_empty() {
            damage.set_full();
        }
        let mut drawn_pixels = 0;
        let all = 0..scene.layers().len();
        if damage.is_full() {
            // Up to each blur made again, then the blur, then on.
            let mut from = 0;
            for &k in &stale {
                self.draw_span(scene, surface, from..k, glyphs);
                if let Some(of) = wanted.iter().find(|w| w.layer == k) {
                    let made = self.make_blur(*of);
                    self.blurs.push(made);
                    self.blurs_made += 1;
                }
                from = k;
            }
            self.draw_span(scene, surface, from..all.end, glyphs);
            drawn_pixels = surface.area() as u64;
        } else {
            for &area in damage.rects() {
                self.draw_span(scene, area, all.clone(), glyphs);
                drawn_pixels += area.area() as u64;
            }
        }
        self.wanted = wanted;
        self.damage = damage;
        if self.partial {
            self.tracker.commit(scene, size);
        }
        Ok(FrameStats {
            quads: scene.quad_count(),
            glyphs: scene.glyph_count(),
            draw_calls: 0,
            drawn_pixels,
            cached_glyphs: self.glyph_cache.len(),
            blurs: stale.len(),
            skipped: false,
            retry: false,
        })
    }
}

impl SoftwareRenderer {
    /// [`Self::draw_layers`], the pixels muted by the scene's emphasis once the layers under
    /// it are drawn (when it falls within `layers`).
    fn draw_span(
        &mut self,
        scene: &Scene,
        area: PixelRect,
        layers: Range<usize>,
        glyphs: &mut dyn GlyphRasterizer,
    ) {
        match scene.emphasis().copied() {
            Some(e) if layers.start < e.below() && e.below() <= layers.end => {
                self.draw_layers(scene, area, layers.start..e.below(), glyphs);
                self.mute(&e, area);
                if e.below() < layers.end {
                    self.draw_layers(scene, area, e.below()..layers.end, glyphs);
                }
            }
            _ => self.draw_layers(scene, area, layers, glyphs),
        }
    }

    /// The pixels within `area` muted by `emphasis` (as `common.wgsl` mutes them on the GPU).
    fn mute(&mut self, emphasis: &Emphasis, area: PixelRect) {
        let Some(area) = area.intersect(&PixelRect::of_size(self.size())) else { return };
        let width = self.pixmap.width() as usize;
        let pixels = self.pixmap.pixels_mut();
        // Outside what it keeps, all is muted alike: no mask to work out.
        let reach = emphasis.reach().map(|r| PixelRect::covering(&r));
        for y in area.y0..area.y1 {
            let row = y as usize * width;
            for x in area.x0..area.x1 {
                let p = &mut pixels[row + x as usize];
                let rgb = [p.red(), p.green(), p.blue()].map(|c| f32::from(c) / 255.0);
                let near = reach.is_some_and(|r| x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1);
                let out = if near {
                    emphasis.apply(rgb, x as f32 + 0.5, y as f32 + 0.5)
                } else {
                    render_api::mute(rgb, emphasis.gray, emphasis.dim)
                };
                if out == rgb {
                    continue;
                }
                let a = p.alpha();
                let [r, g, b] = out.map(|c| ((c * 255.0).round().clamp(0.0, 255.0) as u8).min(a));
                if let Some(c) = PremultipliedColorU8::from_rgba(r, g, b, a) {
                    *p = c;
                }
            }
        }
    }

    /// The scene's pixels within `area` (whole pixels inside the surface), drawn from the
    /// background up; each pixel comes out as in a full frame. Only `layers`: from the first
    /// the background is laid, at the last the window's corners are cut.
    fn draw_layers(
        &mut self,
        scene: &Scene,
        area: PixelRect,
        layers: Range<usize>,
        glyphs: &mut dyn GlyphRasterizer,
    ) {
        let last = layers.end == scene.layers().len();
        if layers.start == 0 {
            self.fill_background(scene, area);
        }
        let surface = self.surface_rect();
        if let Some(bounds) = area.to_rect().intersect(&surface) {
            for (i, layer) in scene.layers().iter().enumerate().take(layers.end).skip(layers.start)
            {
                self.draw_layer(i, layer, &bounds, glyphs);
            }
        }
        if last && scene.window_corners > 0.0 {
            self.cut_corners(scene.window_corners, area);
        }
    }

    /// The background within `area`.
    fn fill_background(&mut self, scene: &Scene, area: PixelRect) {
        let [r, g, b, a] = scene.clear_color.to_rgba8();
        let background = tiny_skia::Color::from_rgba8(r, g, b, a);
        if area == PixelRect::of_size(self.size()) {
            self.pixmap.fill(background);
        } else {
            let fill = background.premultiply().to_color_u8();
            let width = self.pixmap.width() as usize;
            let pixels = self.pixmap.pixels_mut();
            for y in area.y0..area.y1 {
                let row = y as usize * width;
                pixels[row + area.x0 as usize..row + area.x1 as usize].fill(fill);
            }
        }
    }

    /// Layer `index` within `bounds`: its blurs, quads, images, glyphs.
    fn draw_layer(
        &mut self,
        index: usize,
        layer: &Layer,
        bounds: &Rect,
        glyphs: &mut dyn GlyphRasterizer,
    ) {
        let clip = match layer.clip {
            Some(c) => c.intersect(bounds),
            None => Some(*bounds),
        };
        let Some(clip) = clip else { return };
        if !layer.blurs.is_empty()
            && let Some(i) = self.blurs.iter().position(|b| b.of.layer == index)
        {
            for blur in &layer.blurs {
                if let Some(area) = blur.rect.intersect(&clip) {
                    self.draw_blur(i, &area, blur);
                }
            }
        }
        for quad in &layer.quads {
            self.draw_quad(quad, &clip);
        }
        for image in &layer.images {
            self.draw_image(image, &clip);
        }
        for instance in &layer.glyphs {
            if self.glyph_cache.get(&instance.key).is_none() {
                self.glyph_cache.insert(instance.key, glyphs.rasterize(instance.key));
            }
            if let Some(Some(raster)) = self.glyph_cache.get(&instance.key) {
                let origin = (instance.origin.x, instance.origin.y);
                draw_glyph(&mut self.pixmap, raster, origin, instance.color, &clip);
            }
        }
    }

    /// What's under `of` (already drawn in the buffer, within its bounds), reduced by the
    /// plan's factor (each texel the average of its pixels) and blurred both ways.
    fn make_blur(&self, of: BlurLayer) -> Blurred {
        let plan = BlurPlan::for_radius(of.radius);
        let (w, h) = plan.reduced(of.bounds);
        let d = plan.factor as i32;
        let b = of.bounds;
        let width = self.pixmap.width() as usize;
        let source = self.pixmap.pixels();
        let mut reduced = vec![[0.0f32; 4]; (w * h) as usize];
        let n = (d * d) as f32;
        for j in 0..h as i32 {
            for i in 0..w as i32 {
                let mut sum = [0.0f32; 4];
                for y in 0..d {
                    let y = (b.y0 + j * d + y).min(b.y1 - 1) as usize;
                    for x in 0..d {
                        let x = (b.x0 + i * d + x).min(b.x1 - 1) as usize;
                        let p = source[y * width + x];
                        sum[0] += f32::from(p.red());
                        sum[1] += f32::from(p.green());
                        sum[2] += f32::from(p.blue());
                        sum[3] += f32::from(p.alpha());
                    }
                }
                reduced[(j as u32 * w + i as u32) as usize] = sum.map(|v| v / n / 255.0);
            }
        }
        let weights = plan.weights();
        let blurred = gaussian(&gaussian(&reduced, w, h, (1, 0), &weights), w, h, (0, 1), &weights);
        Blurred { of, plan, width: w, height: h, pixels: blurred }
    }

    /// Blur `index` drawn into `area` (scaled up, bilinear), at `opacity`.
    fn draw_blur(&mut self, index: usize, area: &Rect, of: &render_api::BackdropBlur) {
        let blur = &self.blurs[index];
        let (x0, y0, x1, y1) = pixel_span(area);
        let d = blur.plan.factor as f32;
        let (bx, by) = (blur.of.bounds.x0 as f32, blur.of.bounds.y0 as f32);
        let (w, h) = (blur.width as i32, blur.height as i32);
        let texel =
            |x: i32, y: i32| blur.pixels[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
        let width = self.pixmap.width();
        let pixels = self.pixmap.pixels_mut();
        for y in y0..y1 {
            let v = ((y as f32 + 0.5 - by) / d - 0.5).max(0.0);
            let (ty, fy) = (v.floor() as i32, v.fract());
            for x in x0..x1 {
                let u = ((x as f32 + 0.5 - bx) / d - 0.5).max(0.0);
                let (tx, fx) = (u.floor() as i32, u.fract());
                let (a, b, c, e) =
                    (texel(tx, ty), texel(tx + 1, ty), texel(tx, ty + 1), texel(tx + 1, ty + 1));
                // Rounded corners: the pixel's share inside them.
                let inside = if of.corner > 0.0 {
                    render_api::rounded_coverage(
                        &of.rect,
                        of.corner,
                        x as f32 + 0.5,
                        y as f32 + 0.5,
                    )
                } else {
                    1.0
                };
                let opacity = of.opacity * inside;
                let mut out = [0.0f32; 4];
                for k in 0..4 {
                    let top = a[k] * (1.0 - fx) + b[k] * fx;
                    let bottom = c[k] * (1.0 - fx) + e[k] * fx;
                    out[k] = (top * (1.0 - fy) + bottom * fy) * opacity;
                }
                blend_premultiplied(&mut pixels[(y * width + x) as usize], out);
            }
        }
    }
}

/// A separable gaussian pass over a `w` × `h` image, along `dir`, edges clamped.
fn gaussian(src: &[[f32; 4]], w: u32, h: u32, dir: (i32, i32), weights: &[f32]) -> Vec<[f32; 4]> {
    let (w, h) = (w as i32, h as i32);
    let taps = weights.len() as i32 - 1;
    let mut out = vec![[0.0f32; 4]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f32; 4];
            for k in -taps..=taps {
                let sx = (x + k * dir.0).clamp(0, w - 1);
                let sy = (y + k * dir.1).clamp(0, h - 1);
                let p = src[(sy * w + sx) as usize];
                let wk = weights[k.unsigned_abs() as usize];
                for c in 0..4 {
                    sum[c] += p[c] * wk;
                }
            }
            out[(y * w + x) as usize] = sum;
        }
    }
    out
}

/// Composites a premultiplied color (0..1) over the pixel (source-over).
fn blend_premultiplied(dst: &mut PremultipliedColorU8, src: [f32; 4]) {
    let a = src[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let inv = 1.0 - a;
    let mix = |s: f32, d: u8| (s * 255.0 + f32::from(d) * inv).round().clamp(0.0, 255.0) as u8;
    let out_a = mix(a, dst.alpha());
    let (r, g, b) = (mix(src[0], dst.red()), mix(src[1], dst.green()), mix(src[2], dst.blue()));
    if let Some(p) =
        PremultipliedColorU8::from_rgba(r.min(out_a), g.min(out_a), b.min(out_a), out_a)
    {
        *dst = p;
    }
}

/// Whole pixels covered (at least partially) by a rect already clipped to the surface.
fn pixel_span(r: &Rect) -> (u32, u32, u32, u32) {
    let c = |v: f32| v.max(0.0) as u32;
    (c(r.min_x().floor()), c(r.min_y().floor()), c(r.max_x().ceil()), c(r.max_y().ceil()))
}

/// Coverage (fill, border) of point `(px, py)` for a quad with rounded corners.
/// Same formula as in `quad.wgsl`.
fn quad_coverage(quad: &Quad, px: f32, py: f32) -> (f32, f32) {
    let b = &quad.bounds;
    let half_w = b.size.width / 2.0;
    let half_h = b.size.height / 2.0;
    let radius = quad.corner_radius.min(half_w).min(half_h).max(0.0);
    let dx = (px - (b.min_x() + half_w)).abs() - (half_w - radius);
    let dy = (py - (b.min_y() + half_h)).abs() - (half_h - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let dist = outside + dx.max(dy).min(0.0) - radius;

    let shape = (0.5 - dist).clamp(0.0, 1.0);
    if quad.border_width <= 0.0 {
        return (shape, 0.0);
    }
    let inner = (0.5 - (dist + quad.border_width)).clamp(0.0, 1.0);
    (inner, shape - inner)
}

/// Composites `color` (non-premultiplied sRGB) over the pixel with the given coverage
/// (source-over).
fn blend(dst: &mut PremultipliedColorU8, color: Rgba, coverage: f32) {
    let a = (color.a * coverage).clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let inv = 1.0 - a;
    let mix =
        |src: f32, d: u8| (src * a * 255.0 + f32::from(d) * inv).round().clamp(0.0, 255.0) as u8;
    let out_a = mix(1.0, dst.alpha());
    let (r, g, b) = (mix(color.r, dst.red()), mix(color.g, dst.green()), mix(color.b, dst.blue()));
    if let Some(p) =
        PremultipliedColorU8::from_rgba(r.min(out_a), g.min(out_a), b.min(out_a), out_a)
    {
        *dst = p;
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
