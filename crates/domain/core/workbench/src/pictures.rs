//! The windows' pictures: a small image of each window's last frame, taken by the app when
//! the window loses the focus or a module asks ([`crate::SystemHost::capture_window`]), kept
//! here — one per window, a few hundred KB — until its project closes. Any module reads
//! them (the open projects' list shows them on its cards); one taken later has a higher
//! [`WindowPicture::taken`], so a view that drew one knows when it changed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// A window's picture: straight RGBA8, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowPicture {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    /// When it was taken, among all pictures (higher: later).
    pub taken: u64,
}

static KEPT: Mutex<Vec<(u64, WindowPicture)>> = Mutex::new(Vec::new());
static TAKEN: AtomicU64 = AtomicU64::new(1);

fn kept() -> std::sync::MutexGuard<'static, Vec<(u64, WindowPicture)>> {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Window `window`'s new picture (the one before it let go), from any thread; the windows
/// look again.
pub fn keep(window: u64, width: u32, height: u32, rgba: Vec<u8>) {
    let taken = TAKEN.fetch_add(1, Ordering::Relaxed);
    let picture = WindowPicture { width, height, rgba: rgba.into(), taken };
    {
        let mut kept = kept();
        match kept.iter_mut().find(|(w, _)| *w == window) {
            Some((_, old)) => *old = picture,
            None => kept.push((window, picture)),
        }
    }
    crate::app_wide::wake();
}

/// Window `window`'s picture, if one was taken (cheap: its pixels are shared).
pub fn of(window: u64) -> Option<WindowPicture> {
    kept().iter().find(|(w, _)| *w == window).map(|(_, p)| p.clone())
}

/// Only the pictures of `windows` kept (the others' projects closed: their memory goes).
pub fn keep_only(windows: &[u64]) {
    kept().retain(|(w, _)| windows.contains(w));
}

#[cfg(test)]
#[path = "../tests/unit/pictures.rs"]
mod tests;
