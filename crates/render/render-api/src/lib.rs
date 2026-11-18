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
mod renderer;
mod scene;

pub use blur::{BackdropBlur, BlurLayer, BlurPlan, MAX_TAPS, blur_layers, rounded_coverage};
pub use capture::{Captured, shrink};
pub use damage::{Damage, DamageTracker, GlyphBox, PixelRect, glyph_keys_reset, partial_redraw};
pub use emphasis::{Emphasis, Keep, LUMA, MAX_KEEPS, mute, rounded_rect_distance};
pub use glyph::{GlyphFormat, GlyphKey, GlyphRasterizer, RasterizedGlyph};
pub use renderer::{
    FrameStats, GpuApi, GpuMemory, RenderError, RenderOptions, RenderTarget, Renderer,
    RendererFactory,
};
pub use scene::{GlyphInstance, ImageData, ImageInstance, Layer, Quad, Scene};

pub use kernel::{PhysicalSize, Point, Rect, Rgba, Size};
