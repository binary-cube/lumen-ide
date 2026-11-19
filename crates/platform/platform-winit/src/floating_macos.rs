//! The floating layer over the pages (macOS): a transparent view of its own, over the
//! window's view and the pages in it (the system's web views, its subviews), that the app
//! draws what floats into (its own Metal layer, made by the renderer). It never takes the
//! mouse (`hitTest:` finds nothing in it): the mouse goes to what's under it — over a page,
//! the pages' holes give it to the app where something floats.

#![allow(unsafe_code)]

use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSResponder, NSView, NSWindowOrderingMode};
use objc2_foundation::NSPoint;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};

define_class!(
    // SAFETY: `NSView` has no subclassing requirements; the class implements no `Drop`.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AppFloatingLayerView"]
    struct FloatingView;

    impl FloatingView {
        /// Never the mouse's: it goes to the views under it.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            false
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            false
        }
    }
);

/// The layer's view, for its renderer: the same view, held (used only on the main thread,
/// where the window and its renderer live).
struct Handles(Retained<FloatingView>);

impl std::fmt::Debug for Handles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FloatingLayerView")
    }
}

// SAFETY: the view is only touched on the main thread (the window's), as the window's own
// handles are; the renderer only reads its pointer there.
unsafe impl Send for Handles {}
// SAFETY: as above.
unsafe impl Sync for Handles {}

impl HasWindowHandle for Handles {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let view = std::ptr::NonNull::from(&*self.0).cast();
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(view));
        // SAFETY: the view lives as long as `self` (retained).
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Handles {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
        // SAFETY: AppKit has no display handle to keep alive.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

/// The layer's `zPosition`: over the pages' (1, `web_view::above_the_app`).
const FLOATING_Z: f64 = 2.0;

/// The floating layer of a window.
#[derive(Debug)]
pub(crate) struct Floating {
    parent: Retained<NSView>,
    handles: Arc<Handles>,
}

impl Floating {
    /// Over the window's view `window` (its whole size, following it); `None`: not on the
    /// main thread, or the window's view isn't found.
    pub(crate) fn new(window: &winit::window::Window) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
        // SAFETY: the window's `NSView`, alive as long as the window; retained here.
        let parent = unsafe { Retained::retain(h.ns_view.as_ptr().cast::<NSView>()) }?;
        let frame = parent.bounds();
        let this = FloatingView::alloc(mtm).set_ivars(());
        // SAFETY: `-[NSView initWithFrame:]`, the designated initializer, on a fresh object.
        let view: Retained<FloatingView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        view.setHidden(true);
        parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);
        Some(Self { parent, handles: Arc::new(Handles(view)) })
    }
}

impl platform_api::FloatingLayer for Floating {
    fn handles(&self) -> Arc<dyn platform_api::WindowHandles> {
        Arc::clone(&self.handles) as Arc<dyn platform_api::WindowHandles>
    }

    fn set_visible(&self, visible: bool) {
        if self.handles.0.isHidden() == visible {
            self.handles.0.setHidden(!visible);
        }
    }

    fn raise(&self) {
        // Its layer above the pages' too: theirs is lifted over the app's own image (a
        // higher `zPosition`, whatever the views' order), so it's lifted higher. Set again
        // when its renderer gives it another layer.
        let view: &NSView = &self.handles.0;
        // SAFETY: `-[NSView layer]` and `-[CALayer zPosition]` / `-setZPosition:` with their
        // declared types, on the main thread.
        unsafe {
            let layer: Option<Retained<objc2::runtime::AnyObject>> = msg_send![view, layer];
            if let Some(layer) = layer {
                let z: f64 = msg_send![&*layer, zPosition];
                if z < FLOATING_Z {
                    let _: () = msg_send![&*layer, setZPosition: FLOATING_Z];
                }
            }
        }
        // Already the topmost of the window's views: nothing to do.
        let subviews = self.parent.subviews();
        let top = subviews.lastObject();
        if top.as_deref().is_some_and(|t| std::ptr::eq(t, view)) {
            return;
        }
        self.parent.addSubview_positioned_relativeTo(view, NSWindowOrderingMode::Above, None);
    }
}

impl Drop for Floating {
    fn drop(&mut self) {
        self.handles.0.removeFromSuperview();
    }
}
