//! A frame's GPU texture handed to its app, by the system's own way between processes: on
//! Windows its handle duplicated into the app's process, on macOS its `IOSurface`'s Mach
//! port sent to the app's Mach service, on Linux its dmabuf's descriptors sent beside the
//! message. One [`Transfer`] per app, made from its greeting.

use cef::AcceleratedPaintInfo;
use web_engine_protocol::{Hello, PageId, Texture};
use web_host_core::Undelivered;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
use linux as platform;

/// How an app gets its frames.
pub struct Transfer {
    inner: platform::Transfer,
}

impl std::fmt::Debug for Transfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transfer").finish_non_exhaustive()
    }
}

/// A frame's texture handed over: what its message says of it, its size in pixels, and
/// what's taken back if its message never reaches the app (`None`: nothing is).
#[derive(Debug)]
pub struct Handed {
    pub texture: Texture,
    pub width: u32,
    pub height: u32,
    pub undelivered: Option<Undelivered>,
}

impl Transfer {
    /// For the app that said `hello`.
    pub fn to(hello: &Hello) -> Self {
        Self { inner: platform::Transfer::to(hello) }
    }

    /// Frame `number`'s texture handed over (`None`: it couldn't be — the frame is skipped,
    /// the next one shows the page).
    pub fn hand_over(
        &self,
        info: &AcceleratedPaintInfo,
        page: PageId,
        number: u64,
    ) -> Option<Handed> {
        let (width, height) = size_of(info);
        let (texture, undelivered) = self.inner.hand_over(info, page, number)?;
        Some(Handed { texture, width, height, undelivered })
    }
}

/// The frame's size in pixels.
fn size_of(info: &AcceleratedPaintInfo) -> (u32, u32) {
    let rect = &info.extra.coded_size;
    (u32::try_from(rect.width).unwrap_or(0), u32::try_from(rect.height).unwrap_or(0))
}
