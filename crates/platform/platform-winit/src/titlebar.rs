//! Unified title bar on macOS: the app content extends under the (transparent) title
//! bar, and the window buttons (close / minimize / zoom) are centered
//! on the height of the app bar.
//!
//! AppKit repositions the buttons on resize, so the positioning is redone after every
//! `Resized`.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
use objc2_foundation::NSPoint;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// A drag started by the app is in progress (the window is movable until it ends).
static DRAGGING: AtomicBool = AtomicBool::new(false);

#[allow(unsafe_code)]
pub(crate) fn ns_window(window: &winit::window::Window) -> Option<objc2::rc::Retained<NSWindow>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    // SAFETY: `ns_view` is the winit window's view, alive as long as we hold `&Window`;
    // called on the main thread (the event loop).
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

/// Moves the window with the mouse (a press on the empty part of the bar): movable just
/// for this drag.
pub(crate) fn start_drag(window: &winit::window::Window) {
    if let Some(ns_window) = ns_window(window) {
        ns_window.setMovable(true);
        DRAGGING.store(true, Ordering::Relaxed);
    }
    if let Err(e) = window.drag_window() {
        tracing::debug!(error = %e, "the window move didn't start");
    }
}

/// The mouse is used again in a window (the drag is over): not movable by AppKit again.
pub(crate) fn end_drag(windows: impl Iterator<Item = impl AsRef<winit::window::Window>>) {
    if DRAGGING.swap(false, Ordering::Relaxed) {
        for window in windows {
            if let Some(ns_window) = ns_window(window.as_ref()) {
                ns_window.setMovable(false);
            }
        }
    }
}

/// Centers the window buttons on a bar `height` logical pixels tall, the first at `left`.
#[allow(unsafe_code)]
pub(crate) fn layout_buttons(window: &winit::window::Window, height: f64, left: f64) {
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // SAFETY: `ns_view` is the winit window's view, alive as long as we hold `&Window`; the
    // AppKit calls below are made on the main thread (the event loop).
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(ns_window) = view.window() else { return };
    let buttons = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ]
    .map(|b| ns_window.standardWindowButton(b));
    let [Some(close), Some(mini), Some(zoom)] = buttons else { return };
    // The bar's buttons (project, search…) are under the title bar: AppKit would move the
    // window on any press there, even a click with a slight jitter. The app starts the
    // drag itself, only on the empty part of the bar (`start_drag`).
    if !DRAGGING.load(Ordering::Relaxed) {
        ns_window.setMovable(false);
    }

    // The title bar container (the buttons' grandparent) gets the height of our bar.
    // SAFETY: we only read the window's view hierarchy.
    let container = unsafe { close.superview().and_then(|v| v.superview()) };
    if let Some(container) = container {
        let mut frame = container.frame();
        frame.size.height = height;
        frame.origin.y = ns_window.frame().size.height - height;
        container.setFrame(frame);
    }
    let spacing = mini.frame().origin.x - close.frame().origin.x;
    for (i, button) in [&close, &mini, &zoom].into_iter().enumerate() {
        let size = button.frame().size;
        let origin =
            NSPoint::new(left + spacing * i as f64, ((height - size.height) / 2.0).round());
        button.setFrameOrigin(origin);
    }
}
