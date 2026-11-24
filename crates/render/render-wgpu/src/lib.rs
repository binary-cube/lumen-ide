//! Default render driver, based on wgpu (Metal, DX12, Vulkan, GL).
//!
//! The scene's rectangles and glyphs are one kind of instance, each carrying its layer's
//! clip and emphasis: consecutive layers are one instanced draw, broken only by a layer's
//! images (a texture each) or backdrop blurs. Glyphs are kept in a GPU atlas.
//!
//! **One GPU per process:** all windows share [`GpuContext`] (the instance,
//! adapter, device, queue, glyph atlas, image textures); each window
//! has only its own surface, instance buffers and pipelines. The context lives as
//! long as a window exists (after the last one, GPU memory is freed).

mod activity;
mod atlas;
mod blur;
mod context;
mod drawer;
mod instances;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(test, feature = "offscreen"))]
mod offscreen;
mod pipelines;
mod prepared;
mod renderer;
mod surface;

#[cfg(any(test, feature = "offscreen"))]
pub use offscreen::{Measured, Offscreen};

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use bytemuck::{Pod, Zeroable};
use render_api::{
    BlurLayer, Damage, DamageTracker, Emphasis, FrameStats, GlyphBox, GlyphRasterizer, GpuApi,
    MAX_KEEPS, PhysicalSize, PixelRect, Rect, RenderError, RenderOptions, RenderTarget, Renderer,
    RendererFactory, Rgba, Scene, blur_layers,
};

use crate::atlas::{AtlasFull, GlyphAtlas};
use crate::blur::{BlurGpu, BlurPipelines, GpuBlur};
use crate::instances::{
    Batch, Globals, GpuImage, IMAGE_IDLE, IMAGE_TTL, ImageGpu, InstanceBuffer, LayerPlan,
    PRIM_COLOR, PRIM_MASK, PrimGpu, QuadGpu, Step, color_array, rect_array,
};
use crate::pipelines::{Pipelines, atlas_bind_group, create_pipeline};
use crate::surface::{Acquired, resizes, surface_bytes};

/// The glyph atlas starts small (1 MB) and doubles when full, up to the largest size;
/// only then is it cleared.
/// A capture waiting for a frame.
struct PendingCapture {
    max_width: u32,
    done: Box<dyn FnOnce(render_api::Captured) + Send>,
}

impl std::fmt::Debug for PendingCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingCapture").field("max_width", &self.max_width).finish_non_exhaustive()
    }
}

/// How long after the last frame on screen the surface's spare image goes.
const SPARE_AFTER: std::time::Duration = std::time::Duration::from_millis(1500);

const ATLAS_START: u32 = 1024;
const ATLAS_MAX: u32 = 4096;

/// Factory for [`WgpuRenderer`].
#[derive(Debug, Default)]
pub struct WgpuRendererFactory;

impl RendererFactory for WgpuRendererFactory {
    fn name(&self) -> &'static str {
        "wgpu"
    }

    fn create(
        &self,
        target: Arc<dyn RenderTarget>,
        size: PhysicalSize,
        options: &RenderOptions,
    ) -> Result<Box<dyn Renderer>, RenderError> {
        Ok(Box::new(pollster::block_on(WgpuRenderer::new(target, size, options))?))
    }

    fn prepare(&self, options: &RenderOptions) {
        prepared::prepare(options);
    }
}

/// The thread [`RendererFactory::prepare`] makes the GPU on (it ends once done).
pub const PREPARE_THREAD: &str = prepared::THREAD;

/// The GPU context shared by windows (held weakly: goes away with the last window).
static CONTEXT: Mutex<Option<Weak<GpuContext>>> = Mutex::new(None);

/// What windows share: the GPU and the surface-independent resources.
struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    adapter_info: wgpu::AdapterInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    globals_layout: wgpu::BindGroupLayout,
    atlas_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    image_sampler: wgpu::Sampler,
    shader: wgpu::ShaderModule,
    image_shader: wgpu::ShaderModule,
    /// The windows' pipelines, by the surface's format: made once, however many windows.
    pipelines: Mutex<HashMap<wgpu::TextureFormat, Arc<Pipelines>>>,
    /// Shared mutable state (the atlas, the images); used on the UI thread.
    state: Mutex<SharedState>,
}

impl GpuContext {
    /// The pipelines for `format`, made by the first window with it.
    fn pipelines(&self, format: wgpu::TextureFormat) -> Arc<Pipelines> {
        let mut made = self.pipelines.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(made.entry(format).or_insert_with(|| Arc::new(Pipelines::new(self, format))))
    }
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("adapter", &self.adapter_info.name)
            .finish_non_exhaustive()
    }
}

struct SharedState {
    atlas: GlyphAtlas,
    atlas_bind_group: wgpu::BindGroup,
    textures: HashMap<u64, GpuImage>,
    /// Frames drawn (by all windows), for freeing unused images.
    frame: u64,
    /// The images in each window's last scene (by its drawer's id): on screen, never freed.
    shown: HashMap<u64, Vec<u64>>,
}

impl SharedState {
    /// Whether image `id` is in a window's last scene.
    fn is_shown(&self, id: u64) -> bool {
        self.shown.values().any(|ids| ids.contains(&id))
    }

    /// The images on screen nowhere, unused long enough (as a frame frees them), freed:
    /// for when no frame is drawn (the app in the background).
    fn release_unshown(&mut self) {
        let frame = self.frame;
        let shown: std::collections::HashSet<u64> =
            self.shown.values().flatten().copied().collect();
        self.textures.retain(|id, t| {
            let keep = shown.contains(id)
                || (frame.saturating_sub(t.last_frame) < IMAGE_TTL
                    && t.last_used.elapsed() < IMAGE_IDLE
                    && !t.image.released());
            if !keep {
                t.image.evicted();
            }
            keep
        });
    }

    /// When the first image on screen nowhere would be freed ([`Self::release_unshown`]).
    fn unshown_release_at(&self) -> Option<std::time::Instant> {
        self.textures
            .iter()
            .filter(|(id, _)| !self.is_shown(**id))
            .map(|(_, t)| t.last_used + IMAGE_IDLE)
            .min()
    }
}

/// wgpu renderer bound to a window.
#[derive(Debug)]
pub struct WgpuRenderer {
    ctx: Arc<GpuContext>,
    target: Arc<dyn RenderTarget>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    drawer: Drawer,
    /// Frames are drawn into a kept texture, only where they changed, and copied to the
    /// surface's image: the surface accepts copies and `EDITOR_FULL_REDRAW` isn't set.
    partial: bool,
    /// A capture is being read back (one at a time).
    capturing: Arc<std::sync::atomic::AtomicBool>,
    /// A capture asked while no frame was kept: taken from the next one drawn.
    pending_capture: Option<PendingCapture>,
    /// The surface's spare image was given back: its size is set again before the next frame.
    #[cfg(target_os = "macos")]
    spare_released: bool,
    /// Hidden a while: the surface was made one pixel large (its image on screen given back);
    /// set to the window's size again before the next frame.
    hidden_released: bool,
    /// When a frame last reached the screen, while the spare image it took is still held.
    spare_held_since: Option<std::time::Instant>,
    /// How busy the window is drawing: a quiet one keeps no frame.
    activity: activity::Activity,
    /// macOS: frames are presented in sync with the window's transaction until then (set
    /// on each resize; afterwards presenting no longer waits on the main thread).
    #[cfg(target_os = "macos")]
    transactional_until: Option<std::time::Instant>,
    /// Elsewhere: frames go straight to the surface until then (set on each resize), so a
    /// drag doesn't make a window-sized kept frame at every step.
    #[cfg(not(target_os = "macos"))]
    resized_until: Option<std::time::Instant>,
}

/// What draws a window's scenes: its pipelines (they depend on the surface's format),
/// instance buffers, and the last frame kept for partial redraws.
#[derive(Debug)]
struct Drawer {
    /// Its images on screen are known by it ([`SharedState::shown`]).
    id: u64,
    globals: wgpu::Buffer,
    /// The viewport `globals` holds (written again only when it changes).
    viewport: Option<PhysicalSize>,
    globals_bind_group: wgpu::BindGroup,
    /// The globals the layers under the scene's emphasis are drawn with (made at the first
    /// emphasis), and what they hold (written again only when it changes).
    muted: Option<MutedGlobals>,
    /// This frame's layers under the emphasis (`0..muted_below`; 0: none).
    muted_below: usize,
    /// Staging memory for the instance uploads, reused from frame to frame.
    belt: wgpu::util::StagingBelt,
    pipelines: Arc<Pipelines>,
    format: wgpu::TextureFormat,
    /// The surface shows what's behind the window where it's transparent (its corners can
    /// be cut): DX12 through DirectComposition.
    transparent: bool,

    /// The background and the window's corners (drawn apart from the scene).
    quad_buffer: InstanceBuffer,
    prim_buffer: InstanceBuffer,
    image_buffer: InstanceBuffer,
    images: Vec<ImageGpu>,
    /// The image `id` of each instance in `images`.
    image_ids: Vec<u64>,
    quads: Vec<QuadGpu>,
    prims: Vec<PrimGpu>,
    /// This frame's rectangles and glyphs among `prims` (its statistics).
    drawn: (usize, usize),
    batches: Vec<Batch>,
    /// Each of the frame's areas' batches, in order (among `batches`).
    area_batches: Vec<Range<usize>>,
    /// The layers' instances with the pixels each covers, before they're split by area.
    layer_plans: Vec<LayerPlan>,
    layer_quads: Vec<(PrimGpu, PixelRect)>,
    layer_images: Vec<(ImageGpu, u64, PixelRect)>,
    layer_glyphs: Vec<(PrimGpu, PixelRect)>,

    /// The last frame's pixels (one surface-sized texture), while partial redraws are on.
    kept: Option<KeptFrame>,
    /// The size of the frame on screen, if the last one reached it.
    shown: Option<PhysicalSize>,
    /// The frame in `kept` (or, while it's stale, the last one drawn straight on screen), to
    /// tell what the next one changes.
    tracker: DamageTracker,
    /// Frames changing as a whole (an animation over the window) are drawn straight on screen,
    /// not into `kept` and copied: `kept` is then behind (drawn whole when a partial frame
    /// comes), and goes after a while of them ([`DIRECT_RELEASE`]).
    kept_stale: bool,
    /// Whole frames drawn straight on screen in a row.
    direct_frames: u32,
    /// Frames found (or taken) whole in a row: past [`WHOLE_STREAK`], the next are taken
    /// whole without comparing, checked again every [`WHOLE_CHECK`]th.
    whole_streak: u32,
    /// This frame's damage (kept for its memory).
    damage: Damage,

    /// The backdrop blur's pipelines, made at the first blur.
    blur_pipelines: Option<Arc<BlurPipelines>>,
    /// The blurs kept, one per layer that blurs (their textures go when it stops).
    blurs: Vec<GpuBlur>,
    /// This frame's layers that blur, and those whose blur is made again (their layers).
    wanted: Vec<BlurLayer>,
    stale: Vec<usize>,
    blur_instances: Vec<BlurGpu>,
    blur_buffer: Option<InstanceBuffer>,
}

/// The globals with the scene's emphasis, for the layers it mutes.
#[derive(Debug)]
struct MutedGlobals {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// What `buffer` holds.
    written: (PhysicalSize, Emphasis),
}

/// The last frame drawn, offscreen: partial frames draw into it, then it's copied to the
/// surface's image (whose contents aren't kept between frames on most platforms).
#[derive(Debug)]
struct KeptFrame {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: PhysicalSize,
}

/// The extra instances [`Drawer::upload`] adds after the scene's.
/// What a frame's passes draw and into what: the view, the scene and its size, what was
/// uploaded for it, the shared state and the areas drawn (`None`: all of it).
#[derive(Clone, Copy)]
struct Target<'a> {
    view: &'a wgpu::TextureView,
    scene: &'a Scene,
    size: PhysicalSize,
    extras: Extras,
    state: &'a SharedState,
    areas: Option<&'a [PixelRect]>,
}

#[derive(Debug, Clone, Copy)]
struct Extras {
    /// The four window-corner quads (erased last), when the corners are cut.
    corners: Option<u32>,
    /// The background quad, drawn over each damaged area of a partial frame.
    fill: u32,
}

impl WgpuRenderer {
    pub async fn new(
        target: Arc<dyn RenderTarget>,
        size: PhysicalSize,
        options: &RenderOptions,
    ) -> Result<Self, RenderError> {
        let (ctx, surface, prepared) = GpuContext::for_target(&target, options).await?;
        let mut config = Self::surface_config(&surface, &ctx.adapter, size, options)?;
        // Partial frames are copied into the surface's images: it must accept copies.
        let copies =
            surface.get_capabilities(&ctx.adapter).usages.contains(wgpu::TextureUsages::COPY_DST);
        let partial = copies && render_api::partial_redraw();
        if partial {
            config.usage |= wgpu::TextureUsages::COPY_DST;
        } else {
            tracing::info!(copies, "frames are drawn whole (no partial redraw)");
        }
        surface.configure(&ctx.device, &config);
        #[cfg(target_os = "macos")]
        macos::configure_layer(&surface);
        let transparent = matches!(
            config.alpha_mode,
            wgpu::CompositeAlphaMode::PreMultiplied | wgpu::CompositeAlphaMode::PostMultiplied
        );
        // The pipelines made ahead, if they were made for this surface.
        let drawer = match prepared {
            Some(drawer) if drawer.format == config.format && drawer.transparent == transparent => {
                drawer
            }
            _ => Drawer::new(&ctx, config.format, transparent),
        };
        Ok(Self {
            drawer,
            partial,
            capturing: Arc::default(),
            pending_capture: None,
            #[cfg(target_os = "macos")]
            spare_released: false,
            hidden_released: false,
            spare_held_since: None,
            activity: activity::Activity::default(),
            target,
            surface,
            config,
            #[cfg(target_os = "macos")]
            transactional_until: None,
            #[cfg(not(target_os = "macos"))]
            resized_until: None,
            ctx,
        })
    }

    /// Tries graphics APIs in order of preference. For `Auto`: Metal (macOS),
    /// DX12 then Vulkan (Windows: DX12 is native and lighter; Vulkan brings the loader and
    /// layers injected by drivers / overlays), Vulkan (Linux), then GL.
    async fn select_adapter(
        target: &Arc<dyn RenderTarget>,
        options: &RenderOptions,
    ) -> Result<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter), RenderError> {
        let attempts: &[wgpu::Backends] = match options.preferred_api {
            GpuApi::Auto if cfg!(windows) => {
                &[wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL]
            }
            GpuApi::Auto => &[wgpu::Backends::PRIMARY, wgpu::Backends::GL],
            GpuApi::Metal => &[wgpu::Backends::METAL],
            GpuApi::Dx12 => &[wgpu::Backends::DX12],
            GpuApi::Vulkan => &[wgpu::Backends::VULKAN],
            GpuApi::Gl => &[wgpu::Backends::GL],
        };

        let mut last_error = String::from("no backend tried");
        for &backends in attempts {
            let mut desc =
                wgpu::InstanceDescriptor::new_with_display_handle(Box::new(Arc::clone(target)));
            desc.backends = backends;
            // A transparent surface: DX12 presents through DirectComposition (a swap chain
            // for a visual, with its alpha), not straight to the window.
            if options.transparent {
                desc.backend_options.dx12.presentation_system =
                    wgpu::Dx12SwapchainKind::DxgiFromVisual;
            }
            // We never draw indirectly: its validation compiles a compute pipeline at device
            // creation (~23 MB on Metal) for nothing.
            desc.flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
            let instance = wgpu::Instance::new(desc);
            let surface = match instance.create_surface(Arc::clone(target)) {
                Ok(s) => s,
                Err(e) => {
                    last_error = e.to_string();
                    continue;
                }
            };
            match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    // An editor doesn't need the discrete GPU; the integrated one saves battery.
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: false,
                    compatible_surface: Some(&surface),
                    apply_limit_buckets: false,
                })
                .await
            {
                Ok(adapter) => return Ok((instance, surface, adapter)),
                Err(e) => last_error = format!("{backends:?}: {e}"),
            }
        }
        Err(RenderError::NoAdapter(last_error))
    }
}

/// How long the last window's renderer waits for the GPU's work when it goes (its
/// resources freed after it): long enough for a frame or two, never a hang.
const LAST_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

impl Drop for WgpuRenderer {
    /// The window's images are on screen no more. The last window's: what the GPU still
    /// held (resources let go but not yet done with) let go first, the GPU waited for —
    /// otherwise they outlive their memory's allocator, told in the log as leaks (DX12).
    fn drop(&mut self) {
        let last = {
            let mut state = self.ctx.state();
            state.shown.remove(&self.drawer.id);
            state.shown.is_empty()
        };
        if last {
            // Bounded: a driver that never answers doesn't keep the window from closing.
            let wait = wgpu::PollType::Wait { submission_index: None, timeout: Some(LAST_WAIT) };
            if let Err(e) = self.ctx.device.poll(wait) {
                tracing::warn!(error = %e, "the GPU's work wasn't done when the window closed");
            }
        }
    }
}

/// `f` given `buffer`'s bytes once the GPU wrote them (waits for it), where they're mapped
/// (never copied whole); `None` if they can't be.
fn with_mapped<R>(
    ctx: &GpuContext,
    buffer: &wgpu::Buffer,
    f: impl FnOnce(&[u8]) -> R,
) -> Option<R> {
    let (sent, mapped) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = sent.send(r.is_ok());
    });
    if let Err(e) = ctx.device.poll(wgpu::PollType::wait_indefinitely()) {
        tracing::warn!(error = %e, "the frame wasn't read back");
        return None;
    }
    if !mapped.recv().unwrap_or(false) {
        return None;
    }
    let result = f(&buffer.slice(..).get_mapped_range().ok()?);
    buffer.unmap();
    Some(result)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
