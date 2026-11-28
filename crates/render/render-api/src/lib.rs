//! Rendering contracts: Scene, Primitive, Renderer, RendererFactory.
//!
//! The UI describes each frame as a [`Scene`], independent of the backend.
//! Drivers (wgpu, software, ...) implement [`Renderer`] and are known to the
//! rest of the app only through these contracts.

mod blur;
mod capture;
mod damage;
mod emphasis;
mod glyph;
mod live;
mod renderer;
mod scene;

pub use blur::{BackdropBlur, BlurLayer, BlurPlan, MAX_TAPS, blur_layers, rounded_coverage};
pub use capture::{Captured, shrink};
pub use damage::{Damage, DamageTracker, GlyphBox, PixelRect, glyph_keys_reset, partial_redraw};
pub use emphasis::{Emphasis, Keep, LUMA, MAX_KEEPS, mute, rounded_rect_distance};
pub use glyph::{GlyphFormat, GlyphKey, GlyphRasterizer, RasterizedGlyph};
pub use live::{CopiedFrame, FrameImporter, GpuAdapter, ImportedFrame, NativeFrame, PixelFrame};
pub use renderer::{
    FrameStats, GpuApi, GpuInfo, GpuMemory, GpuPreference, RenderError, RenderOptions,
    RenderTarget, Renderer, RendererFactory,
};
pub use scene::{
    GlyphInstance, ImageCorners, ImageData, ImageInstance, Layer, Quad, Scene, pend_copy,
    take_pending_copies,
};

pub use kernel::{Corners, PhysicalSize, Point, Rect, Rgba, Size};
