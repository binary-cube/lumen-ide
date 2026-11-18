//! Glyph atlas: two textures (R8 mask and RGBA8 color) with shelf allocation.

use std::collections::HashMap;

use etagere::{AtlasAllocator, size2};
use render_api::{GlyphFormat, GlyphKey, GlyphRasterizer};

/// Free space left around each glyph.
const PADDING: i32 = 1;

/// New glyphs from this many on are uploaded together (one buffer, a copy each) rather than
/// one `write_texture` each (a staging buffer each: thousands after a zoom or a clear).
const BATCH: usize = 16;

/// A glyph rasterized and placed, its pixels not yet on the GPU.
#[derive(Debug)]
struct Upload {
    color: bool,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Where a glyph is in the atlas and how it is positioned relative to the baseline.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AtlasEntry {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub color: bool,
}

/// The atlas is full; the caller must clear it and redo the frame.
#[derive(Debug)]
pub(crate) struct AtlasFull;

struct AtlasTexture {
    texture: wgpu::Texture,
    allocator: AtlasAllocator,
    bytes_per_pixel: u32,
}

impl std::fmt::Debug for AtlasTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtlasTexture")
            .field("texture", &self.texture)
            .field("allocated_space", &self.allocator.allocated_space())
            .finish_non_exhaustive()
    }
}

impl AtlasTexture {
    fn new(device: &wgpu::Device, size: u32, format: wgpu::TextureFormat, label: &str) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes_per_pixel = format.block_copy_size(None).unwrap_or(1);
        let side = i32::try_from(size).unwrap_or(i32::MAX);
        Self { texture, allocator: AtlasAllocator::new(size2(side, side)), bytes_per_pixel }
    }
}

#[derive(Debug)]
pub(crate) struct GlyphAtlas {
    mask: AtlasTexture,
    color: AtlasTexture,
    entries: HashMap<GlyphKey, Option<AtlasEntry>>,
    /// The mask texture's side.
    size: u32,
    /// Glyphs placed since the last [`Self::flush`] (drawn only after it).
    pending: Vec<Upload>,
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device, size: u32) -> Self {
        Self {
            size,
            mask: AtlasTexture::new(device, size, wgpu::TextureFormat::R8Unorm, "glyph-atlas-mask"),
            // Color glyphs (emoji) are rare: a quarter of the side, 1/16 of the memory.
            color: AtlasTexture::new(
                device,
                (size / 4).max(256),
                wgpu::TextureFormat::Rgba8Unorm,
                "glyph-atlas-color",
            ),
            entries: HashMap::new(),
            pending: Vec::new(),
        }
    }

    pub fn mask_view(&self) -> wgpu::TextureView {
        self.mask.texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub fn color_view(&self) -> wgpu::TextureView {
        self.color.texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The mask texture's side (the color one is a quarter of it, at least 256).
    pub fn size(&self) -> u32 {
        self.size
    }

    /// An atlas of `size` side (bigger, or smaller again), empty: the glyphs are rasterized
    /// again as they're used. The texture views change (the caller rebuilds its bind group).
    pub fn grow(&mut self, device: &wgpu::Device, size: u32) {
        *self = Self::new(device, size);
    }

    /// A smaller side its glyphs would fit in (twice their area), at least `smallest`:
    /// `None` when it has none to give back (it never grew, or it's as full as that).
    pub fn smaller_size(&self, smallest: u32) -> Option<u32> {
        let used = u64::try_from(self.mask.allocator.allocated_space()).unwrap_or(u64::MAX);
        let mut side = smallest;
        while side < self.size && u64::from(side) * u64::from(side) < used.saturating_mul(2) {
            side *= 2;
        }
        (side < self.size).then_some(side)
    }

    pub fn clear(&mut self) {
        self.mask.allocator.clear();
        self.color.allocator.clear();
        self.entries.clear();
        self.pending.clear();
    }

    /// The glyphs placed since the last flush, onto the textures before `encoder`'s draws: a
    /// few one by one, many from one buffer.
    pub fn flush(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        if pending.len() < BATCH || !self.upload_together(device, encoder, &pending) {
            self.write_each(queue, &pending);
        }
    }

    /// Each glyph with its own `write_texture`.
    fn write_each(&self, queue: &wgpu::Queue, pending: &[Upload]) {
        let layout = |u: &Upload, target: &AtlasTexture| u.width * target.bytes_per_pixel;
        {
            for u in pending {
                let target = if u.color { &self.color } else { &self.mask };
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &target.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: u.x, y: u.y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &u.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(layout(u, target)),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d { width: u.width, height: u.height, depth_or_array_layers: 1 },
                );
            }
        }
    }

    /// The glyphs from one buffer, copied by `encoder` (`false`: the buffer couldn't be
    /// written: nothing done).
    fn upload_together(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        pending: &[Upload],
    ) -> bool {
        let layout = |u: &Upload, target: &AtlasTexture| u.width * target.bytes_per_pixel;
        // Rows padded to the copy's alignment, each glyph at an aligned offset.
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let mut offsets = Vec::with_capacity(pending.len());
        let mut total = 0u64;
        for u in pending {
            let target = if u.color { &self.color } else { &self.mask };
            let row = layout(u, target).next_multiple_of(align);
            offsets.push((total, row));
            total += u64::from(row) * u64::from(u.height);
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph-uploads"),
            size: total.max(4),
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        {
            let Ok(mut bytes) = buffer.slice(..).get_mapped_range_mut() else { return false };
            for (u, &(offset, row)) in pending.iter().zip(&offsets) {
                let target = if u.color { &self.color } else { &self.mask };
                let line = layout(u, target) as usize;
                for (r, src) in u.data.chunks_exact(line).take(u.height as usize).enumerate() {
                    let at = offset as usize + r * row as usize;
                    bytes.slice(at..at + line).copy_from_slice(src);
                }
            }
        }
        buffer.unmap();
        for (u, &(offset, row)) in pending.iter().zip(&offsets) {
            let target = if u.color { &self.color } else { &self.mask };
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset,
                        bytes_per_row: Some(row),
                        rows_per_image: None,
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &target.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: u.x, y: u.y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: u.width, height: u.height, depth_or_array_layers: 1 },
            );
        }
        true
    }

    /// The glyph from the atlas, rasterized and placed on first request (on the GPU at the
    /// next [`Self::flush`]). `Ok(None)` for glyphs with no pixels (e.g. space).
    pub fn get_or_insert(
        &mut self,
        key: GlyphKey,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> Result<Option<AtlasEntry>, AtlasFull> {
        if let Some(entry) = self.entries.get(&key) {
            return Ok(*entry);
        }
        let entry = match rasterizer.rasterize(key) {
            Some(glyph) if glyph.width > 0 && glyph.height > 0 => {
                let is_color = glyph.format == GlyphFormat::Color;
                let target = if is_color { &mut self.color } else { &mut self.mask };
                let w = i32::try_from(glyph.width).map_err(|_| AtlasFull)?;
                let h = i32::try_from(glyph.height).map_err(|_| AtlasFull)?;
                let alloc = target
                    .allocator
                    .allocate(size2(w + 2 * PADDING, h + 2 * PADDING))
                    .ok_or(AtlasFull)?;
                let x = (alloc.rectangle.min.x + PADDING) as u32;
                let y = (alloc.rectangle.min.y + PADDING) as u32;
                self.pending.push(Upload {
                    color: is_color,
                    x,
                    y,
                    width: glyph.width,
                    height: glyph.height,
                    data: glyph.data,
                });
                Some(AtlasEntry {
                    x,
                    y,
                    width: glyph.width,
                    height: glyph.height,
                    left: glyph.left,
                    top: glyph.top,
                    color: is_color,
                })
            }
            _ => None,
        };
        self.entries.insert(key, entry);
        Ok(entry)
    }
}
