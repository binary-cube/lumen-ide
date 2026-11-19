//! Holes in a page (macOS): what the app draws over a page of the system's web engine (a
//! pinned window, a tooltip) shows through it. The page's layer gets a mask with those
//! places cut out (rounded as what's drawn there), and a local event monitor gives the
//! mouse there to the window's own view (the app) instead of the page: a press that starts
//! in a hole goes to the app until it's released, one that starts on the page stays the
//! page's. No holes: no mask, the page whole.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use block2::RcBlock;
use kernel::Rect;
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// CoreGraphics' path (opaque).
#[repr(C)]
struct CgPath {
    _private: [u8; 0],
}

// SAFETY: `CGPathRef` is a pointer to the `CGPath` struct, as Objective-C encodes it.
unsafe impl RefEncode for CgPath {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGPath", &[]));
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPathCreateMutable() -> *mut CgPath;
    fn CGPathAddRect(path: *mut CgPath, transform: *const std::ffi::c_void, rect: NSRect);
    fn CGPathAddRoundedRect(
        path: *mut CgPath,
        transform: *const std::ffi::c_void,
        rect: NSRect,
        corner_width: f64,
        corner_height: f64,
    );
    fn CGPathRelease(path: *mut CgPath);
}

/// `NSEventMask`s of the mouse's presses, releases, moves and drags.
const MOUSE_EVENTS: u64 = (1 << 1) // left down
    | (1 << 2) // left up
    | (1 << 3) // right down
    | (1 << 4) // right up
    | (1 << 5) // moved
    | (1 << 6) // left dragged
    | (1 << 7) // right dragged
    | (1 << 25) // other down
    | (1 << 26) // other up
    | (1 << 27); // other dragged

/// What the mask and the monitor read.
#[derive(Debug, Default)]
struct State {
    /// The holes (physical pixels, in the window) and their corners' radius.
    holes: Vec<(Rect, f32)>,
    /// A press began in a hole: its drags and its release are the app's too.
    pressed: bool,
    /// The page isn't drawn in its holes (else they only take the mouse: what floats is
    /// drawn over the page, on the window's floating layer).
    cut: bool,
}

/// A page's holes.
pub(crate) struct Holes {
    window: Arc<winit::window::Window>,
    /// The window's own view (the app's): the page's parent.
    parent: Retained<AnyObject>,
    /// The page's view.
    page: Retained<AnyObject>,
    state: Rc<RefCell<State>>,
    /// The event monitor, made with the first hole, removed with the page.
    monitor: RefCell<Option<Retained<AnyObject>>>,
}

impl std::fmt::Debug for Holes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Holes").field("state", &self.state).finish_non_exhaustive()
    }
}

impl Holes {
    /// For `page` (an `NSView`) in `window`; `None`: the window's view isn't found.
    pub(crate) fn new(
        window: &Arc<winit::window::Window>,
        page: Retained<AnyObject>,
    ) -> Option<Self> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
        // SAFETY: the window's `NSView`, alive as long as the window (kept by `window`);
        // retained here.
        let parent = unsafe { Retained::retain(h.ns_view.as_ptr().cast::<AnyObject>()) }?;
        Some(Self {
            window: Arc::clone(window),
            parent,
            page,
            state: Rc::default(),
            monitor: RefCell::new(None),
        })
    }

    /// The holes now (`area`: the page's, physical pixels in the window): those over it
    /// take the mouse from it — and, `cut`, are cut out of it.
    pub(crate) fn set(&self, holes: &[(Rect, f32)], area: Rect, cut: bool) {
        let over: Vec<(Rect, f32)> =
            holes.iter().copied().filter(|(r, _)| r.intersect(&area).is_some()).collect();
        let changed = {
            let s = self.state.borrow();
            s.holes != over || s.cut != cut
        };
        if !changed {
            return;
        }
        {
            let mut s = self.state.borrow_mut();
            s.holes = over;
            s.cut = cut;
        }
        self.mask(area);
        if !self.state.borrow().holes.is_empty() && self.monitor.borrow().is_none() {
            self.watch();
        }
    }

    /// The page moved or resized: its mask follows.
    pub(crate) fn moved(&self, area: Rect) {
        if !self.state.borrow().holes.is_empty() {
            self.mask(area);
        }
    }

    /// The page's layer masked: all of it but the holes (`area`: where the page is).
    fn mask(&self, area: Rect) {
        let scale = self.window.scale_factor();
        let holes = {
            let s = self.state.borrow();
            if s.cut { s.holes.clone() } else { Vec::new() }
        };
        // SAFETY: `-[NSView layer]`, `-[CALayer bounds]`, `-setMask:`, `+[CAShapeLayer
        // layer]`, `-setFrame:`, `-setFillRule:`, `-setPath:`, `-contentsAreFlipped` are called
        // with their declared types on the main thread; the path is retained by the layer
        // and released here after.
        unsafe {
            let layer: Option<Retained<AnyObject>> = objc2::msg_send![&*self.page, layer];
            let Some(layer) = layer else { return };
            if holes.is_empty() {
                let none: *const AnyObject = std::ptr::null();
                let _: () = objc2::msg_send![&*layer, setMask: none];
                return;
            }
            let bounds: NSRect = objc2::msg_send![&*layer, bounds];
            let mask: Retained<AnyObject> = objc2::msg_send![objc2::class!(CAShapeLayer), layer];
            let _: () = objc2::msg_send![&*mask, setFrame: bounds];
            let _: () = objc2::msg_send![&*layer, setMask: &*mask];
            // Its y as drawn: from the top when the layer is flipped (the page's view is),
            // from the bottom otherwise.
            let flipped: bool = objc2::msg_send![&*mask, contentsAreFlipped];
            let path = CGPathCreateMutable();
            if path.is_null() {
                return;
            }
            CGPathAddRect(path, std::ptr::null(), bounds);
            for (hole, radius) in holes {
                let x = f64::from(hole.min_x() - area.min_x()) / scale;
                let top = f64::from(hole.min_y() - area.min_y()) / scale;
                let (w, h) =
                    (f64::from(hole.size.width) / scale, f64::from(hole.size.height) / scale);
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let y = if flipped { top } else { bounds.size.height - top - h };
                let r = (f64::from(radius) / scale).clamp(0.0, (w.min(h) / 2.0 - 0.01).max(0.0));
                let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
                if r > 0.0 {
                    CGPathAddRoundedRect(path, std::ptr::null(), rect, r, r);
                } else {
                    CGPathAddRect(path, std::ptr::null(), rect);
                }
            }
            let rule = NSString::from_str("even-odd");
            let _: () = objc2::msg_send![&*mask, setFillRule: &*rule];
            let _: () = objc2::msg_send![&*mask, setPath: path.cast_const()];
            CGPathRelease(path);
        }
    }

    /// The mouse in a hole given to the window's view (the app): a local monitor of this
    /// window's mouse events.
    fn watch(&self) {
        let state = Rc::clone(&self.state);
        let parent = self.parent.clone();
        let scale = self.window.scale_factor();
        let handler = RcBlock::new(move |event: NonNull<AnyObject>| -> *mut AnyObject {
            // SAFETY: AppKit passes a valid event, used only during this call on the main
            // thread; the view is retained by the block.
            unsafe { route(event, &parent, &state, scale) }
        });
        // SAFETY: `+[NSEvent addLocalMonitorForEventsMatchingMask:handler:]` with a block of
        // the declared type `NSEvent *(^)(NSEvent *)`; AppKit copies it and returns the
        // monitor, removed in `drop`.
        let monitor: Option<Retained<AnyObject>> = unsafe {
            objc2::msg_send![
                objc2::class!(NSEvent),
                addLocalMonitorForEventsMatchingMask: MOUSE_EVENTS,
                handler: &*handler
            ]
        };
        *self.monitor.borrow_mut() = monitor;
    }
}

/// `event` given to `parent` (and taken from the page) when it's in a hole of `state`'s, or
/// part of a press that began in one; else left as it is.
///
/// # Safety
/// `event` is a valid `NSEvent`, on the main thread.
unsafe fn route(
    event: NonNull<AnyObject>,
    parent: &AnyObject,
    state: &RefCell<State>,
    scale: f64,
) -> *mut AnyObject {
    let event_ref = unsafe { event.as_ref() };
    let pass = event.as_ptr();
    // SAFETY: `-[NSEvent window]`, `-[NSView window]`, `-type`, `-locationInWindow`,
    // `-convertPoint:fromView:`, `-isFlipped`, `-bounds` with their declared types.
    let (same, kind, at) = unsafe {
        let window: *const AnyObject = objc2::msg_send![event_ref, window];
        let own: *const AnyObject = objc2::msg_send![parent, window];
        let kind: u64 = objc2::msg_send![event_ref, type];
        let location: NSPoint = objc2::msg_send![event_ref, locationInWindow];
        let none: *const AnyObject = std::ptr::null();
        let p: NSPoint = objc2::msg_send![parent, convertPoint: location, fromView: none];
        let flipped: bool = objc2::msg_send![parent, isFlipped];
        let bounds: NSRect = objc2::msg_send![parent, bounds];
        let top = if flipped { p.y } else { bounds.size.height - p.y };
        (
            !window.is_null() && window == own,
            kind,
            kernel::Point::new((p.x * scale) as f32, (top * scale) as f32),
        )
    };
    if !same {
        return pass;
    }
    let mut s = state.borrow_mut();
    let inside = s.holes.iter().any(|(r, _)| r.contains(at));
    // Presses: 1 left, 3 right, 25 other; releases 2, 4, 26; drags 6, 7, 27; moves 5.
    let take = match kind {
        1 | 3 | 25 => {
            s.pressed = inside;
            inside
        }
        2 | 4 | 26 => std::mem::take(&mut s.pressed),
        6 | 7 | 27 => s.pressed,
        5 => inside,
        _ => false,
    };
    drop(s);
    if !take {
        return pass;
    }
    // SAFETY: the window's view handles each of these (`NSResponder`'s), with the event.
    unsafe {
        match kind {
            1 => {
                let _: () = objc2::msg_send![parent, mouseDown: event_ref];
            }
            2 => {
                let _: () = objc2::msg_send![parent, mouseUp: event_ref];
            }
            3 => {
                let _: () = objc2::msg_send![parent, rightMouseDown: event_ref];
            }
            4 => {
                let _: () = objc2::msg_send![parent, rightMouseUp: event_ref];
            }
            5 => {
                let _: () = objc2::msg_send![parent, mouseMoved: event_ref];
            }
            6 => {
                let _: () = objc2::msg_send![parent, mouseDragged: event_ref];
            }
            7 => {
                let _: () = objc2::msg_send![parent, rightMouseDragged: event_ref];
            }
            25 => {
                let _: () = objc2::msg_send![parent, otherMouseDown: event_ref];
            }
            26 => {
                let _: () = objc2::msg_send![parent, otherMouseUp: event_ref];
            }
            _ => {
                let _: () = objc2::msg_send![parent, otherMouseDragged: event_ref];
            }
        }
    }
    std::ptr::null_mut()
}

impl Drop for Holes {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.borrow_mut().take() {
            // SAFETY: `+[NSEvent removeMonitor:]` with the monitor it returned, on the main
            // thread.
            unsafe {
                let _: () = objc2::msg_send![objc2::class!(NSEvent), removeMonitor: &*monitor];
            }
        }
    }
}
