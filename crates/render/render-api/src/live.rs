//! Textures filled from outside the app's drawing: a web page's frames, drawn by the web
//! engine in its own process and handed over as a GPU handle ([`NativeFrame`]); the
//! renderer copies each one on the GPU into the [live](crate::ImageData::live) image's
//! texture ([`FrameImporter`]), which the scene then draws like any image.
//!
//! A frame says where it changed ([`ImportedFrame::changed`]): only that is copied, and
//! only that is drawn again where the image shows (its recent frames' areas are kept with
//! the image, [`FrameHistory`], so the damage of a window that skipped some still covers
//! them all).

use std::sync::Mutex;

use crate::PixelRect;

/// A frame's GPU texture as the system passes it between processes (still owned by its
/// caller: the importer only reads it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFrame {
    /// macOS: a send right to the frame's `IOSurface`.
    MacSurfacePort(u32),
    /// Windows: a D3D11 shared texture's NT handle, in this process.
    WindowsHandle(u64),
}

/// A frame given as a GPU texture ([`FrameImporter::import`]).
#[derive(Debug, Clone, Copy)]
pub struct ImportedFrame<'a> {
    pub native: NativeFrame,
    /// Its size (pixels): what the texture must be.
    pub size: (u32, u32),
    /// Where it changed from the frame before, in its pixels (empty: all of it).
    pub changed: &'a [PixelRect],
}

/// A frame given as pixels ([`FrameImporter::write`]).
#[derive(Debug, Clone, Copy)]
pub struct PixelFrame<'a> {
    /// BGRA rows of `stride` bytes, `size.1` of them (premultiplied, as the engine draws).
    pub bgra: &'a [u8],
    /// Its size (pixels).
    pub size: (u32, u32),
    /// The bytes from a row to the next (at least `size.0 * 4`).
    pub stride: u32,
    /// Where it changed from the frame before, in its pixels (empty: all of it).
    pub changed: &'a [PixelRect],
}

/// Copies frames into live images' textures, from any thread (the frame's buffer goes
/// back to its engine once this returns: the copy is done on the GPU by then).
pub trait FrameImporter: Send + Sync + std::fmt::Debug {
    /// `frame`'s texture copied into `image`'s (made or resized to the frame's size: then
    /// copied whole; else only where it changed). `false`: it couldn't be (another GPU API,
    /// a handle not valid, a texture not of the size or format said).
    fn import(&self, image: &crate::ImageData, frame: ImportedFrame<'_>) -> bool;

    /// Whether it opens the system's shared textures ([`Self::import`]) with this GPU and
    /// graphics API; else frames come as pixels ([`Self::write`]).
    fn opens_textures(&self) -> bool {
        true
    }

    /// A frame given as pixels written into `image`'s texture (only where it changed, but
    /// for a texture made now). Any GPU, any graphics API: one copy through the CPU.
    /// `false`: it couldn't be.
    fn write(&self, image: &crate::ImageData, frame: PixelFrame<'_>) -> bool {
        let _ = (image, frame);
        false
    }

    /// The GPU its frames must be made on to be opened (`None`: any — one GPU, or the
    /// system shares them across).
    fn adapter(&self) -> Option<GpuAdapter> {
        None
    }
}

/// A GPU as the system names it between processes: Windows' adapter LUID, its two halves
/// as DXGI gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuAdapter {
    /// The LUID's high half (`LUID::HighPart`).
    pub high: i32,
    /// The LUID's low half (`LUID::LowPart`).
    pub low: u32,
}

/// A frame put in a live image's texture, as the importer tells it
/// ([`crate::ImageData::frame_copied`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopiedFrame {
    /// The frame's size (pixels): its texture's.
    pub size: (u32, u32),
    /// Where it changed, in its pixels (`None`: all of it — a texture made anew).
    pub changed: Option<PixelRect>,
    /// The bytes its texture takes on the GPU.
    pub bytes: u64,
}

/// Where a live image changed between two of its frames: an area of its frames' pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameChange {
    /// The frames' size (pixels).
    pub size: (u32, u32),
    pub area: PixelRect,
}

/// How many of a live image's last frames are remembered: a window drawing less often than
/// the page (a frame every few of the page's) still finds what changed; past them, all of
/// the image is drawn again.
const HISTORY: usize = 8;

/// A frame of a live image as remembered: its number, its size, where it changed (`None`:
/// all of it).
#[derive(Debug, Clone, Copy, Default)]
struct Remembered {
    generation: u64,
    size: (u32, u32),
    changed: Option<PixelRect>,
}

/// A live image's last frames' areas, by their number (`generation`).
#[derive(Debug, Default)]
pub(crate) struct FrameHistory {
    /// Slot `generation % HISTORY`: that frame (if its number is the one looked for).
    frames: Mutex<[Remembered; HISTORY]>,
}

impl FrameHistory {
    /// Frame `generation` was `copied`.
    pub(crate) fn record(&self, generation: u64, copied: &CopiedFrame) {
        let Ok(mut frames) = self.frames.lock() else { return };
        frames[slot(generation)] =
            Remembered { generation, size: copied.size, changed: copied.changed };
    }

    /// Where the frames after `from` up to `to` changed it (`None`: all of it — one of them
    /// forgotten, whole, or of another size).
    pub(crate) fn changed_between(&self, from: u64, to: u64) -> Option<FrameChange> {
        if to <= from || to - from > HISTORY as u64 {
            return None;
        }
        let frames = self.frames.lock().ok()?;
        let mut change: Option<FrameChange> = None;
        for generation in from + 1..=to {
            let frame = frames[slot(generation)];
            let area = frame.changed.filter(|_| frame.generation == generation)?;
            let size = frame.size;
            change = Some(match change {
                None => FrameChange { size, area },
                Some(c) if c.size == size => FrameChange { size, area: c.area.union(&area) },
                Some(_) => return None,
            });
        }
        change
    }
}

/// Where frame `generation` is remembered.
fn slot(generation: u64) -> usize {
    (generation % HISTORY as u64) as usize
}
