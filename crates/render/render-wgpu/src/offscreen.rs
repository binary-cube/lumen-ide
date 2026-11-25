//! A scene drawn on the GPU without a window, the way a window draws it (the same drawer,
//! pipelines, atlas and partial redraw), to measure what a frame costs and read back its
//! pixels: the snapshots' GPU picture and the renderer's before / after comparisons.
//! Nothing here runs in a window's life.

use std::time::{Duration, Instant};

use kernel::PhysicalSize;
use render_api::{FrameStats, GlyphRasterizer, RenderError, Scene};

use crate::{Drawer, GpuContext};

/// The format drawn into (as a window's surface usually is).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// What drawing a scene cost: the frame's statistics and the CPU's time to encode it (the
/// GPU waited for apart: its own time isn't counted).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Measured {
    pub stats: FrameStats,
    pub encode: Duration,
}

/// A GPU without a window and a drawer of its own.
pub struct Offscreen {
    ctx: std::sync::Arc<GpuContext>,
    drawer: Drawer,
    size: PhysicalSize,
}

impl std::fmt::Debug for Offscreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offscreen").field("size", &self.size).finish_non_exhaustive()
    }
}

impl Offscreen {
    /// A GPU found without a window (an error where there's none: a machine without one).
    pub fn new(size: PhysicalSize) -> Result<Self, RenderError> {
        let ctx = std::sync::Arc::new(headless()?);
        let drawer = Drawer::new(&ctx, FORMAT, false);
        Ok(Self { ctx, drawer, size })
    }

    /// `scene` drawn as a window would after the frame before it: only where it changed
    /// (`whole`: everything, as after a resize); waits for the GPU to finish.
    pub fn draw(
        &mut self,
        scene: &Scene,
        rasterizer: &mut dyn GlyphRasterizer,
        whole: bool,
    ) -> Measured {
        if whole {
            self.drawer.tracker.invalidate();
        }
        let started = Instant::now();
        self.drawer.compute_damage(&self.ctx, scene, self.size, rasterizer, true);
        if self.drawer.damage.is_empty() {
            return Measured { stats: FrameStats::default(), encode: started.elapsed() };
        }
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("offscreen") });
        let (draw_calls, drawn_pixels) =
            self.drawer.draw_kept(&self.ctx, &mut encoder, scene, self.size, rasterizer);
        self.ctx.queue.submit([encoder.finish()]);
        let encode = started.elapsed();
        let _ = self.ctx.device.poll(wgpu::PollType::wait_indefinitely());
        Measured {
            stats: FrameStats {
                quads: self.drawer.drawn.0,
                glyphs: self.drawer.drawn.1,
                draw_calls,
                drawn_pixels,
                cached_glyphs: self.ctx.state().atlas.len(),
                ..FrameStats::default()
            },
            encode,
        }
    }

    /// What copies frames made elsewhere into live images, as a window's renderer gives it.
    pub fn frame_importer(&self) -> std::sync::Arc<dyn render_api::FrameImporter> {
        std::sync::Arc::new(crate::import::Importer::new(std::sync::Arc::clone(&self.ctx)))
    }

    /// The importer itself, its steps called apart.
    #[cfg(test)]
    pub(crate) fn importer(&self) -> crate::import::Importer {
        crate::import::Importer::new(std::sync::Arc::clone(&self.ctx))
    }

    /// Every texture made as if last drawn `ago` earlier (what a while unshown frees).
    #[cfg(test)]
    pub(crate) fn age_textures(&self, ago: std::time::Duration) {
        let Some(then) = Instant::now().checked_sub(ago) else { return };
        for texture in self.ctx.state().textures.values_mut() {
            texture.last_used = then;
        }
    }

    /// Whether image `id`'s texture stays at the next frame ([`GpuImage::kept`]), `shown`
    /// or not (`None`: it has none).
    ///
    /// [`GpuImage::kept`]: crate::instances::GpuImage::kept
    #[cfg(test)]
    pub(crate) fn kept(&self, id: u64, shown: bool) -> Option<bool> {
        let state = self.ctx.state();
        state.textures.get(&id).map(|t| t.kept(shown, state.frame))
    }

    /// Whether `image` would be drawn now ([`GpuContext::upload_image`]).
    #[cfg(test)]
    pub(crate) fn drawable(&self, image: &render_api::ImageData) -> bool {
        let mut state = self.ctx.state();
        self.ctx.upload_image(&mut state, image)
    }

    /// When the first texture on screen nowhere goes if nothing is drawn.
    #[cfg(test)]
    pub(crate) fn unshown_release_at(&self) -> Option<Instant> {
        self.ctx.state().unshown_release_at()
    }

    /// How many frames' textures opened from another process are kept.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn sources(&self) -> usize {
        self.ctx.state().sources.len()
    }

    /// The last frame's pixels, RGBA8 row after row (`None`: none drawn yet).
    pub fn pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (buffer, (w, h, row)) = self.drawer.copy_kept(&self.ctx)?;
        let bgra = self.drawer.bgra();
        let rgba = crate::with_mapped(&self.ctx, &buffer, |data| {
            let mut out = Vec::with_capacity(w as usize * h as usize * 4);
            for y in 0..h as usize {
                for px in data[y * row..][..w as usize * 4].as_chunks::<4>().0 {
                    match bgra {
                        true => out.extend_from_slice(&[px[2], px[1], px[0], px[3]]),
                        false => out.extend_from_slice(px),
                    }
                }
            }
            out
        })?;
        Some((w, h, rgba))
    }
}

/// The GPU, without a window (the integrated one where there are two, as a window's).
pub(crate) fn headless() -> Result<GpuContext, RenderError> {
    pollster::block_on(async {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        GpuContext::new(instance, adapter).await
    })
}

#[cfg(test)]
#[path = "../tests/unit/offscreen.rs"]
mod tests;
