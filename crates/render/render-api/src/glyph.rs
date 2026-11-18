//! Contract between the text system and the renderer for glyphs.

/// Opaque identifier of a rasterizable glyph (font, glyph, size, subpixel).
/// Issued by the text system; the renderer uses it only as a cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphFormat {
    /// 8-bit coverage per pixel; tinted with the text color.
    Mask,
    /// Non-premultiplied RGBA8 (e.g. color emoji).
    Color,
}

/// A glyph's bitmap. The bitmap's top-left corner is at
/// `(origin.x + left, origin.y - top)`, where `origin` is the position on the baseline.
#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub format: GlyphFormat,
    /// `width * height` bytes for `Mask`, `width * height * 4` for `Color`.
    pub data: Vec<u8>,
}

/// Source of glyph bitmaps. The renderer calls it only for glyphs
/// that aren't already in its cache (the atlas).
pub trait GlyphRasterizer {
    /// `None` if the glyph has no visual representation (e.g. space).
    fn rasterize(&mut self, key: GlyphKey) -> Option<RasterizedGlyph>;
}
