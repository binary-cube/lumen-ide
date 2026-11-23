//! The floating layer over the pages (Windows): a child window of its own, over the
//! window's pages (the system's web views, child windows too), that the app draws what
//! floats into — through DirectComposition only (no redirection surface of the system's
//! under it), transparent where nothing is drawn. Its region
//! is where something floats (with room for its shadow): only there is it drawn and does it
//! take the mouse, which it hands to the app's window as if it were there (it sits at the
//! window's origin, so its coordinates are the window's).

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::num::NonZeroIsize;
use std::sync::{Arc, Once};

use kernel::Rect;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, RGN_OR, SetWindowRgn, ValidateRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GW_CHILD, GetClientRect, GetParent, GetWindow,
    HTCLIENT, HWND_TOP, RegisterClassW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SendMessageW, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_PAINT, WM_RBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_NOREDIRECTIONBITMAP,
};

/// `MA_NOACTIVATE`: a click on it doesn't activate anything.
const MA_NOACTIVATE: LRESULT = 3;

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
                let parent = GetParent(hwnd);
                if parent.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                // At the parent's origin: the same client coordinates (the wheel's are the
                // screen's anyway).
                SendMessageW(parent, msg, wparam, lparam)
            }
            WM_SETCURSOR => {
                let parent = GetParent(hwnd);
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
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(parent, &mut client);
            let (w, h) = (client.right - client.left, client.bottom - client.top);
            // No redirection surface (as the app's window has none): only DirectComposition
            // draws it, transparent where nothing floats. With one, the system's opaque surface
            // shows under it, over the whole region (the shapes and their shadows' room).
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                name.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                w.max(1),
                h.max(1),
                parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            // Nothing of it until something floats.
            SetWindowRgn(hwnd, CreateRectRgn(0, 0, 0, 0), 0);
            SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
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

    /// Its size kept the parent's client area's.
    fn fit(&self) {
        // SAFETY: both windows alive (the layer's is the parent's child).
        unsafe {
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(self.parent, &mut client);
            let size = (client.right - client.left, client.bottom - client.top);
            if size != self.size.get() {
                self.size.set(size);
                SetWindowPos(
                    self.hwnd(),
                    std::ptr::null_mut(),
                    0,
                    0,
                    size.0.max(1),
                    size.1.max(1),
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
    }
}

impl platform_api::FloatingLayer for Floating {
    fn handles(&self) -> Arc<dyn platform_api::WindowHandles> {
        Arc::clone(&self.handles) as Arc<dyn platform_api::WindowHandles>
    }

    fn set_visible(&self, visible: bool) {
        // SAFETY: the layer's window, alive.
        unsafe {
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

    fn raise(&self) {
        self.fit();
        // SAFETY: both windows alive; the topmost child is the first of the parent's.
        unsafe {
            if GetWindow(self.parent, GW_CHILD) != self.hwnd() {
                SetWindowPos(
                    self.hwnd(),
                    HWND_TOP,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
}

impl Drop for Floating {
    fn drop(&mut self) {
        // SAFETY: the layer's own window, destroyed once.
        unsafe {
            DestroyWindow(self.hwnd());
        }
    }
}
