//! Frames made in another process (a web page's: plan 99) copied into live images'
//! textures, from any thread: the frame's GPU texture opened here as a wgpu texture (no CPU
//! copy), copied on the GPU into the image's own texture (kept, made again only when the
//! frame's size changes) — only where the frame says it changed, all of it into a texture
//! made now — and waited for: the frame's buffer goes back to its engine as soon as this
//! returns. The shared state's lock is held only to look textures up, to put new ones in
//! and to tell the image its frame is in (only if its texture is still the one written:
//! one freed meanwhile took the frame with it), never around GPU work. A frame not taken
//! leaves its texture marked as not holding a whole frame ([`Filled`]): one made for it
//! isn't drawn, and the next frame is copied whole. Why a frame wasn't taken is a
//! [`Refused`], in the log. A frame's work is submitted while no window sets its surface up
//! (`GpuContext::submitting`): wgpu refuses to set one up while another thread submits.
//!
//! A frame's texture is checked before it's read: its size and format must be the ones the
//! engine said (another process's handle is not trusted). On macOS, the few `IOSurface`s an
//! engine draws into in turn are opened once each ([`Sources`], by the surface's id) and
//! read again at each frame (their content is the frame's: it's copied every time); they go
//! with their image's texture.
//!
//! The import of a macOS `IOSurface` follows `cef::osr_texture_import` (Apache-2.0 / MIT).

use std::sync::Arc;
use std::time::{Duration, Instant};

use render_api::{CopiedFrame, FrameImporter, ImageData, ImportedFrame, PixelFrame, PixelRect};

use crate::GpuContext;
use crate::instances::{Filled, GpuImage};

/// The live images' texture format: the engine's frames' (BGRA, 8 bits a channel).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// The bytes a pixel of [`FORMAT`] takes.
const BYTES_PER_PIXEL: u32 = 4;

/// How long a frame's copy is waited for at most. A copy takes well under a millisecond on
/// its own, but a GPU busy with other work (other programs, other windows, a machine under
/// load) may run it much later; and the frame's buffer goes back to its engine only once
/// it's read (drawn into while the copy is still queued, the frame would tear). Past this,
/// the frame is given up: its image keeps what it had, its next frame copied whole. (A GPU
/// lost doesn't come back as a time out: wgpu ends on it.)
const COPY_LIMIT: Duration = Duration::from_secs(5);

/// A copy waited for longer than this is noted in the log (the GPU is behind).
const SLOW_COPY: Duration = Duration::from_millis(100);

/// How many frames' textures opened from another process are kept for one image: an
/// engine draws into two or three in turn (one shown, one drawn, one waiting).
#[cfg(target_os = "macos")]
const SOURCES_PER_IMAGE: usize = 4;

/// Whether frames from another process can be opened with `backend`: Metal's `IOSurface`s
/// on macOS, DX12's shared textures on Windows (Linux: 99e).
pub(crate) fn supported(backend: wgpu::Backend) -> bool {
    match backend {
        wgpu::Backend::Metal => cfg!(target_os = "macos"),
        wgpu::Backend::Dx12 => cfg!(windows),
        _ => false,
    }
}

/// Why a frame wasn't taken.
#[derive(Debug)]
pub(crate) enum Refused {
    /// Its image isn't live, or was released.
    NotLive,
    /// No size, or larger than a texture can be on this GPU.
    TooLarge,
    /// Its pixels are fewer than its size and rows say.
    TooFewPixels,
    /// Its handle opened nothing (not of this platform's kind, or gone).
    NoSurface,
    /// What it opened isn't of the size and format its engine said.
    NotAsTold,
    /// What it opened couldn't be made a texture here.
    NoTexture,
    /// The GPU didn't finish its copy within [`COPY_LIMIT`] (how long it was waited for).
    CopyTimedOut(Duration),
    /// The GPU refused the wait.
    Gpu(wgpu::PollError),
    /// Its image's texture was freed while the frame was copied (it took the frame with it).
    TextureFreed,
}

impl Refused {
    /// Whether it happens in the normal course of things (an image released, a texture
    /// freed as a frame came), rather than something worth a warning.
    fn expected(&self) -> bool {
        matches!(self, Self::NotLive | Self::TextureFreed)
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLive => f.write_str("not live"),
            Self::TooLarge => f.write_str("no size, or too large"),
            Self::TooFewPixels => f.write_str("too few pixels"),
            Self::NoSurface => f.write_str("its handle opened nothing"),
            Self::NotAsTold => f.write_str("not of the size and format told"),
            Self::NoTexture => f.write_str("not a texture here"),
            Self::CopyTimedOut(waited) => write!(f, "its copy not done after {waited:?}"),
            Self::Gpu(e) => write!(f, "the GPU: {e}"),
            Self::TextureFreed => f.write_str("its texture freed meanwhile"),
        }
    }
}

/// `outcome` as the importer's callers are told it (taken or not), why not in the log.
fn told(outcome: Result<CopiedFrame, Refused>) -> bool {
    match outcome {
        Ok(_) => true,
        Err(why) if why.expected() => {
            tracing::debug!(reason = %why, "a web page's frame wasn't taken");
            false
        }
        Err(why) => {
            tracing::warn!(reason = %why, "a web page's frame wasn't taken");
            false
        }
    }
}

/// The renderer's importer (one per GPU context: every window's).
#[derive(Debug)]
pub(crate) struct Importer {
    ctx: Arc<GpuContext>,
}

impl FrameImporter for Importer {
    fn adapter(&self) -> Option<render_api::GpuAdapter> {
        platform::adapter(&self.ctx.device)
    }

    fn opens_textures(&self) -> bool {
        supported(self.ctx.adapter_info.backend)
    }

    fn write(&self, image: &ImageData, frame: PixelFrame<'_>) -> bool {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.try_write(image, frame)
        })) {
            Ok(outcome) => told(outcome),
            Err(panic) => {
                self.ctx.lost.store(true, std::sync::atomic::Ordering::Release);
                tracing::error!(message = crate::panic_message(&panic), "frame write panicked");
                false
            }
        }
    }

    fn import(&self, image: &ImageData, frame: ImportedFrame<'_>) -> bool {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.try_import(image, frame)
        })) {
            Ok(outcome) => told(outcome),
            Err(panic) => {
                self.ctx.lost.store(true, std::sync::atomic::Ordering::Release);
                tracing::error!(message = crate::panic_message(&panic), "frame import panicked");
                false
            }
        }
    }
}

impl Importer {
    pub(crate) fn new(ctx: Arc<GpuContext>) -> Self {
        Self { ctx }
    }

    /// [`FrameImporter::write`], saying why a frame wasn't taken.
    pub(crate) fn try_write(
        &self,
        image: &ImageData,
        frame: PixelFrame<'_>,
    ) -> Result<CopiedFrame, Refused> {
        if self.ctx.lost.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Refused::NotLive);
        }
        let size = self.frame_size(image, frame.size)?;
        let stride = frame.stride;
        let needed = u64::from(stride) * u64::from(size.height);
        if u64::from(stride) < u64::from(size.width) * u64::from(BYTES_PER_PIXEL)
            || (frame.bgra.len() as u64) < needed
        {
            return Err(Refused::TooFewPixels);
        }
        let needed = usize::try_from(needed).map_err(|_| Refused::TooFewPixels)?;
        self.fill(image, size, frame.changed, |target, areas| {
            for area in areas.rects() {
                let at = u64::from(area.origin.y) * u64::from(stride)
                    + u64::from(area.origin.x) * u64::from(BYTES_PER_PIXEL);
                let pixels = usize::try_from(at)
                    .ok()
                    .and_then(|at| frame.bgra.get(at..needed))
                    .ok_or(Refused::TooFewPixels)?;
                // Copied out by the queue at once (the pixels can be written over after this).
                self.ctx.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: target,
                        mip_level: 0,
                        origin: area.origin,
                        aspect: wgpu::TextureAspect::All,
                    },
                    pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(area.size.height),
                    },
                    area.size,
                );
            }
            // Only the submit is held (the writes are only staged until one): a window's
            // surface never waits for the pixels to be copied.
            let _submitting = self.ctx.submitting();
            self.ctx.queue.submit([]);
            Ok(())
        })
    }

    /// [`FrameImporter::import`], saying why a frame wasn't taken.
    pub(crate) fn try_import(
        &self,
        image: &ImageData,
        frame: ImportedFrame<'_>,
    ) -> Result<CopiedFrame, Refused> {
        if self.ctx.lost.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Refused::NotLive);
        }
        let size = self.frame_size(image, frame.size)?;
        let source = platform::open(&self.ctx, image.id, frame.native, size)?;
        self.fill(image, size, frame.changed, |target, areas| {
            let mut encoder =
                self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("web-frame-copy"),
                });
            for area in areas.rects() {
                let at = |texture| wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: area.origin,
                    aspect: wgpu::TextureAspect::All,
                };
                encoder.copy_texture_to_texture(at(&source.texture), at(target), area.size);
            }
            // Only the submit is held: a surface set up meanwhile waits for the copy itself.
            let submitted = {
                let _submitting = self.ctx.submitting();
                self.ctx.queue.submit([encoder.finish()])
            };
            self.wait(submitted)
        })
    }

    /// `size` as a texture's, if `image` takes frames and one of that size fits this GPU.
    fn frame_size(
        &self,
        image: &ImageData,
        (width, height): (u32, u32),
    ) -> Result<wgpu::Extent3d, Refused> {
        if !image.is_live() || image.released() {
            return Err(Refused::NotLive);
        }
        let max = self.ctx.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > max || height > max {
            return Err(Refused::TooLarge);
        }
        Ok(wgpu::Extent3d { width, height, depth_or_array_layers: 1 })
    }

    /// The GPU waited for until `submission` is done, up to [`COPY_LIMIT`].
    fn wait(&self, submission: wgpu::SubmissionIndex) -> Result<(), Refused> {
        let started = Instant::now();
        let wait =
            wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(COPY_LIMIT) };
        let waited = self.ctx.device.poll(wait);
        let elapsed = started.elapsed();
        match waited {
            Ok(_) => {
                if elapsed > SLOW_COPY {
                    tracing::debug!(?elapsed, "a web page's frame copied late");
                }
                Ok(())
            }
            Err(wgpu::PollError::Timeout) => Err(Refused::CopyTimedOut(elapsed)),
            Err(e) => Err(Refused::Gpu(e)),
        }
    }

    /// A frame of `size` put in `image`'s texture by `copy` (into the texture, the areas
    /// [`Areas::of`] says), the image told. A copy that fails leaves the texture as not
    /// holding a frame ([`Self::spoiled`]): the next one is copied whole.
    pub(crate) fn fill(
        &self,
        image: &ImageData,
        size: wgpu::Extent3d,
        changed: &[PixelRect],
        copy: impl FnOnce(&wgpu::Texture, &Areas) -> Result<(), Refused>,
    ) -> Result<CopiedFrame, Refused> {
        let (target, made) = self.target(image, size);
        let areas = Areas::of(changed, size, made);
        if let Err(why) = copy(&target, &areas) {
            self.spoiled(image, &target);
            return Err(why);
        }
        self.copied(image, &target, areas.copied(size))
    }

    /// `image` told its frame is in `target`, if `target` is still its texture: one freed
    /// meanwhile (the image unshown a while, a popup let go) took the frame with it, and the
    /// image stays without one ([`Refused::TextureFreed`]: the next frame is copied whole
    /// into a new texture). Looked at and told under the state's lock, so the texture can't
    /// go between.
    pub(crate) fn copied(
        &self,
        image: &ImageData,
        target: &wgpu::Texture,
        frame: CopiedFrame,
    ) -> Result<CopiedFrame, Refused> {
        let mut state = self.ctx.state();
        let Some(gpu) =
            state.textures.get_mut(&image.id).filter(|g| g.live.as_ref() == Some(target))
        else {
            return Err(Refused::TextureFreed);
        };
        gpu.filled = Filled::Whole;
        image.frame_copied(frame);
        Ok(frame)
    }

    /// A copy into `target` failed: if it's still `image`'s texture, what it holds is no
    /// longer a whole frame the next can be copied over in parts (one made now stays empty:
    /// not drawn).
    fn spoiled(&self, image: &ImageData, target: &wgpu::Texture) {
        let mut state = self.ctx.state();
        if let Some(gpu) = state.textures.get_mut(&image.id)
            && gpu.live.as_ref() == Some(target)
            && gpu.filled == Filled::Whole
        {
            gpu.filled = Filled::Torn;
        }
    }

    /// `image`'s texture at `size`: the one it has, or a new one put in its place; `true`:
    /// to be copied whole (made now, or what it holds isn't a whole frame).
    pub(crate) fn target(&self, image: &ImageData, size: wgpu::Extent3d) -> (wgpu::Texture, bool) {
        let kept = self.ctx.state().textures.get(&image.id).and_then(|gpu| {
            let texture = gpu.live.as_ref().filter(|t| t.size() == size)?;
            Some((texture.clone(), gpu.filled != Filled::Whole))
        });
        if let Some(kept) = kept {
            return kept;
        }
        let device = &self.ctx.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("web-page"),
            // Copied from too: a page's last frame kept after it goes (`ImageData::copy_of`).
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            ..frame_descriptor(size)
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("web-page"),
            layout: &self.ctx.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.ctx.image_sampler),
                },
            ],
        });
        let mut state = self.ctx.state();
        let frame = state.frame;
        state.textures.insert(
            image.id,
            GpuImage {
                bind_group,
                bytes: texture_bytes(size),
                last_frame: frame,
                last_used: Instant::now(),
                image: image.clone(),
                live: Some(texture.clone()),
                filled: Filled::Empty,
            },
        );
        (texture, true)
    }
}

/// The bytes a frame's texture of `size` takes.
fn texture_bytes(size: wgpu::Extent3d) -> u64 {
    u64::from(size.width) * u64::from(size.height) * u64::from(BYTES_PER_PIXEL)
}

/// A frame's texture of `size` as it's described (the usage aside: the caller's).
fn frame_descriptor(size: wgpu::Extent3d) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("web-frame"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    }
}

/// A region of a frame to copy: where it starts, how large.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Area {
    pub(crate) origin: wgpu::Origin3d,
    pub(crate) size: wgpu::Extent3d,
}

/// The parts of a frame copied: where it changed (within it), or all of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Areas {
    /// Each region copied (all of the frame: one).
    parts: Vec<Area>,
    whole: bool,
}

impl Areas {
    /// What of a frame of `size` is copied: all of it into a texture `made` now, or when it
    /// says nothing (or nothing within it); else where it changed, cut to it.
    pub(crate) fn of(changed: &[PixelRect], size: wgpu::Extent3d, made: bool) -> Self {
        let whole =
            || Self { parts: vec![Area { origin: wgpu::Origin3d::ZERO, size }], whole: true };
        if made {
            return whole();
        }
        let frame = PixelRect::new(
            0,
            0,
            i32::try_from(size.width).unwrap_or(i32::MAX),
            i32::try_from(size.height).unwrap_or(i32::MAX),
        );
        let parts: Vec<Area> = changed
            .iter()
            .filter_map(|r| r.intersect(&frame))
            .map(|r| Area {
                // Within the frame: never below 0.
                origin: wgpu::Origin3d { x: r.x0.unsigned_abs(), y: r.y0.unsigned_abs(), z: 0 },
                size: wgpu::Extent3d {
                    width: r.width(),
                    height: r.height(),
                    depth_or_array_layers: 1,
                },
            })
            .collect();
        if parts.is_empty() { whole() } else { Self { parts, whole: false } }
    }

    /// Each region copied.
    pub(crate) fn rects(&self) -> &[Area] {
        &self.parts
    }

    /// The frame copied, as its image is told (where it changed: the regions' bounds).
    pub(crate) fn copied(&self, size: wgpu::Extent3d) -> CopiedFrame {
        let changed = if self.whole {
            None
        } else {
            self.parts
                .iter()
                .map(|a| {
                    let at = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
                    let (x, y) = (at(a.origin.x), at(a.origin.y));
                    PixelRect::new(
                        x,
                        y,
                        x.saturating_add(at(a.size.width)),
                        y.saturating_add(at(a.size.height)),
                    )
                })
                .reduce(|a, b| a.union(&b))
        };
        CopiedFrame { size: (size.width, size.height), changed, bytes: texture_bytes(size) }
    }
}

/// A frame's texture opened here, and what keeps it valid while it's read.
#[derive(Clone)]
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) struct Source {
    texture: wgpu::Texture,
    /// The system's object behind it (released after the texture).
    _keep: platform::Keep,
}

/// The frames' textures opened from another process, kept to be read again: by the image
/// they're copied into, the system's id of each (a few per image, the least recently read
/// let go first), gone with the image's texture. (Of a kind of its own for its tests.)
#[cfg(target_os = "macos")]
pub(crate) struct Sources<S = Source> {
    /// In the order they were last read (the latest at the end).
    kept: Vec<KeptSource<S>>,
}

#[cfg(target_os = "macos")]
impl<S> Default for Sources<S> {
    fn default() -> Self {
        Self { kept: Vec::new() }
    }
}

#[cfg(target_os = "macos")]
struct KeptSource<S> {
    image: u64,
    /// The system's id of the frame's texture (an `IOSurface`'s).
    id: u64,
    size: wgpu::Extent3d,
    source: S,
}

#[cfg(target_os = "macos")]
impl<S: Clone> Sources<S> {
    /// The source `id` of `image`, of `size`, if it's kept (now the latest read). One of
    /// another size under the same id is let go: the surface it was made from (held with
    /// it) changed under it, a page's engine giving its id to a surface of the new size.
    fn get(&mut self, image: u64, id: u64, size: wgpu::Extent3d) -> Option<S> {
        let at = self.kept.iter().position(|k| k.image == image && k.id == id)?;
        let kept = self.kept.remove(at);
        if kept.size != size {
            return None;
        }
        let source = kept.source.clone();
        self.kept.push(kept);
        Some(source)
    }

    /// `source` kept as `image`'s `id` (its oldest let go past [`SOURCES_PER_IMAGE`]); the
    /// image's sources of another size go (the page resized: its engine draws into new
    /// surfaces, the old ones would only hold memory — 33 MB each at 4K).
    fn put(&mut self, image: u64, id: u64, size: wgpu::Extent3d, source: S) {
        self.kept.retain(|k| k.image != image || (k.id != id && k.size == size));
        let of_image = self.kept.iter().filter(|k| k.image == image).count();
        if of_image >= SOURCES_PER_IMAGE
            && let Some(oldest) = self.kept.iter().position(|k| k.image == image)
        {
            self.kept.remove(oldest);
        }
        self.kept.push(KeptSource { image, id, size, source });
    }

    /// Only the sources of the images `alive` says kept.
    pub(crate) fn keep_for(&mut self, alive: impl Fn(u64) -> bool) {
        if !self.kept.is_empty() {
            self.kept.retain(|k| alive(k.image));
        }
    }

    /// How many are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.kept.len()
    }
}

/// Nothing kept: a frame's texture is opened for that frame only (a Windows handle names
/// it for one frame; elsewhere none is opened yet).
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub(crate) struct Sources {}

#[cfg(not(target_os = "macos"))]
impl Sources {
    /// Nothing to let go.
    pub(crate) fn keep_for(&mut self, _alive: impl Fn(u64) -> bool) {}
}

#[cfg(target_os = "macos")]
mod platform {
    #![allow(unsafe_code)]

    use objc2_core_foundation::CFRetained;
    use objc2_io_surface::IOSurfaceRef;
    use objc2_metal::{
        MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
        MTLTextureUsage,
    };
    use render_api::NativeFrame;

    use super::{BYTES_PER_PIXEL, FORMAT, Refused, Source, frame_descriptor};
    use crate::GpuContext;

    /// The four characters an `IOSurface` of BGRA pixels names its format with
    /// (`kCVPixelFormatType_32BGRA`).
    pub(super) const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

    #[link(name = "IOSurface", kind = "framework")]
    unsafe extern "C" {
        /// The surface a send right names (+1: the caller releases it), or null.
        fn IOSurfaceLookupFromMachPort(port: u32) -> *mut IOSurfaceRef;
    }

    /// The surface, held while its texture is read.
    #[derive(Clone)]
    pub(super) struct Keep(
        #[expect(dead_code, reason = "held for its drop")] CFRetained<IOSurfaceRef>,
    );

    // SAFETY: an `IOSurface` is made to be shared across threads and processes (its
    // retain count is atomic); it's only held here, its properties read, never written.
    unsafe impl Send for Keep {}
    // SAFETY: as above: nothing of it changes through a shared reference.
    unsafe impl Sync for Keep {}

    /// Any GPU: an `IOSurface` is shared across a Mac's GPUs by the system.
    pub(super) fn adapter(_device: &wgpu::Device) -> Option<render_api::GpuAdapter> {
        None
    }

    /// Whether `surface` holds a frame of `size` in BGRA, one plane: what the engine said.
    pub(super) fn matches(surface: &IOSurfaceRef, size: wgpu::Extent3d) -> bool {
        surface.width() == size.width as usize
            && surface.height() == size.height as usize
            && surface.pixel_format() == BGRA
            && surface.bytes_per_element() == BYTES_PER_PIXEL as usize
            && surface.plane_count() == 0
    }

    /// The frame's `IOSurface` as a wgpu texture (Metal only), opened once per surface and
    /// kept for `image`'s next frames; refused if it isn't a BGRA surface of `size`.
    pub(super) fn open(
        ctx: &GpuContext,
        image: u64,
        native: NativeFrame,
        size: wgpu::Extent3d,
    ) -> Result<Source, Refused> {
        let NativeFrame::MacSurfacePort(port) = native else { return Err(Refused::NoSurface) };
        // SAFETY: a send right we hold for the call; the surface comes retained (+1).
        let surface = std::ptr::NonNull::new(unsafe { IOSurfaceLookupFromMachPort(port) })
            .ok_or(Refused::NoSurface)?;
        // SAFETY: the +1 reference is taken over (released with `surface`).
        let surface = unsafe { CFRetained::from_raw(surface) };
        if !matches(&surface, size) {
            return Err(Refused::NotAsTold);
        }
        let id = u64::from(surface.id());
        if let Some(kept) = ctx.state().sources.get(image, id, size) {
            return Ok(kept);
        }
        let source = texture(&ctx.device, surface, size).ok_or(Refused::NoTexture)?;
        ctx.state().sources.put(image, id, size, source.clone());
        Ok(source)
    }

    /// `surface` (checked: BGRA, `size`) as a wgpu texture to copy from.
    fn texture(
        device: &wgpu::Device,
        surface: CFRetained<IOSurfaceRef>,
        size: wgpu::Extent3d,
    ) -> Option<Source> {
        let descriptor = MTLTextureDescriptor::new();
        // SAFETY: a plain 2D texture's description, the surface's size and format.
        unsafe {
            descriptor.setWidth(size.width as usize);
            descriptor.setHeight(size.height as usize);
            descriptor.setTextureType(MTLTextureType::Type2D);
            descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            descriptor.setUsage(MTLTextureUsage::ShaderRead);
            descriptor.setStorageMode(MTLStorageMode::Managed);
        }
        // SAFETY: the device is wgpu's Metal device; the texture made from the surface is
        // read only while `surface` is held (kept with it).
        let hal = unsafe {
            let device_guard = device.as_hal::<wgpu::wgc::api::Metal>()?;
            let raw = device_guard.raw_device().newTextureWithDescriptor_iosurface_plane(
                &descriptor,
                &surface,
                0,
            )?;
            <wgpu::wgc::api::Metal as wgpu::hal::Api>::Device::texture_from_raw(
                raw,
                FORMAT,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent { width: size.width, height: size.height, depth: 1 },
                None,
            )
        };
        // SAFETY: the hal texture is the Metal device's, described as made.
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::wgc::api::Metal>(
                hal,
                &frame_descriptor(size),
                wgpu::TextureUses::RESOURCE,
            )
        };
        Some(Source { texture, _keep: Keep(surface) })
    }
}

#[cfg(windows)]
mod platform {
    #![allow(unsafe_code)]

    use render_api::NativeFrame;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_TEXTURE_LAYOUT_UNKNOWN,
        ID3D12Resource,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    use super::{FORMAT, Refused, Source, frame_descriptor};
    use crate::GpuContext;

    /// Nothing held: the resource opened from the handle lives with the wgpu texture (the
    /// handle itself is the caller's, closed once the frame is copied). Not kept for the
    /// next frames: a handle is the caller's for one frame (its value given again to
    /// another object once closed), so it can't name a texture across frames.
    pub(super) type Keep = ();

    /// The adapter wgpu's DX12 device is on: a shared texture opens only on the adapter it
    /// was made on, so the engine is asked to draw there.
    pub(super) fn adapter(device: &wgpu::Device) -> Option<render_api::GpuAdapter> {
        // SAFETY: wgpu's DX12 device, asked only its adapter's LUID.
        let luid =
            unsafe { device.as_hal::<wgpu::wgc::api::Dx12>()?.raw_device().GetAdapterLuid() };
        Some(render_api::GpuAdapter { high: luid.HighPart, low: luid.LowPart })
    }

    /// Whether a resource described by `desc` is a frame of `size` in BGRA: what the engine
    /// said, and what wgpu's texture is described as (one sample, the driver's own layout).
    pub(super) fn matches(desc: &D3D12_RESOURCE_DESC, size: wgpu::Extent3d) -> bool {
        desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
            && desc.Width == u64::from(size.width)
            && desc.Height == size.height
            && desc.DepthOrArraySize == 1
            && desc.MipLevels == 1
            && desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM
            && desc.SampleDesc.Count == 1
            && desc.Layout == D3D12_TEXTURE_LAYOUT_UNKNOWN
    }

    /// The frame's D3D11 shared texture (its NT handle, in this process) opened on wgpu's
    /// DX12 device (DX12 only); refused if it isn't a BGRA texture of `size`.
    pub(super) fn open(
        ctx: &GpuContext,
        _image: u64,
        native: NativeFrame,
        size: wgpu::Extent3d,
    ) -> Result<Source, Refused> {
        let NativeFrame::WindowsHandle(handle) = native else { return Err(Refused::NoSurface) };
        let device = &ctx.device;
        // SAFETY: wgpu's DX12 device; the handle is a shared texture's, valid while the
        // caller holds it (this call); the resource opened is ours (released with the
        // texture made from it, or here if it isn't the frame told).
        let hal = unsafe {
            let device_guard = device.as_hal::<wgpu::wgc::api::Dx12>().ok_or(Refused::NoTexture)?;
            let mut resource: Option<ID3D12Resource> = None;
            let shared = HANDLE(handle as usize as *mut std::ffi::c_void);
            device_guard
                .raw_device()
                .OpenSharedHandle(shared, &raw mut resource)
                .map_err(|_| Refused::NoSurface)?;
            let resource = resource.ok_or(Refused::NoSurface)?;
            if !matches(&resource.GetDesc(), size) {
                return Err(Refused::NotAsTold);
            }
            <wgpu::wgc::api::Dx12 as wgpu::hal::Api>::Device::texture_from_raw(
                resource,
                FORMAT,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            )
        };
        // SAFETY: the hal texture is the DX12 device's, described as made. A shared
        // texture opened is in the common state, which D3D12 promotes to a copy's source by
        // itself: said already a copy's source, wgpu adds no barrier.
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::wgc::api::Dx12>(
                hal,
                &frame_descriptor(size),
                wgpu::TextureUses::COPY_SRC,
            )
        };
        Ok(Source { texture, _keep: () })
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use render_api::NativeFrame;

    use super::{Refused, Source};
    use crate::GpuContext;

    /// Nothing held (no import here yet: Linux in plan 99's step 99e).
    pub(super) type Keep = ();

    pub(super) fn adapter(_device: &wgpu::Device) -> Option<render_api::GpuAdapter> {
        None
    }

    pub(super) fn open(
        _ctx: &GpuContext,
        _image: u64,
        _native: NativeFrame,
        _size: wgpu::Extent3d,
    ) -> Result<Source, Refused> {
        Err(Refused::NoSurface)
    }
}

#[cfg(test)]
#[path = "../tests/unit/import.rs"]
mod tests;
