//! Files and folders dragged from the system's file manager onto a window. winit tells
//! them one by one (`HoveredFile` per item as the drag enters, `DroppedFile` per item on
//! the drop) and without the pointer: they're gathered here and told to the app as one
//! event once the loop is idle, with the pointer asked of the system. While a drag is over
//! a window its pointer is asked again on a short timer (neither AppKit nor OLE sends mouse
//! moves to the window under a drag).

use std::path::PathBuf;
use std::time::Duration;

use kernel::Point;

/// How often the pointer is asked while a drag is over a window.
pub(crate) const POLL: Duration = Duration::from_millis(40);

/// A window's drag from the file manager.
#[derive(Debug, Default)]
pub(crate) struct FileDrag {
    /// The items of the drag over the window.
    hovered: Vec<PathBuf>,
    /// The app was told about `hovered` (a cancel is told then, not otherwise).
    announced: bool,
    /// The items dropped, waiting for the loop to be idle.
    dropped: Vec<PathBuf>,
}

/// What a window's drag has to tell the app now.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DragNews {
    Hovered(Vec<PathBuf>),
    Dropped(Vec<PathBuf>),
}

impl FileDrag {
    pub fn hovered(&mut self, path: PathBuf) {
        // A new drag after one already told (its end wasn't seen): starts over.
        if self.announced {
            self.hovered.clear();
            self.announced = false;
        }
        self.hovered.push(path);
    }

    pub fn dropped(&mut self, path: PathBuf) {
        self.dropped.push(path);
    }

    /// The drag left: `true` if the app was told about it (it's told it ended).
    pub fn cancelled(&mut self) -> bool {
        let told = self.announced;
        self.hovered.clear();
        self.announced = false;
        told
    }

    /// The drop first (it ends the hover), else a hover not told yet.
    pub fn take_news(&mut self) -> Option<DragNews> {
        if !self.dropped.is_empty() {
            self.hovered.clear();
            self.announced = false;
            return Some(DragNews::Dropped(std::mem::take(&mut self.dropped)));
        }
        if !self.hovered.is_empty() && !self.announced {
            self.announced = true;
            return Some(DragNews::Hovered(self.hovered.clone()));
        }
        None
    }

    /// A drag the app knows of is over the window (its pointer is followed).
    pub fn active(&self) -> bool {
        self.announced
    }
}

/// The pointer over `window`'s content (physical pixels), asked of the system.
#[cfg(target_os = "macos")]
pub(crate) fn pointer(window: &winit::window::Window) -> Option<Point> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    // SAFETY: `ns_view` is the winit window's view, alive as long as we hold `&Window`;
    // called on the main thread (the event loop).
    #[allow(unsafe_code)]
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let ns_window = view.window()?;
    // In the window's coordinates (points, from its bottom-left), then the view's.
    let at = view.convertPoint_fromView(ns_window.mouseLocationOutsideOfEventStream(), None);
    let y = if view.isFlipped() { at.y } else { view.bounds().size.height - at.y };
    let scale = window.scale_factor();
    Some(Point::new((at.x * scale) as f32, (y * scale) as f32))
}

/// The pointer over `window`'s client area (physical pixels: the app is DPI aware).
#[cfg(target_os = "windows")]
pub(crate) fn pointer(window: &winit::window::Window) -> Option<Point> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let hwnd = crate::caption_windows::hwnd(window)?;
    let mut at = POINT { x: 0, y: 0 };
    // SAFETY: `at` is a valid out pointer; `hwnd` is the live winit window's handle.
    #[allow(unsafe_code)]
    let ok = unsafe { GetCursorPos(&mut at) != 0 && ScreenToClient(hwnd, &mut at) != 0 };
    ok.then(|| Point::new(at.x as f32, at.y as f32))
}

/// Elsewhere the system isn't asked: the last known position stays.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn pointer(_window: &winit::window::Window) -> Option<Point> {
    None
}

#[cfg(test)]
#[path = "../tests/unit/file_drag.rs"]
mod tests;
