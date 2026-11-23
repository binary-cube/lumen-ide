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
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(test, feature = "offscreen"))]
mod offscreen;
mod prepared;

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

/// What draws into a surface of one format: the same for every window with it.
#[derive(Debug)]
struct Pipelines {
    /// Every layer's rectangles and glyphs, in one draw ([`PrimGpu`]).
    prim: wgpu::RenderPipeline,
    /// Writes the background over a partial frame's areas.
    fill: wgpu::RenderPipeline,
    corner: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,
    /// Made when the first blur is drawn.
    blur: std::sync::OnceLock<Arc<BlurPipelines>>,
}

impl Pipelines {
    fn new(ctx: &GpuContext, format: wgpu::TextureFormat) -> Self {
        let device = &ctx.device;
        let quad_attributes = &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2];
        let quad_layout = (&quad_attributes[..], std::mem::size_of::<QuadGpu>() as u64);
        // Pipelines depend on the surface format (of each window).
        let fill_pipeline = create_pipeline(
            device,
            &ctx.shader,
            format,
            &[Some(&ctx.globals_layout)],
            ("vs_quad", "fs_quad"),
            quad_layout,
            wgpu::BlendState::REPLACE,
        );
        let prim_pipeline = create_pipeline(
            device,
            &ctx.shader,
            format,
            &[Some(&ctx.globals_layout), Some(&ctx.atlas_layout)],
            ("vs_prim", "fs_prim"),
            (
                &wgpu::vertex_attr_array![
                    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Uint16x4,
                    4 => Float32x4, 5 => Uint32
                ],
                std::mem::size_of::<PrimGpu>() as u64,
            ),
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        // The window's corners: what's drawn there is erased (`dst * (1 - alpha)`).
        let erase = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let corner_pipeline = create_pipeline(
            device,
            &ctx.shader,
            format,
            &[Some(&ctx.globals_layout)],
            ("vs_quad", "fs_window_corner"),
            quad_layout,
            wgpu::BlendState { color: erase, alpha: erase },
        );
        let image_pipeline = create_pipeline(
            device,
            &ctx.image_shader,
            format,
            &[Some(&ctx.globals_layout), Some(&ctx.image_layout)],
            ("vs_image", "fs_image"),
            (
                &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32],
                std::mem::size_of::<ImageGpu>() as u64,
            ),
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        Self {
            prim: prim_pipeline,
            fill: fill_pipeline,
            corner: corner_pipeline,
            image: image_pipeline,
            blur: std::sync::OnceLock::new(),
        }
    }
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

/// The bind group exposing the atlas textures to the glyph shader.
fn atlas_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    atlas: &GlyphAtlas,
) -> wgpu::BindGroup {
    let (mask_view, color_view) = (atlas.mask_view(), atlas.color_view());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glyph-atlas"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
        ],
    })
}

/// `Globals` in `common.wgsl`: the viewport and the emphasis a batch is drawn with (all
/// zero: none).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    _pad: [f32; 2],
    /// Gray, dim, feather, how many kept areas.
    emphasis: [f32; 4],
    keep_rects: [[f32; 4]; MAX_KEEPS],
    /// Each kept area's corner radius and weight.
    keep_params: [[f32; 4]; MAX_KEEPS],
}

impl Globals {
    fn new(size: PhysicalSize, emphasis: Option<&Emphasis>) -> Self {
        let mut g = Self::zeroed();
        g.viewport = [size.width as f32, size.height as f32];
        if let Some(e) = emphasis {
            let keeps = e.keeps();
            g.emphasis = [e.gray, e.dim, e.feather, keeps.len() as f32];
            for (i, k) in keeps.iter().enumerate() {
                g.keep_rects[i] = rect_array(&k.rect);
                g.keep_params[i] = [k.radius, k.weight, 0.0, 0.0];
            }
        }
        g
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct QuadGpu {
    rect: [f32; 4],
    color: [f32; 4],
    border_color: [f32; 4],
    params: [f32; 2],
}

/// A rectangle or a glyph as the one pipeline draws it (`PrimInstance` in `shaders.wgsl`),
/// with its layer's clip and emphasis: every layer's in one draw, in the scene's order.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PrimGpu {
    /// x, y, w, h (a glyph's in whole pixels).
    rect: [f32; 4],
    color: [f32; 4],
    border_color: [f32; 4],
    /// Its layer's clip within the area drawn: x0, y0, x1, y1 (whole pixels; a surface is
    /// never wider than a `u16` counts).
    clip: [u16; 4],
    /// A quad's corner radius and border width; a glyph's place in the atlas.
    params: [f32; 4],
    /// Its kind ([`PRIM_QUAD`], [`PRIM_MASK`], [`PRIM_COLOR`]), muted ([`PRIM_MUTED`]).
    flags: u32,
}

/// [`PrimGpu::flags`]: a rectangle, a glyph's mask, a glyph's own colors; muted by the
/// emphasis.
const PRIM_QUAD: u32 = 0;
const PRIM_MASK: u32 = 1;
const PRIM_COLOR: u32 = 2;
const PRIM_MUTED: u32 = 4;

impl PrimGpu {
    fn quad(q: &render_api::Quad) -> Self {
        Self {
            rect: rect_array(&q.bounds),
            color: color_array(q.color),
            border_color: color_array(q.border_color),
            clip: [0; 4],
            params: [q.corner_radius, q.border_width, 0.0, 0.0],
            flags: PRIM_QUAD,
        }
    }

    /// This instance cut by `clip`, muted or not.
    fn within(mut self, clip: PixelRect, muted: bool) -> Self {
        let at = |v: i32| u16::try_from(v.max(0)).unwrap_or(u16::MAX);
        self.clip = [at(clip.x0), at(clip.y0), at(clip.x1), at(clip.y1)];
        if muted {
            self.flags |= PRIM_MUTED;
        }
        self
    }
}

/// An image instance (`ImageInstance` in `image.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ImageGpu {
    rect: [f32; 4],
    /// A mask's color (straight alpha); alpha below 0: an RGBA image.
    tint: [f32; 4],
    opacity: f32,
    _pad: [f32; 3],
}

/// An image uploaded to the GPU.
#[derive(Debug)]
struct GpuImage {
    bind_group: wgpu::BindGroup,
    bytes: u64,
    /// The last frame in which it was drawn (unused ones are freed).
    last_frame: u64,
    /// When it was last drawn: when idle no frames are drawn, so frame counts alone would
    /// keep an image (e.g. the splash art) on the GPU indefinitely.
    last_used: std::time::Instant,
    /// Told when the texture is freed (a `gpu_only` image must then be decoded again).
    image: render_api::ImageData,
}

/// How many frames an image stays on the GPU without being drawn.
const IMAGE_TTL: u64 = 600;
/// …or that many seconds.
const IMAGE_IDLE: std::time::Duration = std::time::Duration::from_secs(20);
/// How long after the last resize frames are still drawn as while resizing (whole, straight
/// into the surface): a drag's steps come closer than this.
#[cfg(not(target_os = "macos"))]
const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

#[derive(Debug)]
struct Batch {
    /// The scene's layer it starts with (a run of primitives may go on to later layers,
    /// never past one whose blur is made again: a render pass ends there).
    layer: usize,
    step: Step,
}

/// What a [`Batch`] draws.
#[derive(Debug)]
enum Step {
    /// Rectangles and glyphs of one or more layers, each cut by its own clip: one draw.
    Prims(Range<u32>),
    /// A layer's images, each with its texture, cut by the layer's clip (`scissor`).
    Images { scissor: PixelRect, range: Range<u32> },
    /// A layer's backdrop blurs drawn back (in the blur instances), cut by its clip.
    Blurs { scissor: PixelRect, range: Range<u32> },
}

/// A layer's instances, before they're split by area: where they are in the drawer's
/// lists of the frame's layers, and its clip.
#[derive(Debug)]
struct LayerPlan {
    index: usize,
    clip: PixelRect,
    quads: Range<usize>,
    images: Range<usize>,
    glyphs: Range<usize>,
    /// Its blur's image, when it blurs (x, y, w, h).
    blur_image: Option<[f32; 4]>,
}

/// Instance buffer that grows as needed (never shrinks).
#[derive(Debug)]
struct InstanceBuffer {
    buffer: wgpu::Buffer,
    label: &'static str,
}

impl InstanceBuffer {
    fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self { buffer: Self::allocate(device, label, 64 * 1024), label }
    }

    fn allocate(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Copies `data` in through the staging belt (its buffers reused from frame to frame:
    /// `queue.write_buffer` would make a staging buffer per call, every frame).
    fn upload<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        belt: &mut wgpu::util::StagingBelt,
        encoder: &mut wgpu::CommandEncoder,
        data: &[T],
    ) {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) else { return };
        if size.get() > self.buffer.size() {
            self.buffer = Self::allocate(device, self.label, size.get().next_power_of_two());
        }
        belt.write_buffer(encoder, &self.buffer, 0, size).copy_from_slice(bytes);
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

    fn surface_config(
        surface: &wgpu::Surface<'_>,
        adapter: &wgpu::Adapter,
        size: PhysicalSize,
        options: &RenderOptions,
    ) -> Result<wgpu::SurfaceConfiguration, RenderError> {
        let mut config =
            surface.get_default_config(adapter, size.width.max(1), size.height.max(1)).ok_or_else(
                || RenderError::Surface("the surface isn't supported by the adapter".into()),
            )?;
        let caps = surface.get_capabilities(adapter);

        // Format without sRGB conversion: blending in sRGB space gives text closer
        // to the system's native rendering (no thinned glyphs).
        if let Some(format) = caps.formats.iter().find(|f| !f.is_srgb()) {
            config.format = *format;
        }
        // Transparent where the scene leaves it so (the window's corners, a floating layer),
        // when it can be: premultiplied (DX12, Vulkan); on Metal "post-multiplied" only makes
        // the layer not opaque — Core Animation reads its pixels premultiplied, as drawn.
        if options.transparent {
            if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
            } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
                config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
            }
        }
        config.present_mode = if options.vsync {
            wgpu::PresentMode::Fifo
        } else {
            [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
                .into_iter()
                .find(|m| caps.present_modes.contains(m))
                .unwrap_or(wgpu::PresentMode::Fifo)
        };
        // 1 = two images (one shown, one drawn): an editor's frames are few and quick. The
        // third one exists only while resizing (see `resize`): on Metal, with only two,
        // presenting in sync with the window blocks acquiring the next frame for up to ~1 s;
        // elsewhere a drag's frames come close. A window-sized image less the rest of the time.
        config.desired_maximum_frame_latency = 1;
        Ok(config)
    }

    fn reconfigure(&mut self) {
        if self.config.width > 0 && self.config.height > 0 {
            self.surface.configure(&self.ctx.device, &self.config);
        }
    }

    fn recreate_surface(&mut self) -> Result<(), RenderError> {
        self.replace_surface()?;
        self.reconfigure();
        Ok(())
    }

    /// A new surface for the window, not configured yet; the old one goes with all its images
    /// (on macOS its layer is taken out of the window first: left there, it would keep
    /// showing — and holding — its last image).
    fn replace_surface(&mut self) -> Result<(), RenderError> {
        let surface = self
            .ctx
            .instance
            .create_surface(Arc::clone(&self.target))
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        #[cfg(target_os = "macos")]
        {
            macos::detach_layer(&self.surface);
            macos::configure_layer(&surface);
            macos::lower_layer(&surface);
            if self.transactional_until.is_some() {
                macos::set_transactional(&surface, true);
            }
            self.spare_released = false;
        }
        self.surface = surface;
        self.spare_held_since = None;
        Ok(())
    }

    /// A window hidden a while gives back its surface's images, `true` when it did (the next
    /// frame sets the surface up again). macOS: the layer goes, with every image it holds
    /// (a covered window is given no image to replace them with: wgpu skips it,
    /// gfx-rs/wgpu#8309); a new, empty one takes its place.
    #[cfg(target_os = "macos")]
    fn give_back_surface(&mut self) -> bool {
        match self.replace_surface() {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(error = %e, "the hidden window's surface stays");
                false
            }
        }
    }

    /// Elsewhere: the surface made one pixel large, its images given back once a 1×1 frame
    /// replaces the one on screen.
    #[cfg(not(target_os = "macos"))]
    fn give_back_surface(&mut self) -> bool {
        let tiny = wgpu::SurfaceConfiguration {
            width: 1,
            height: 1,
            desired_maximum_frame_latency: 1,
            ..self.config.clone()
        };
        self.surface.configure(&self.ctx.device, &tiny);
        if let wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = self.surface.get_current_texture()
        {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hidden") });
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hidden"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
            self.ctx.queue.submit([encoder.finish()]);
            self.ctx.queue.present(frame);
        }
        true
    }

    /// The surface's next image. On resize the surface may be "outdated": it's reconfigured
    /// and asked again immediately, in the same frame, so the window doesn't show a stretched
    /// image (resize jitter).
    /// The surface's image not on screen goes back too (macOS keeps a pool of two).
    fn release_spare_image(&mut self) {
        self.spare_held_since = None;
        #[cfg(target_os = "macos")]
        {
            macos::release_spare_drawables(&self.surface);
            self.spare_released = true;
        }
        // What was dropped goes when the GPU says it's done with it: no frame may follow.
        let _ = self.ctx.device.poll(wgpu::PollType::Poll);
    }

    fn acquire_frame(&mut self) -> Result<Acquired, RenderError> {
        let skipped = FrameStats { skipped: true, ..FrameStats::default() };
        let retry = FrameStats { retry: true, ..skipped };
        let mut attempts = 0;
        loop {
            attempts += 1;
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => {
                    return Ok(Acquired::Frame { frame, suboptimal: false });
                }
                // Draw anyway (otherwise the old image stays), then reconfigure.
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    return Ok(Acquired::Frame { frame, suboptimal: true });
                }
                wgpu::CurrentSurfaceTexture::Outdated if attempts < 2 => {
                    tracing::debug!("surface outdated; reconfiguring");
                    self.reconfigure();
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Timeout => {
                    tracing::debug!("frame unavailable; retrying");
                    return Ok(Acquired::Skipped(retry));
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    tracing::debug!("window hidden; frame skipped");
                    return Ok(Acquired::Skipped(skipped));
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    tracing::debug!("surface lost; recreating it");
                    self.recreate_surface()?;
                    return Ok(Acquired::Skipped(retry));
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(RenderError::Surface("surface validation error".into()));
                }
            }
        }
    }
}

/// The result of asking the surface for its next image.
enum Acquired {
    /// An image to draw into; `suboptimal`: the surface is reconfigured after presenting it.
    Frame { frame: wgpu::SurfaceTexture, suboptimal: bool },
    /// No image this time: the frame is skipped with these stats.
    Skipped(FrameStats),
}

impl Drop for WgpuRenderer {
    /// The window's images are on screen no more.
    fn drop(&mut self) {
        self.ctx.state().shown.remove(&self.drawer.id);
    }
}

impl Renderer for WgpuRenderer {
    fn description(&self) -> String {
        let info = &self.ctx.adapter_info;
        format!("wgpu / {:?} / {}", info.backend, info.name)
    }

    fn purge_caches(&mut self) {
        // The GPU context is shared by windows: clearing affects all of them.
        let mut state = self.ctx.state();
        // Back to the starting size (it grows again only if needed).
        state.atlas.grow(&self.ctx.device, ATLAS_START);
        state.atlas_bind_group =
            atlas_bind_group(&self.ctx.device, &self.ctx.atlas_layout, &state.atlas);
        for t in state.textures.values() {
            t.image.evicted();
        }
        state.textures.clear();
        // The text system reissues glyph keys with it: every window's next frame is whole.
        render_api::glyph_keys_reset();
    }

    fn gpu_memory(&self) -> Option<render_api::GpuMemory> {
        // Shared by the windows (one GPU context): the atlas (R8 mask + RGBA8 color) and the
        // images' textures. This window's: its instance buffers, surface images
        // ([`surface_bytes`]), the kept frame and the blurs.
        let state = self.ctx.state();
        let side = u64::from(state.atlas.size());
        let color_side = (side / 4).max(256);
        let shared = side * side
            + color_side * color_side * 4
            + state.textures.values().map(|t| t.bytes).sum::<u64>();
        drop(state);
        let drawer = &self.drawer;
        let buffers = drawer.quad_buffer.buffer.size()
            + drawer.prim_buffer.buffer.size()
            + drawer.image_buffer.buffer.size();
        #[cfg(target_os = "macos")]
        let spare_released = self.spare_released;
        #[cfg(not(target_os = "macos"))]
        let spare_released = false;
        let surface = surface_bytes(
            PhysicalSize::new(self.config.width, self.config.height),
            self.config.desired_maximum_frame_latency,
            spare_released,
            self.hidden_released,
        );
        let window = buffers + surface + drawer.kept_bytes() + drawer.blur_bytes();
        Some(render_api::GpuMemory { shared, window })
    }

    fn set_visible(&mut self, visible: bool) {
        if !visible {
            self.release_idle();
        } else if std::mem::take(&mut self.hidden_released) {
            self.reconfigure();
        }
    }

    fn release_hidden(&mut self) {
        if self.hidden_released || self.config.width == 0 || self.config.height == 0 {
            return;
        }
        self.release_idle();
        // The frame on screen goes with the surface: the next one is drawn whole.
        self.drawer.shown = None;
        if self.give_back_surface() {
            self.hidden_released = true;
        }
        // What was given back goes when the GPU is done with it.
        let _ = self.ctx.device.poll(wgpu::PollType::Poll);
    }

    fn release_idle(&mut self) {
        // A capture's copy of the kept frame was submitted already: it keeps what it reads.
        self.drawer.release_kept();
        self.drawer.release_blurs();
        self.release_spare_image();
        drawer::shrink_atlas(&self.ctx);
        // No frame may come for long (the app in the background): the images on screen
        // nowhere go now if they're old enough, not at the next frame.
        self.ctx.state().release_unshown();
    }

    fn kept_frame_release_at(&self) -> Option<std::time::Instant> {
        let quiet = self.activity.quiet_at()?;
        let kept = self.drawer.kept.as_ref().map(|_| quiet);
        let spare = self.spare_held_since.map(|shown| quiet.max(shown + SPARE_AFTER));
        // An image off every screen (a tab left) is freed when it's old enough even if no
        // frame comes then (frames free it themselves while drawing).
        let images = self.ctx.state().unshown_release_at().map(|at| quiet.max(at));
        // Only ahead: one that can't go yet (a blur reads it) waits for the next frame.
        let now = std::time::Instant::now();
        kept.into_iter().chain(spare).chain(images).filter(|at| *at > now).min()
    }

    fn capture_waits_for_frame(&self) -> bool {
        self.pending_capture.is_some()
    }

    fn backdrop_blur(&self) -> bool {
        // The blur reads the kept frame.
        self.partial
    }

    fn transparent(&self) -> bool {
        self.drawer.transparent
    }

    fn capture(
        &mut self,
        max_width: u32,
        done: Box<dyn FnOnce(render_api::Captured) + Send>,
    ) -> bool {
        // A quiet window keeps no frame: the next one drawn is kept for this.
        if self.drawer.kept.is_none() {
            self.pending_capture = Some(PendingCapture { max_width, done });
            return true;
        }
        // One at a time: a frame's read-back buffer is as large as the window.
        if self.capturing.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return false;
        }
        // Copied now (the next frame may change the kept one); waited for and made small in
        // the background, from where it's mapped.
        let Some((buffer, layout)) = self.drawer.copy_kept(&self.ctx) else {
            self.capturing.store(false, std::sync::atomic::Ordering::Release);
            return false;
        };
        let (ctx, bgra, capturing) =
            (Arc::clone(&self.ctx), self.drawer.bgra(), Arc::clone(&self.capturing));
        kernel::work::run("frame-capture", move || {
            let picture = with_mapped(&ctx, &buffer, |data| {
                render_api::shrink(data, layout, bgra, max_width)
            });
            // The buffer goes before the next capture may make another.
            drop(buffer);
            capturing.store(false, std::sync::atomic::Ordering::Release);
            if let Some(picture) = picture {
                done(picture);
            }
        });
        true
    }

    fn resize(&mut self, size: PhysicalSize) {
        // The size it has already (the system tells it again as the window opens): nothing to
        // set up, and no third image for a resize that isn't one.
        if !resizes(&self.config, size) {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        if size.width == 0 || size.height == 0 {
            // Minimized.
            self.drawer.release_kept();
        }
        self.config.desired_maximum_frame_latency = 2;
        self.reconfigure();
        #[cfg(target_os = "macos")]
        {
            if self.transactional_until.is_none() {
                macos::set_transactional(&self.surface, true);
            }
            self.transactional_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(300));
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.resized_until = Some(std::time::Instant::now() + RESIZE_SETTLE);
        }
    }

    fn render(
        &mut self,
        scene: &Scene,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> Result<FrameStats, RenderError> {
        // The resize is over: frames no longer wait for the window's transaction.
        #[cfg(target_os = "macos")]
        if self.transactional_until.is_some_and(|t| std::time::Instant::now() >= t) {
            self.transactional_until = None;
            macos::set_transactional(&self.surface, false);
            // Back to two drawables (the third is needed only while resizing).
            self.config.desired_maximum_frame_latency = 1;
            self.reconfigure();
        }
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(FrameStats { skipped: true, ..FrameStats::default() });
        }
        // Drawn while hidden (or shown without being told): the window's size again.
        if std::mem::take(&mut self.hidden_released) {
            self.reconfigure();
        }
        let size = PhysicalSize::new(self.config.width, self.config.height);
        // While resizing, every frame is whole and of another size: no kept frame (a texture
        // made per step), straight into the surface's image.
        #[cfg(target_os = "macos")]
        let resizing = self.transactional_until.is_some();
        #[cfg(not(target_os = "macos"))]
        let resizing = {
            if self.resized_until.is_some_and(|t| std::time::Instant::now() >= t) {
                self.resized_until = None;
                // Back to two images (the third is needed only while resizing).
                self.config.desired_maximum_frame_latency = 1;
                self.reconfigure();
            }
            self.resized_until.is_some()
        };
        let now = std::time::Instant::now();
        self.activity.frame(now);
        // A window standing still gives its kept frame back (as large as the window): frames
        // are few, drawing one whole costs little. The blurs and a capture read it.
        let blurs = scene.layers().iter().any(|l| !l.blurs.is_empty());
        let keep = self.partial
            && !resizing
            && (!self.activity.quiet(now) || blurs || self.pending_capture.is_some());
        if !keep {
            let had_one = self.drawer.kept.is_some();
            self.drawer.release_kept();
            if had_one && !resizing {
                self.release_spare_image();
            }
        }
        if self.partial && !resizing {
            self.drawer.compute_damage(&self.ctx, scene, size, rasterizer, keep);
            // Stood still long enough (a caret's blink never does): the spare image goes too.
            if !keep
                && self.drawer.damage.is_empty()
                && self.spare_held_since.is_some_and(|shown| now >= shown + SPARE_AFTER)
            {
                self.release_spare_image();
            }
            // Nothing changed: what's on screen is already this frame.
            if self.drawer.damage.is_empty() {
                let state = self.ctx.state();
                return Ok(FrameStats {
                    quads: scene.quad_count(),
                    glyphs: scene.glyph_count(),
                    cached_glyphs: state.atlas.len(),
                    ..FrameStats::default()
                });
            }
        }
        self.drawer.shown = None;
        #[cfg(target_os = "macos")]
        if std::mem::take(&mut self.spare_released) {
            self.reconfigure();
        }
        let (frame, suboptimal) = match self.acquire_frame()? {
            Acquired::Frame { frame, suboptimal } => (frame, suboptimal),
            Acquired::Skipped(stats) => {
                if !stats.retry {
                    // Hidden: the kept frame's memory goes back meanwhile.
                    self.drawer.release_kept();
                }
                return Ok(stats);
            }
        };

        let surface_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // Glyph rasterization and atlas uploads only once a frame can actually be drawn
        // (a hidden window or a timeout would waste that work).
        let ctx = Arc::clone(&self.ctx);
        // The whole frame changes (an animation over the window): straight on screen, no
        // copy (measured: the copy and its pass were a fifth of the frame's CPU); the frame
        // kept catches up when a partial one comes. Not with blurs or a capture: they read it.
        let whole =
            keep && self.drawer.damage.is_full() && !blurs && self.pending_capture.is_none();
        let (draw_calls, drawn_pixels) = if whole {
            self.drawer.draw_straight(&ctx, &mut encoder, scene, size, rasterizer, &surface_view)
        } else if keep {
            let drawn = self.drawer.draw_kept(&ctx, &mut encoder, scene, size, rasterizer);
            if let Some(kept) = &self.drawer.kept {
                encoder.copy_texture_to_texture(
                    kept.texture.as_image_copy(),
                    frame.texture.as_image_copy(),
                    wgpu::Extent3d {
                        width: size.width,
                        height: size.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            drawn
        } else {
            self.drawer.draw(&ctx, &mut encoder, scene, size, rasterizer, Some(&surface_view))
        };
        self.ctx.queue.submit([encoder.finish()]);
        self.ctx.queue.present(frame);
        self.drawer.shown = Some(size);
        self.spare_held_since = cfg!(target_os = "macos").then(std::time::Instant::now);
        if suboptimal {
            self.reconfigure();
        }
        if let Some(PendingCapture { max_width, done }) = self.pending_capture.take() {
            self.capture(max_width, done);
        }

        Ok(FrameStats {
            quads: self.drawer.drawn.0,
            glyphs: self.drawer.drawn.1,
            draw_calls,
            drawn_pixels,
            cached_glyphs: ctx.state().atlas.len(),
            blurs: if keep { self.drawer.stale.len() } else { 0 },
            skipped: false,
            retry: false,
        })
    }
}

/// Whether `size` changes the surface set up by `config`.
fn resizes(config: &wgpu::SurfaceConfiguration, size: PhysicalSize) -> bool {
    (config.width, config.height) != (size.width, size.height)
}

/// The bytes a window's surface images hold (4 a pixel): one per frame in flight
/// (`latency`) plus the one on screen; only that one once the spare went (macOS); while
/// hidden none (macOS: the layer went with them) or one pixel's (elsewhere: made 1×1).
fn surface_bytes(size: PhysicalSize, latency: u32, spare_released: bool, hidden: bool) -> u64 {
    let images = if spare_released { 1 } else { u64::from(latency) + 1 };
    let (width, height) = match hidden {
        false => (size.width, size.height),
        true if cfg!(target_os = "macos") => return 0,
        true => (1, 1),
    };
    u64::from(width) * u64::from(height) * 4 * images
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

pub(crate) fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    (vertex_entry, fragment_entry): (&str, &str),
    (attributes, stride): (&[wgpu::VertexAttribute], u64),
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(vertex_entry),
        bind_group_layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vertex_entry),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex_entry),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn rect_array(r: &Rect) -> [f32; 4] {
    [r.origin.x, r.origin.y, r.size.width, r.size.height]
}

fn color_array(c: Rgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
