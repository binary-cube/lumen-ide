//! A page's frames as pixels (where its app asked for them instead of GPU textures): each
//! frame written into a file of the channel's folder, which the app reads whole. Only the
//! rows that changed are written once the file holds a frame of the same size (it keeps the
//! rest of the previous one); a first frame, or one of another size, is written whole.
//!
//! The files are the page's while it's open: removed when it closes, or when it couldn't be
//! made — whoever holds them, nothing is left behind.

use std::fs::File;
use std::path::{Path, PathBuf};

/// Bytes a BGRA pixel takes.
const BYTES_PER_PIXEL: u64 = 4;

/// A page's pixels' files: its view's, and its popup's (an open `<select>`'s list) beside it.
#[derive(Debug)]
pub struct PixelFiles {
    pub view: PixelsFile,
    pub popup: Option<PixelsFile>,
}

impl PixelFiles {
    /// The files made at `path` (and its popup's beside it), only in the channel's folder
    /// `dir` and named as the app names them (`None`: anything else, or the view's can't be
    /// made — the page isn't opened).
    pub fn open(dir: &Path, path: &Path) -> Option<Self> {
        let view = PixelsFile::open(dir, path)?;
        let popup = PixelsFile::open(dir, &web_engine_protocol::popup_pixels(path));
        Some(Self { view, popup })
    }

    /// The view's file, or the popup's.
    pub fn of(&mut self, popup: bool) -> Option<&mut PixelsFile> {
        match popup {
            true => self.popup.as_mut(),
            false => Some(&mut self.view),
        }
    }
}

/// One frame's pixels as CEF paints them.
#[derive(Debug, Clone, Copy)]
pub struct Painted<'a> {
    /// BGRA, `width` × `height`, rows one after the other.
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// A file a page's frames are written into (removed as this drops).
#[derive(Debug)]
pub struct PixelsFile {
    file: File,
    path: PathBuf,
    /// The size of the frame it holds (`None`: none yet).
    holds: Option<(u32, u32)>,
    /// The bands of rows to write, kept between frames (no list made per frame).
    bands: Vec<(u32, u32)>,
}

impl PixelsFile {
    /// The file made at `path`, only in the channel's folder `dir` and named as the app names
    /// them (`frames-*.bgra`): anything else is refused. Only its owner reads it.
    pub fn open(dir: &Path, path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let ours = path.parent() == Some(dir) && web_engine_protocol::is_pixels_name(name);
        if !ours {
            tracing::warn!("a page's pixels asked outside the channel's folder: refused");
            return None;
        }
        // Made new (the app names each page's anew): a file or a link already at its place
        // is never written through nor taken over (`create_new` fails on a link, even one
        // leading nowhere).
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = match options.open(path) {
            Ok(file) => file,
            Err(e) => {
                tracing::warn!(error = %e, "a page's pixels' file wasn't made");
                return None;
            }
        };
        Some(Self { file, path: path.to_path_buf(), holds: None, bands: Vec::new() })
    }

    /// `frame` written: only the rows of `dirty` (`(top, rows)` each, as CEF tells what
    /// changed) when the file holds a frame of its size, else whole. `false`: not written.
    pub fn write(&mut self, frame: Painted<'_>, dirty: impl Iterator<Item = (i32, i32)>) -> bool {
        let stride = u64::from(frame.width) * BYTES_PER_PIXEL;
        let length = stride.saturating_mul(u64::from(frame.height));
        if usize::try_from(length).ok() != Some(frame.pixels.len()) {
            return false;
        }
        let size = (frame.width, frame.height);
        if self.holds != Some(size) {
            self.holds = None;
            let whole = self.write_at(0, frame.pixels);
            self.holds = whole.then_some(size);
            return whole;
        }
        bands(dirty, frame.height, &mut self.bands);
        let mut written = true;
        for index in 0..self.bands.len() {
            let (top, rows) = self.bands[index];
            let at = u64::from(top) * stride;
            let end = at + u64::from(rows) * stride;
            let part = usize::try_from(at)
                .ok()
                .zip(usize::try_from(end).ok())
                .and_then(|(start, end)| frame.pixels.get(start..end));
            let Some(part) = part else {
                written = false;
                break;
            };
            written &= self.write_at(at, part);
        }
        if !written {
            // What the file holds is unknown now: the next frame is written whole.
            self.holds = None;
        }
        written
    }

    /// What the file holds no longer known (a frame of it not written): the next is written
    /// whole.
    pub fn forget(&mut self) {
        self.holds = None;
    }

    /// `bytes` written at `at` (one call: no seek, then a write).
    fn write_at(&self, at: u64, bytes: &[u8]) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt as _;
            self.file.write_all_at(bytes, at).is_ok()
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt as _;
            let (mut at, mut rest) = (at, bytes);
            while !rest.is_empty() {
                match self.file.seek_write(rest, at) {
                    Ok(0) => return false,
                    Ok(n) => {
                        rest = rest.get(n..).unwrap_or_default();
                        at = at.saturating_add(n as u64);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return false,
                }
            }
            true
        }
    }
}

impl Drop for PixelsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The bands of rows the dirty rectangles cover (`(top, rows)` each, within `height`), sorted
/// and merged where they touch or overlap, into `out`.
fn bands(dirty: impl Iterator<Item = (i32, i32)>, height: u32, out: &mut Vec<(u32, u32)>) {
    out.clear();
    let height = i64::from(height);
    for (top, rows) in dirty {
        let start = i64::from(top).clamp(0, height);
        let end = (i64::from(top) + i64::from(rows.max(0))).clamp(0, height);
        if let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end))
            && end > start
        {
            out.push((start, end - start));
        }
    }
    out.sort_unstable();
    let mut merged = 0;
    for index in 0..out.len() {
        let (top, rows) = out[index];
        if merged > 0 {
            let (last_top, last_rows) = out[merged - 1];
            if top <= last_top + last_rows {
                out[merged - 1].1 = (top + rows).max(last_top + last_rows) - last_top;
                continue;
            }
        }
        out[merged] = (top, rows);
        merged += 1;
    }
    out.truncate(merged);
}

#[cfg(test)]
#[path = "../tests/unit/pixels.rs"]
mod tests;
