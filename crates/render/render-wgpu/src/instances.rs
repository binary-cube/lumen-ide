//! What a frame sends the GPU: the globals, the instances of quads, glyphs and images, the
//! batches and steps a layer is drawn in, and the buffers they're uploaded to.

use super::*;

/// `Globals` in `common.wgsl`: the viewport and the emphasis a batch is drawn with (all
/// zero: none).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct Globals {
    viewport: [f32; 2],
    _pad: [f32; 2],
    /// Gray, dim, feather, how many kept areas.
    emphasis: [f32; 4],
    keep_rects: [[f32; 4]; MAX_KEEPS],
    /// Each kept area's corner radius and weight.
    keep_params: [[f32; 4]; MAX_KEEPS],
}

impl Globals {
    pub(crate) fn new(size: PhysicalSize, emphasis: Option<&Emphasis>) -> Self {
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
pub(crate) struct QuadGpu {
    pub(crate) rect: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) border_color: [f32; 4],
    pub(crate) params: [f32; 2],
}

/// A rectangle or a glyph as the one pipeline draws it (`PrimInstance` in `shaders.wgsl`),
/// with its layer's clip and emphasis: every layer's in one draw, in the scene's order.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct PrimGpu {
    /// x, y, w, h (a glyph's in whole pixels).
    pub(crate) rect: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) border_color: [f32; 4],
    /// Its layer's clip within the area drawn: x0, y0, x1, y1 (whole pixels; a surface is
    /// never wider than a `u16` counts).
    pub(crate) clip: [u16; 4],
    /// A quad's corner radius and border width; a glyph's place in the atlas.
    pub(crate) params: [f32; 4],
    /// Its kind ([`PRIM_QUAD`], [`PRIM_MASK`], [`PRIM_COLOR`]), muted ([`PRIM_MUTED`]).
    pub(crate) flags: u32,
}

/// [`PrimGpu::flags`]: a rectangle, a glyph's mask, a glyph's own colors; muted by the
/// emphasis.
const PRIM_QUAD: u32 = 0;
pub(crate) const PRIM_MASK: u32 = 1;
pub(crate) const PRIM_COLOR: u32 = 2;
const PRIM_MUTED: u32 = 4;

impl PrimGpu {
    pub(crate) fn quad(q: &render_api::Quad) -> Self {
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
    pub(crate) fn within(mut self, clip: PixelRect, muted: bool) -> Self {
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
pub(crate) struct ImageGpu {
    pub(crate) rect: [f32; 4],
    /// A mask's color (straight alpha); alpha below 0: an RGBA image.
    pub(crate) tint: [f32; 4],
    pub(crate) opacity: f32,
    pub(crate) _pad: [f32; 3],
}

/// An image uploaded to the GPU.
#[derive(Debug)]
pub(crate) struct GpuImage {
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) bytes: u64,
    /// The last frame in which it was drawn (unused ones are freed).
    pub(crate) last_frame: u64,
    /// When it was last drawn: when idle no frames are drawn, so frame counts alone would
    /// keep an image (e.g. the splash art) on the GPU indefinitely.
    pub(crate) last_used: std::time::Instant,
    /// Told when the texture is freed (a `gpu_only` image must then be decoded again).
    pub(crate) image: render_api::ImageData,
}

/// How many frames an image stays on the GPU without being drawn.
pub(crate) const IMAGE_TTL: u64 = 600;
/// …or that many seconds.
pub(crate) const IMAGE_IDLE: std::time::Duration = std::time::Duration::from_secs(20);
/// How long after the last resize frames are still drawn as while resizing (whole, straight
/// into the surface): a drag's steps come closer than this.
#[cfg(not(target_os = "macos"))]
pub(crate) const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

#[derive(Debug)]
pub(crate) struct Batch {
    /// The scene's layer it starts with (a run of primitives may go on to later layers,
    /// never past one whose blur is made again: a render pass ends there).
    pub(crate) layer: usize,
    pub(crate) step: Step,
}

/// What a [`Batch`] draws.
#[derive(Debug)]
pub(crate) enum Step {
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
pub(crate) struct LayerPlan {
    pub(crate) index: usize,
    pub(crate) clip: PixelRect,
    pub(crate) quads: Range<usize>,
    pub(crate) images: Range<usize>,
    pub(crate) glyphs: Range<usize>,
    /// Its blur's image, when it blurs (x, y, w, h).
    pub(crate) blur_image: Option<[f32; 4]>,
}

/// Instance buffer that grows as needed (never shrinks).
#[derive(Debug)]
pub(crate) struct InstanceBuffer {
    pub(crate) buffer: wgpu::Buffer,
    label: &'static str,
}

impl InstanceBuffer {
    pub(crate) fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self { buffer: Self::allocate(device, label, 64 * 1024), label }
    }

    pub(crate) fn allocate(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Copies `data` in through the staging belt (its buffers reused from frame to frame:
    /// `queue.write_buffer` would make a staging buffer per call, every frame).
    pub(crate) fn upload<T: Pod>(
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

pub(crate) fn rect_array(r: &Rect) -> [f32; 4] {
    [r.origin.x, r.origin.y, r.size.width, r.size.height]
}

pub(crate) fn color_array(c: Rgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}
