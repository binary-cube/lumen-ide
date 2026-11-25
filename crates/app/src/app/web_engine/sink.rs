//! Where a page's engine events go, on the connection's reader thread: its frames copied
//! into its image there (a GPU texture imported, or pixels read from its file), the rest
//! queued for the UI thread ([`Told`]), the window woken for both.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use platform_api::WebEvent;
#[cfg(any(target_os = "macos", windows))]
use render_api::NativeFrame;
use render_api::{FrameImporter, ImageData};
use web_engine::Native;
use web_engine::protocol::{Event, PopupRect};
use web_engine::{ArrivedFrame, PageEvent, Sink};

use super::access::cursor_of;
use super::{IMPORTS_FAILED, Wake};

/// What a page told, from the reader thread to the UI thread.
pub(super) enum Told {
    Event(WebEvent),
    /// Its engine went (crashed, ended): the page is made again by its owner.
    Gone,
    /// The pointer it asks for.
    Cursor(platform_api::CursorIcon),
    /// Its frames can't be opened here (see [`IMPORTS_FAILED`]): as pixels from now on.
    AsPixels,
    /// Its popup shown where it is (CSS pixels in the page), or gone.
    Popup(Option<PopupRect>),
    /// Its menu asked for (the app shows its own).
    Menu(web_engine::protocol::PageMenu),
    /// Where its text being composed is (CSS pixels).
    Caret(PopupRect),
    /// Its accessibility tree, whole.
    Access(Vec<web_engine::protocol::AccessNode>),
}

/// What a page's sink is made with ([`sink`]).
pub(super) struct SinkOf {
    /// The page's image, its frames copied into it.
    pub image: ImageData,
    /// Its popup's image.
    pub popup_image: ImageData,
    pub importer: Arc<dyn FrameImporter>,
    /// Where what it tells goes, for the UI thread.
    pub told: Sender<Told>,
    pub wake: Wake,
    /// Its frames' file when they come as pixels (`None`: as GPU textures).
    pub pixels: Option<PathBuf>,
}

/// Where a page's events go, on the reader thread: frames copied into its image there, the
/// rest queued for the UI thread; the window woken for both.
pub(super) fn sink(of: SinkOf) -> Sink {
    let SinkOf { image, popup_image, importer, told, wake, pixels } = of;
    let showing = Showing::default();
    let failures = AtomicU32::new(0);
    // A copy of the view (0) or the popup (1) that failed: the next one is copied whole
    // (what changed since is told against the frame missed).
    let missed = [AtomicBool::new(false), AtomicBool::new(false)];
    let popup_reader =
        pixels.as_deref().map(|path| PixelReader::new(web_engine::protocol::popup_pixels(path)));
    let reader = pixels.map(PixelReader::new);
    // The pointer it asked last (told again only when it changes).
    let pointer = Mutex::new(None);
    Arc::new(move |event| {
        let said = match event {
            PageEvent::Frame(frame) => {
                // The page's own frame, or its popup's.
                let (target, reader) = match frame.popup {
                    true => (&popup_image, &popup_reader),
                    false => (&image, &reader),
                };
                let missed = &missed[usize::from(frame.popup)];
                let changed = match missed.load(Ordering::Acquire) {
                    true => Vec::new(),
                    false => changed_of(&frame),
                };
                let copied = match (&frame.native, reader) {
                    (Native::Pixels { stride }, Some(reader)) => reader.write(Pixels {
                        importer: importer.as_ref(),
                        image: target,
                        frame: &frame,
                        changed: &changed,
                        stride: *stride,
                    }),
                    _ => import(importer.as_ref(), target, &frame, &changed),
                };
                missed.store(!copied, Ordering::Release);
                if frame.popup {
                    drop(frame);
                    wake();
                    return;
                }
                if !copied {
                    if failures.fetch_add(1, Ordering::AcqRel) + 1 != IMPORTS_FAILED {
                        return;
                    }
                    let _ = told.send(Told::AsPixels);
                    wake();
                    return;
                }
                failures.store(0, Ordering::Release);
                showing.framed().then_some(Told::Event(WebEvent::Shown))
            }
            PageEvent::Event(Event::Loaded { .. }) => {
                showing.loaded(image.has_frame()).then_some(Told::Event(WebEvent::Shown))
            }
            PageEvent::Event(Event::Message { text, .. }) => {
                Some(Told::Event(WebEvent::Message(text)))
            }
            PageEvent::Event(Event::OpenLink { url, .. }) => {
                Some(Told::Event(WebEvent::OpenLink(url)))
            }
            PageEvent::Event(Event::Crashed { .. }) => Some(Told::Gone),
            PageEvent::Event(Event::Popup { rect, .. }) => {
                // Closed: its picture goes now (not after a while unshown) — here, in order
                // with its frames, so a next popup's first frame is never let go with it.
                if rect.is_none() {
                    popup_image.let_frame_go();
                }
                Some(Told::Popup(rect))
            }
            PageEvent::Event(Event::ContextMenu { menu, .. }) => Some(Told::Menu(menu)),
            PageEvent::Event(Event::Caret { rect, .. }) => Some(Told::Caret(rect)),
            PageEvent::Event(Event::Access { nodes, .. }) => Some(Told::Access(nodes)),
            // Another pointer: the window's changed at once (the mouse may stay still).
            PageEvent::Event(Event::Cursor { cursor, .. }) => {
                let mut last = pointer.lock().unwrap_or_else(PoisonError::into_inner);
                if last.replace(cursor) == Some(cursor) {
                    return;
                }
                drop(last);
                Some(Told::Cursor(cursor_of(cursor)))
            }
            PageEvent::Event(_) => None,
        };
        if let Some(said) = said {
            let _ = told.send(said);
        }
        wake();
    })
}

/// When a page is shown (its view's loader stops): its content loaded (the blank page it
/// starts on isn't) and a frame drawn — whichever comes last. A page still after it loads
/// draws no frame more, so one drawn before counts.
#[derive(Debug, Default)]
pub(super) struct Showing {
    loaded: AtomicBool,
    framed: AtomicBool,
    shown: AtomicBool,
}

impl Showing {
    /// A frame came: `true` if the page is shown now (once).
    pub(super) fn framed(&self) -> bool {
        self.framed.store(true, Ordering::Release);
        self.loaded.load(Ordering::Acquire) && !self.shown.swap(true, Ordering::AcqRel)
    }

    /// Its content loaded (`drawn`: a frame is in already): `true` if it's shown now (once).
    pub(super) fn loaded(&self, drawn: bool) -> bool {
        self.loaded.store(true, Ordering::Release);
        (drawn || self.framed.load(Ordering::Acquire)) && !self.shown.swap(true, Ordering::AcqRel)
    }
}

/// `frame`'s GPU texture copied into `image` (its buffer given back to the engine as
/// `frame` drops).
#[cfg(any(target_os = "macos", windows))]
fn import(
    importer: &dyn FrameImporter,
    image: &ImageData,
    frame: &ArrivedFrame,
    changed: &[render_api::PixelRect],
) -> bool {
    let native = match &frame.native {
        #[cfg(target_os = "macos")]
        Native::Surface(right) => NativeFrame::MacSurfacePort(right.port()),
        #[cfg(windows)]
        Native::Handle(handle) => NativeFrame::WindowsHandle(handle.0),
        // Pixels without their file (never asked as such): not drawn.
        Native::Pixels { .. } => return false,
    };
    let size = (frame.width, frame.height);
    importer.import(image, render_api::ImportedFrame { native, size, changed })
}

/// Elsewhere frames come as pixels only (Linux until its dmabufs): no texture to open.
#[cfg(not(any(target_os = "macos", windows)))]
fn import(
    _importer: &dyn FrameImporter,
    _image: &ImageData,
    _frame: &ArrivedFrame,
    _changed: &[render_api::PixelRect],
) -> bool {
    false
}

/// What changed in `frame`, as the renderer copies it (its pixels; empty: all of it).
fn changed_of(frame: &ArrivedFrame) -> Vec<render_api::PixelRect> {
    changed_within(&frame.changed, (frame.width, frame.height))
}

/// `changed` (an engine's, untrusted) within a frame of `size`: each cut to the frame, none
/// left empty; more than the protocol allows ([`web_engine::protocol::MAX_CHANGED`]), or
/// none left, is all of it (empty).
fn changed_within(
    changed: &[web_engine::protocol::Changed],
    (width, height): (u32, u32),
) -> Vec<render_api::PixelRect> {
    if changed.len() > web_engine::protocol::MAX_CHANGED {
        return Vec::new();
    }
    let side = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    let (width, height) = (side(width), side(height));
    let rect = |c: &web_engine::protocol::Changed| {
        let (x, y) = (side(c.x).min(width), side(c.y).min(height));
        let right = x.saturating_add(side(c.width)).min(width);
        let bottom = y.saturating_add(side(c.height)).min(height);
        (right > x && bottom > y).then(|| render_api::PixelRect::new(x, y, right, bottom))
    };
    changed.iter().filter_map(rect).collect()
}

/// A frame come as pixels, to write into its image ([`PixelReader::write`]).
struct Pixels<'a> {
    importer: &'a dyn FrameImporter,
    image: &'a ImageData,
    frame: &'a ArrivedFrame,
    /// Where it changed (empty: all of it).
    changed: &'a [render_api::PixelRect],
    /// The bytes from one of its rows to the next.
    stride: u32,
}

/// A page's frames read as pixels from its file (on the reader thread, where they arrive):
/// the file opened at the first frame, one buffer reused for every frame.
struct PixelReader {
    path: PathBuf,
    file: Mutex<Option<std::fs::File>>,
    buffer: Mutex<Vec<u8>>,
}

impl PixelReader {
    fn new(path: PathBuf) -> Self {
        Self { path, file: Mutex::new(None), buffer: Mutex::new(Vec::new()) }
    }

    /// A frame's [`Pixels`] read and written into its image: only where it changed.
    fn write(&self, pixels: Pixels<'_>) -> bool {
        use std::io::{Read, Seek};
        let Pixels { importer, image, frame, changed, stride } = pixels;
        let Some(length) = (stride as usize).checked_mul(frame.height as usize) else {
            return false;
        };
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        if file.is_none() {
            *file = std::fs::File::open(&self.path).ok();
        }
        let Some(file) = file.as_mut() else { return false };
        let mut buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.resize(length, 0);
        let read =
            file.seek(std::io::SeekFrom::Start(0)).and_then(|_| file.read_exact(&mut buffer));
        let size = (frame.width, frame.height);
        let frame = render_api::PixelFrame { bgra: &buffer, size, stride, changed };
        read.is_ok() && importer.write(image, frame)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_engine/sink.rs"]
mod tests;
