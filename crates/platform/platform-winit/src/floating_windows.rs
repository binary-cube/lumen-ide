//! The floating layer over the pages (Windows): a window of its own over the app's window
//! and its pages (the system's web views, child windows), that the app draws what floats
//! into — through DirectComposition only (no redirection surface of the system's under it),
//! transparent where nothing is drawn.
//!
//! It is a popup the app's window owns, not a child of it: a child window cuts its place
//! out of the parent's composition (what's under its transparent pixels would be the
//! desktop), a window it owns is composed over it, alpha included. Owned, it stays just over
//! its owner (never over other programs' windows), goes with it when it's minimized, and
//! follows it when it moves or changes size (its owner's messages are watched: a window
//! dragged draws no frames). It sits on the owner's client area, so its coordinates are the
//! owner's. Its region is where something floats (with room for its shadow): only there is
//! it drawn and does it take the mouse, which it hands to the owner as if it were there.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::num::NonZeroIsize;
use std::sync::{Arc, Once};

use kernel::Rect;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, RGN_OR,
    SetWindowRgn, ValidateRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GW_HWNDPREV, GW_OWNER, GetClientRect,
    GetForegroundWindow, GetWindow, HTCLIENT, HWND_TOP, RegisterClassW, SW_HIDE, SW_SHOWNA,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, WINDOWPOS, WM_ENABLE,
    WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SHOWWINDOW, WM_WINDOWPOSCHANGED, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN,
    WM_XBUTTONUP, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

/// `MA_NOACTIVATE`: a click on it doesn't activate anything.
const MA_NOACTIVATE: LRESULT = 3;

/// The id the layer watches its owner's messages under (`SetWindowSubclass`).
const FOLLOW_ID: usize = 0x666C_6F61;

/// The window that owns `hwnd` (the app's).
unsafe fn owner(hwnd: HWND) -> HWND {
    // SAFETY: asked of a live window.
    unsafe { GetWindow(hwnd, GW_OWNER) }
}

/// `layer` put over its owner's client area (`owner`); its size.
unsafe fn place(layer: HWND, owner: HWND) -> (i32, i32) {
    // SAFETY: both windows alive.
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(owner, &mut client);
        let mut origin = POINT { x: 0, y: 0 };
        ClientToScreen(owner, &mut origin);
        let (w, h) = ((client.right - client.left).max(1), (client.bottom - client.top).max(1));
        SetWindowPos(
            layer,
            std::ptr::null_mut(),
            origin.x,
            origin.y,
            w,
            h,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
        (w, h)
    }
}

/// `layer` put right over its owner in the windows' order: under whatever was over the
/// owner (made, it's at the top of all windows: over other programs' too, until the owner
/// is next brought forward).
unsafe fn over_owner(layer: HWND, owner: HWND) {
    // SAFETY: both windows alive.
    unsafe {
        let above = GetWindow(owner, GW_HWNDPREV);
        if above == layer {
            return;
        }
        let after = if above.is_null() { HWND_TOP } else { above };
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        SetWindowPos(layer, after, 0, 0, 0, 0, flags);
    }
}

/// The owner's messages, watched: moved or resized, the layer goes with it at once; hidden,
/// it's hidden (shown again by the next frame); disabled (a dialog of its own over it), the
/// layer takes no input either; gone, the watch ends.
unsafe extern "system" fn follow_owner(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    layer: usize,
) -> LRESULT {
    // SAFETY: a subclass procedure's call; `layer` is the layer's window, alive while the
    // subclass is (removed before it's destroyed); `lparam` of `WM_WINDOWPOSCHANGED` is its
    // `WINDOWPOS`.
    unsafe {
        let layer = layer as HWND;
        match msg {
            WM_WINDOWPOSCHANGED => {
                let flags = (*(lparam as *const WINDOWPOS)).flags;
                if flags & SWP_HIDEWINDOW != 0 {
                    ShowWindow(layer, SW_HIDE);
                } else if flags & SWP_NOMOVE == 0 || flags & SWP_NOSIZE == 0 {
                    place(layer, hwnd);
                }
            }
            WM_SHOWWINDOW if wparam == 0 => {
                ShowWindow(layer, SW_HIDE);
            }
            WM_ENABLE => {
                EnableWindow(layer, i32::from(wparam != 0));
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(follow_owner), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

/// The window class' name (UTF-16, NUL-ended).
fn class_name() -> Vec<u16> {
    "AppFloatingLayer\0".encode_utf16().collect()
}

/// Its window's messages: nothing painted by GDI (its content is DirectComposition's), the
/// mouse handed to the parent (the app's window) as it came.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: a window procedure's calls, with the message's own arguments.
    unsafe {
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                ValidateRect(hwnd, std::ptr::null());
                0
            }
            WM_NCHITTEST => HTCLIENT as LRESULT,
            WM_MOUSEACTIVATE => MA_NOACTIVATE,
            WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN
            | WM_RBUTTONUP | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP
            | WM_MBUTTONDBLCLK | WM_XBUTTONDOWN | WM_XBUTTONUP | WM_XBUTTONDBLCLK
            | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let parent = owner(hwnd);
                if parent.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                // A press while the app is behind another program's window: the app's to
                // the front, as a click on it would (the layer itself never takes it).
                let pressed = matches!(
                    msg,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                );
                if pressed && GetForegroundWindow() != parent {
                    SetForegroundWindow(parent);
                }
                // Over the owner's client area: the same client coordinates (the wheel's are
                // the screen's anyway).
                SendMessageW(parent, msg, wparam, lparam)
            }
            WM_SETCURSOR => {
                let parent = owner(hwnd);
                if parent.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                SendMessageW(parent, msg, parent as WPARAM, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The layer's window, for its renderer.
#[derive(Debug)]
struct Handles {
    hwnd: isize,
    instance: isize,
}

impl HasWindowHandle for Handles {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.hwnd).ok_or(HandleError::Unavailable)?;
        let mut handle = Win32WindowHandle::new(hwnd);
        handle.hinstance = NonZeroIsize::new(self.instance);
        // SAFETY: the window lives as long as its layer, which outlives its renderer.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for Handles {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: Windows has no display handle to keep alive.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
        })
    }
}

/// The floating layer of a window.
#[derive(Debug)]
pub(crate) struct Floating {
    parent: HWND,
    handles: Arc<Handles>,
    /// Its size as last set (the parent's client area).
    size: Cell<(i32, i32)>,
    /// The shapes last told (a region made only when they change).
    shapes: RefCell<Vec<(Rect, f32)>>,
}

impl Floating {
    /// Over the window `window` (its client area); `None`: its window couldn't be made.
    pub(crate) fn new(window: &winit::window::Window) -> Option<Self> {
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return None };
        let parent = h.hwnd.get() as HWND;
        let name = class_name();
        // SAFETY: the class registered once for the process, with a procedure of the
        // declared type; the window made as a child of the app's window (alive while it is).
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            static REGISTER: Once = Once::new();
            REGISTER.call_once(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    lpszClassName: name.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&class);
            });
            // No redirection surface (as the app's window has none): only DirectComposition
            // draws it, transparent where nothing floats. With one, the system's opaque surface
            // shows under it, over the whole region (the shapes and their shadows' room). A
            // tool window: no button on the taskbar, none in Alt+Tab; never activated.
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                name.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            // Nothing of it until something floats; no system animation when it shows or
            // hides, no corners of the system's rounding.
            SetWindowRgn(hwnd, CreateRectRgn(0, 0, 0, 0), 0);
            let off: i32 = 1;
            let corners = DWMWCP_DONOTROUND;
            let size = std::mem::size_of::<i32>() as u32;
            let attribute = |name: i32, value: *const i32| {
                DwmSetWindowAttribute(hwnd, name as u32, value.cast(), size)
            };
            attribute(DWMWA_TRANSITIONS_FORCEDISABLED, &off);
            attribute(DWMWA_WINDOW_CORNER_PREFERENCE, &corners);
            EnableWindow(hwnd, IsWindowEnabled(parent));
            let (w, h) = place(hwnd, parent);
            over_owner(hwnd, parent);
            // It follows its owner from now on (until it goes).
            SetWindowSubclass(parent, Some(follow_owner), FOLLOW_ID, hwnd as usize);
            Some(Self {
                parent,
                handles: Arc::new(Handles { hwnd: hwnd as isize, instance: instance as isize }),
                size: Cell::new((w, h)),
                shapes: RefCell::default(),
            })
        }
    }

    fn hwnd(&self) -> HWND {
        self.handles.hwnd as HWND
    }

    /// Its size kept the owner's client area's (its place follows the owner's moves by
    /// itself: `follow_owner`).
    fn fit(&self) {
        // SAFETY: both windows alive (the layer is the owner's while it lives).
        unsafe {
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(self.parent, &mut client);
            let size = (client.right - client.left, client.bottom - client.top);
            if size != self.size.get() {
                self.size.set(size);
                place(self.hwnd(), self.parent);
            }
        }
    }
}

impl platform_api::FloatingLayer for Floating {
    fn handles(&self) -> Arc<dyn platform_api::WindowHandles> {
        Arc::clone(&self.handles) as Arc<dyn platform_api::WindowHandles>
    }

    fn set_visible(&self, visible: bool) {
        // SAFETY: the layer's window and its owner, alive.
        unsafe {
            if visible {
                over_owner(self.hwnd(), self.parent);
            }
            ShowWindow(self.hwnd(), if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }

    fn set_shapes(&self, shapes: &[(Rect, f32)], margin: f32) {
        self.fit();
        if *self.shapes.borrow() == shapes {
            return;
        }
        *self.shapes.borrow_mut() = shapes.to_vec();
        // SAFETY: GDI regions made and combined here; the one given to the window is the
        // system's from then on, the others deleted.
        unsafe {
            let region = CreateRectRgn(0, 0, 0, 0);
            if region.is_null() {
                return;
            }
            for (shape, radius) in shapes {
                let x0 = (shape.min_x() - margin).floor() as i32;
                let y0 = (shape.min_y() - margin).floor() as i32;
                let x1 = (shape.max_x() + margin).ceil() as i32;
                let y1 = (shape.max_y() + margin).ceil() as i32;
                let d = ((radius + margin) * 2.0).round().max(0.0) as i32;
                let part = if d > 0 {
                    CreateRoundRectRgn(x0, y0, x1 + 1, y1 + 1, d, d)
                } else {
                    CreateRectRgn(x0, y0, x1, y1)
                };
                if !part.is_null() {
                    CombineRgn(region, region, part, RGN_OR);
                    DeleteObject(part);
                }
            }
            if SetWindowRgn(self.hwnd(), region, 1) == 0 {
                DeleteObject(region);
            }
        }
    }

    /// Owned, it's over its owner and the owner's pages: kept right over the owner (never
    /// raised among all windows: over other programs').
    fn raise(&self) {
        self.fit();
        // SAFETY: the layer's window and its owner, alive.
        unsafe {
            over_owner(self.hwnd(), self.parent);
        }
    }
}

impl Drop for Floating {
    fn drop(&mut self) {
        // SAFETY: the owner's watch removed before the layer's own window is destroyed (once).
        unsafe {
            RemoveWindowSubclass(self.parent, Some(follow_owner), FOLLOW_ID);
            DestroyWindow(self.hwnd());
        }
    }
}
