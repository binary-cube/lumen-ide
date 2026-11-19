//! Holes in a page (Windows): what the app draws over a page of the system's web engine (a
//! pinned window, the resources' popover, a notification, a tooltip) shows through it. The
//! window hosting the page (WebView2's, a child of the app's) gets a region with those
//! places cut out, rounded as what's drawn there: the page and its own windows are drawn
//! only inside it, and the mouse over a hole reaches the app's window under it (a window
//! takes the mouse only inside its region). No holes: no region, the page whole.

#![allow(unsafe_code)]

use std::cell::RefCell;

use kernel::Rect;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, RGN_DIFF, SetWindowRgn,
};

/// A page's holes: its host window and the holes last told.
#[derive(Debug)]
pub(crate) struct Holes {
    hwnd: HWND,
    /// The holes (physical pixels, in the app's window) and their corners' radius.
    holes: RefCell<Vec<(Rect, f32)>>,
    /// Whether a region is set now.
    cut: std::cell::Cell<bool>,
}

impl Holes {
    /// For the page hosted by window `hwnd` (its own, a child of the app's).
    pub(crate) fn new(hwnd: isize) -> Self {
        Self { hwnd: hwnd as HWND, holes: RefCell::default(), cut: std::cell::Cell::new(false) }
    }

    /// The holes now (`area`: the page's, physical pixels in the window): those over it cut
    /// out of it.
    pub(crate) fn set(&self, holes: &[(Rect, f32)], area: Rect) {
        *self.holes.borrow_mut() = holes.to_vec();
        self.apply(area);
    }

    /// The page moved or resized to `area`: its holes cut again where they are on it.
    pub(crate) fn moved(&self, area: Rect) {
        self.apply(area);
    }

    fn apply(&self, area: Rect) {
        // Where the host window is (as the engine placed it: rounded).
        let (left, top) = (area.min_x().round(), area.min_y().round());
        let (width, height) =
            (area.size.width.max(1.0).round() as i32, area.size.height.max(1.0).round() as i32);
        let over: Vec<(Rect, f32)> = self
            .holes
            .borrow()
            .iter()
            .filter_map(|(hole, radius)| area.intersect(hole).map(|_| (*hole, *radius)))
            .collect();
        if over.is_empty() {
            if self.cut.replace(false) {
                // SAFETY: the page's window lives as long as its view; no region: whole.
                unsafe { SetWindowRgn(self.hwnd, std::ptr::null_mut(), 1) };
            }
            return;
        }
        // SAFETY: GDI regions made and combined here; the one given to the window is the
        // system's from then on (not deleted), the holes' are deleted.
        unsafe {
            let region = CreateRectRgn(0, 0, width, height);
            if region.is_null() {
                return;
            }
            for (hole, radius) in over {
                let x0 = (hole.min_x() - left).round() as i32;
                let y0 = (hole.min_y() - top).round() as i32;
                let x1 = (hole.max_x() - left).round() as i32;
                let y1 = (hole.max_y() - top).round() as i32;
                // The ellipse's size is the corner's diameter; the right and bottom edges
                // are outside the region (one more).
                let d = (radius * 2.0).round().max(0.0) as i32;
                let cut = if d > 0 {
                    CreateRoundRectRgn(x0, y0, x1 + 1, y1 + 1, d, d)
                } else {
                    CreateRectRgn(x0, y0, x1, y1)
                };
                if !cut.is_null() {
                    CombineRgn(region, region, cut, RGN_DIFF);
                    DeleteObject(cut);
                }
            }
            if SetWindowRgn(self.hwnd, region, 1) == 0 {
                DeleteObject(region);
                return;
            }
        }
        self.cut.set(true);
    }
}
