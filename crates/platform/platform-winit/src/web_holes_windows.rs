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
    CombineRgn, CreateEllipticRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, RGN_DIFF,
    SetWindowRgn,
};

/// A page's holes: its host window and the holes last told.
#[derive(Debug)]
pub(crate) struct Holes {
    hwnd: HWND,
    /// The holes (physical pixels, in the app's window) and their corners' radius.
    holes: RefCell<Vec<(Rect, f32)>>,
    /// Whether a region is set now.
    cut: std::cell::Cell<bool>,
    /// Its corners rounded (radius, which: `platform_api::Corners`' bits).
    corners: std::cell::Cell<(f32, u8)>,
}

impl Holes {
    /// For the page hosted by window `hwnd` (its own, a child of the app's).
    pub(crate) fn new(hwnd: isize) -> Self {
        Self {
            hwnd: hwnd as HWND,
            holes: RefCell::default(),
            cut: std::cell::Cell::new(false),
            corners: std::cell::Cell::new((0.0, 0)),
        }
    }

    /// The holes now (`area`: the page's, physical pixels in the window): those over it cut
    /// out of it — none unless `cut` (what floats is drawn over the page then, on the
    /// window's floating layer, which takes the mouse there itself).
    pub(crate) fn set(&self, holes: &[(Rect, f32)], area: Rect, cut: bool) {
        *self.holes.borrow_mut() = if cut { holes.to_vec() } else { Vec::new() };
        self.apply(area);
    }

    /// Its `corners` rounded by `radius` (as the card it's in), `area` where it is.
    pub(crate) fn set_corners(&self, radius: f32, corners: u8, area: Rect) {
        if self.corners.replace((radius, corners)) != (radius, corners) {
            self.apply(area);
        }
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
        let (radius, corners) = self.corners.get();
        let rounded = corners != 0 && radius >= 1.0;
        if over.is_empty() && !rounded {
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
            if rounded {
                // Each corner: its square taken out, a quarter of a circle put back.
                use platform_api::Corners;
                let r = radius.round() as i32;
                for (corner, x, y) in [
                    (Corners::TOP_LEFT, 0, 0),
                    (Corners::TOP_RIGHT, width - r, 0),
                    (Corners::BOTTOM_LEFT, 0, height - r),
                    (Corners::BOTTOM_RIGHT, width - r, height - r),
                ] {
                    if corners & corner == 0 {
                        continue;
                    }
                    let square = CreateRectRgn(x, y, x + r, y + r);
                    // The circle's box: 2r wide, its quarter inside the square.
                    let cx = if x == 0 { 0 } else { width - 2 * r };
                    let cy = if y == 0 { 0 } else { height - 2 * r };
                    let circle = CreateEllipticRgn(cx, cy, cx + 2 * r + 1, cy + 2 * r + 1);
                    if !square.is_null() && !circle.is_null() {
                        CombineRgn(square, square, circle, RGN_DIFF);
                        CombineRgn(region, region, square, RGN_DIFF);
                    }
                    if !square.is_null() {
                        DeleteObject(square);
                    }
                    if !circle.is_null() {
                        DeleteObject(circle);
                    }
                }
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
