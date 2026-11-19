//! Contract for render drivers.

use std::fmt;
use std::sync::Arc;

use kernel::PhysicalSize;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::{GlyphRasterizer, Scene};

/// Any native window that can be drawn on. Implemented automatically for
/// types that expose `raw-window-handle` handles, so the driver doesn't
/// depend on the windowing library (winit or another).
pub trait RenderTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + fmt::Debug {}

impl<T: HasWindowHandle + HasDisplayHandle + Send + Sync + fmt::Debug + ?Sized> RenderTarget for T {}

/// Preferred graphics API. `Auto` lets the driver pick what's best on the platform.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GpuApi {
    #[default]
    Auto,
    Metal,
    Dx12,
    Vulkan,
    Gl,
}

impl GpuApi {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "metal" => Self::Metal,
            "dx12" | "d3d12" => Self::Dx12,
            "vulkan" => Self::Vulkan,
            "gl" | "opengl" => Self::Gl,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub vsync: bool,
    pub preferred_api: GpuApi,
    /// A surface that can be transparent (where the API allows it: DX12 through
    /// DirectComposition), so the window's corners can be cut ([`Scene::window_corners`]).
    pub transparent: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { vsync: true, preferred_api: GpuApi::Auto, transparent: false }
    }
}

/// Statistics for a frame (shown in the debug overlay).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub quads: usize,
    pub glyphs: usize,
    pub draw_calls: usize,
    /// Pixels drawn again: the whole surface for a full frame, the damaged areas' for a
    /// partial one, 0 when nothing changed.
    pub drawn_pixels: u64,
    /// Glyphs in the driver's cache after this frame.
    pub cached_glyphs: usize,
    /// Backdrop blurs made again this frame (0 while the ones kept still hold).
    pub blurs: usize,
    /// `true` if the frame was skipped (window hidden, surface being reconfigured).
    pub skipped: bool,
    /// Frame skipped for a transient reason: the app must request another one immediately.
    /// `false` for a hidden window, where the visibility event is awaited.
    pub retry: bool,
}

/// The GPU memory a window's renderer holds (bytes), estimated from what it created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuMemory {
    /// What every window draws with (one GPU device per process): the glyph atlas, the
    /// images' textures. The same in every window, so counted once across them.
    pub shared: u64,
    /// This window's own: its surface images, the kept frame, the blurs, the instance
    /// buffers.
    pub window: u64,
}

impl GpuMemory {
    /// All this window draws with.
    pub fn total(self) -> u64 {
        self.shared + self.window
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no compatible graphics adapter: {0}")]
    NoAdapter(String),
    #[error("drawing surface error: {0}")]
    Surface(String),
    #[error("graphics device error: {0}")]
    Device(String),
}

/// An active render driver, bound to a surface.
pub trait Renderer {
    /// Description for diagnostics, e.g. `wgpu / Metal / Apple M3`.
    fn description(&self) -> String;

    fn resize(&mut self, size: PhysicalSize);

    /// GPU memory allocated by the renderer (textures, buffers, surface images), estimated
    /// from the created resources: what's shared with the other windows apart. `None` if
    /// the driver can't estimate it.
    fn gpu_memory(&self) -> Option<GpuMemory> {
        None
    }

    /// Clears the GPU caches that are rebuilt on demand (glyph atlas, image textures).
    /// Called together with clearing the `TextSystem` (glyph keys are reused).
    fn purge_caches(&mut self) {}

    /// The window was hidden or minimized (`false`) or shown again: while hidden, a renderer
    /// that keeps the last frame (for partial redraws) gives that memory back; the first
    /// frame after is drawn whole.
    fn set_visible(&mut self, _visible: bool) {}

    /// The window has been hidden a while (minimized, covered): the image on screen goes
    /// too — a frame of one pixel takes its place, nothing of it seen; the next frame, once
    /// it's shown again, is drawn whole at the window's size (plan 76f).
    fn release_hidden(&mut self) {}

    /// The window lost the focus (it rarely draws now): what's kept only to redraw it
    /// quickly — the last frame, the blurs, each as large as the window — is given back; the
    /// next frame is drawn whole and keeps them again.
    fn release_idle(&mut self) {}

    /// When the frame kept for partial redraws goes if nothing more is drawn: a frame asked
    /// for then gives it back. `None`: none is kept.
    fn kept_frame_release_at(&self) -> Option<std::time::Instant> {
        None
    }

    /// A [`capture`](Self::capture) found no kept frame: it waits for the next one rendered.
    fn capture_waits_for_frame(&self) -> bool {
        false
    }

    /// Whether its surface is transparent where the scene leaves it so (what's under it
    /// shows: a floating layer over the pages needs it).
    fn transparent(&self) -> bool {
        false
    }

    /// Whether it draws the layers' backdrop blurs ([`crate::Layer::backdrop_blur`]); one
    /// that doesn't leaves them out (the app then draws its backdrops without them).
    fn backdrop_blur(&self) -> bool {
        false
    }

    fn render(
        &mut self,
        scene: &Scene,
        glyphs: &mut dyn GlyphRasterizer,
    ) -> Result<FrameStats, RenderError>;

    /// A picture of the last frame at most `max_width` wide, given to `done` on a
    /// background thread once read (the window's picture in the open projects' list).
    /// `false`: nothing to picture (no frame kept), or a renderer that can't.
    fn capture(&mut self, max_width: u32, done: Box<dyn FnOnce(crate::Captured) + Send>) -> bool {
        let _ = (max_width, done);
        false
    }
}

/// Creates renderers for windows. The app tries the factories in order of
/// priority and uses the first one that succeeds.
pub trait RendererFactory {
    fn name(&self) -> &'static str;

    fn create(
        &self,
        target: Arc<dyn RenderTarget>,
        size: PhysicalSize,
        options: &RenderOptions,
    ) -> Result<Box<dyn Renderer>, RenderError>;

    /// Starts making, in the background, what needs no window (the GPU's device, its
    /// shaders), before the first window exists; its `create` takes it. Nothing by default.
    fn prepare(&self, _options: &RenderOptions) {}
}
