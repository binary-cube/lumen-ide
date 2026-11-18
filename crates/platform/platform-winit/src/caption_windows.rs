//! Windows without the system's title bar (as on macOS, the app's top bar is the title
//! bar): the window keeps its shadow and gets rounded corners (Windows 11), resizes from
//! its edges, and the app's maximize button is the system's for the snap layouts (hovering
//! it shows them) — through a subclass answering where things are (`WM_NCHITTEST`). The
//! mouse over that button is the system's then: the subclass tells the app (its hover).
//! Alt or F10 alone don't enter the system's menu loop (the window menu waiting for a key):
//! they're the app's, its menu drawer's keys.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTMAXBUTTON, HTRIGHT, HTTOP,
    HTTOPLEFT, HTTOPRIGHT, IsZoomed, SC_KEYMENU, SW_MAXIMIZE, SW_RESTORE, ShowWindow, WM_MOUSEMOVE,
    WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    WM_SYSCOMMAND,
};

/// What the subclass knows: where the app draws its maximize button (client pixels), and
/// whether the window is full screen (nothing to resize then); what it tells: the mouse is
/// over that button.
#[derive(Debug)]
pub struct Caption {
    hwnd: HWND,
    maximize: Mutex<Option<(i32, i32, i32, i32)>>,
    fullscreen: AtomicBool,
    maximize_hovered: AtomicBool,
}

// SAFETY: the handle is only an identifier here, used with thread-safe window calls.
#[allow(unsafe_code)]
unsafe impl Send for Caption {}
#[allow(unsafe_code)]
unsafe impl Sync for Caption {}

impl Caption {
    /// The maximize button's place (physical client pixels: x, y, width, height).
    pub fn set_maximize(&self, rect: Option<(i32, i32, i32, i32)>) {
        if let Ok(mut maximize) = self.maximize.lock() {
            *maximize = rect;
        }
    }

    /// Full screen: nothing resizes, the corners are square (rounded again after).
    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.fullscreen.store(fullscreen, Ordering::Relaxed);
        round_corners(self.hwnd, !fullscreen);
    }

    /// The mouse is over the app's maximize button (the system has it there).
    pub fn maximize_hovered(&self) -> bool {
        self.maximize_hovered.load(Ordering::Relaxed)
    }

    /// The hover changed: the app draws it (a repaint asked).
    fn set_maximize_hovered(&self, hovered: bool) {
        if self.maximize_hovered.swap(hovered, Ordering::Relaxed) != hovered {
            // SAFETY: this window's own repaint request.
            #[allow(unsafe_code)]
            unsafe {
                InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
        }
    }
}

/// The subclass's id (one per window).
const SUBCLASS: usize = 0x6361_7074;

pub(crate) fn hwnd(window: &winit::window::Window) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as HWND),
        _ => None,
    }
}

/// Rounded corners, and the subclass answering where things are. `None`: no window handle.
#[allow(unsafe_code)]
pub fn install(window: &winit::window::Window) -> Option<Arc<Caption>> {
    let hwnd = hwnd(window)?;
    let caption = Arc::new(Caption {
        hwnd,
        maximize: Mutex::default(),
        fullscreen: AtomicBool::new(false),
        maximize_hovered: AtomicBool::new(false),
    });
    round_corners(hwnd, true);
    // SAFETY: the subclass gets its own reference to `caption`, released at WM_NCDESTROY.
    let data = Arc::into_raw(Arc::clone(&caption)) as usize;
    let installed = unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS, data) };
    if installed == 0 {
        // SAFETY: not handed over: taken back.
        drop(unsafe { Arc::from_raw(data as *const Caption) });
        return None;
    }
    Some(caption)
}

/// Rounded corners (Windows 11) or square ones (full screen). Windows 10 doesn't know the
/// attribute: it's refused, the corners stay square — logged, to tell why.
#[allow(unsafe_code)]
fn round_corners(hwnd: HWND, round: bool) {
    let preference = if round { DWMWCP_ROUND } else { DWMWCP_DONOTROUND };
    // SAFETY: a valid window; the attribute is a u32 read during the call.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const preference).cast(),
            std::mem::size_of_val(&preference) as u32,
        )
    };
    if result == 0 {
        tracing::debug!(round, "window corners set");
    } else {
        tracing::info!("window corners refused by the system (0x{result:08x}): square corners");
    }
}

/// The cursor's position in a mouse message (screen pixels, signed).
fn point(lparam: LPARAM) -> POINT {
    POINT { x: (lparam & 0xFFFF) as i16 as i32, y: ((lparam >> 16) & 0xFFFF) as i16 as i32 }
}

#[allow(unsafe_code)]
unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: `data` is the `Caption` handed over in `install`, alive until WM_NCDESTROY.
    let caption = unsafe { &*(data as *const Caption) };
    match msg {
        WM_NCHITTEST => {
            let at = point(lparam);
            let mut window = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            // SAFETY: plain queries of this window.
            let (zoomed, dpi) = unsafe {
                GetWindowRect(hwnd, &mut window);
                (IsZoomed(hwnd) != 0, GetDpiForWindow(hwnd).max(96))
            };
            // The edges resize (not when maximized or full screen).
            if !zoomed && !caption.fullscreen.load(Ordering::Relaxed) {
                let border = (6 * dpi / 96) as i32;
                let left = at.x < window.left + border;
                let right = at.x >= window.right - border;
                let top = at.y < window.top + border;
                let bottom = at.y >= window.bottom - border;
                let edge = match (top, bottom, left, right) {
                    (true, _, true, _) => Some(HTTOPLEFT),
                    (true, _, _, true) => Some(HTTOPRIGHT),
                    (_, true, true, _) => Some(HTBOTTOMLEFT),
                    (_, true, _, true) => Some(HTBOTTOMRIGHT),
                    (true, ..) => Some(HTTOP),
                    (_, true, ..) => Some(HTBOTTOM),
                    (_, _, true, _) => Some(HTLEFT),
                    (_, _, _, true) => Some(HTRIGHT),
                    _ => None,
                };
                if let Some(edge) = edge {
                    return edge as LRESULT;
                }
            }
            // The app's maximize button: the system's (its snap layouts on hover).
            let mut client = at;
            // SAFETY: converts a point for this window.
            unsafe { ScreenToClient(hwnd, &mut client) };
            let over = caption.maximize.lock().ok().and_then(|m| *m).is_some_and(|(x, y, w, h)| {
                client.x >= x && client.x < x + w && client.y >= y && client.y < y + h
            });
            if over {
                return HTMAXBUTTON as LRESULT;
            }
            // SAFETY: the default handling (winit's, then the system's).
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        // The mouse over the button (the system's area): the app draws its hover; told
        // when it leaves the window's non-client parts.
        WM_NCMOUSEMOVE => {
            let over = wparam == HTMAXBUTTON as WPARAM;
            if over && !caption.maximize_hovered() {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: a valid, owned structure for this window.
                unsafe { TrackMouseEvent(&mut track) };
            }
            caption.set_maximize_hovered(over);
            // SAFETY: the default handling.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCMOUSELEAVE | WM_MOUSEMOVE => {
            caption.set_maximize_hovered(false);
            // SAFETY: the default handling.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        // Alt or F10 released alone (no mnemonic key: `lparam` 0) would start the system's
        // menu loop, swallowing the next key: the app's menu drawer takes them instead. Alt +
        // Space (the window menu) still reaches the system.
        WM_SYSCOMMAND if (wparam & 0xFFF0) == SC_KEYMENU as WPARAM && lparam == 0 => 0,
        // A click on that button maximizes / restores (the system would draw its own).
        WM_NCLBUTTONDOWN if wparam == HTMAXBUTTON as WPARAM => 0,
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as WPARAM => {
            // SAFETY: plain calls on this window.
            unsafe {
                let show = if IsZoomed(hwnd) != 0 { SW_RESTORE } else { SW_MAXIMIZE };
                ShowWindow(hwnd, show);
            }
            0
        }
        WM_NCDESTROY => {
            // SAFETY: the window goes: the subclass and its reference to `caption` too.
            unsafe {
                RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS);
                drop(Arc::from_raw(data as *const Caption));
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
        // SAFETY: the default handling.
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}
