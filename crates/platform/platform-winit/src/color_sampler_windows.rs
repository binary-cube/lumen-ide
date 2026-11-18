//! The screen eyedropper on Windows (there is no system one): like the macOS loupe.
//!
//! The whole virtual screen (every monitor) is captured once; a borderless topmost window
//! covering it shows that capture (so nothing visibly changes) with a magnifier next to
//! the cursor: the pixels around it enlarged, the one under the cursor framed, and the
//! color's hex value below. A click picks the pixel, Esc or a right click cancels.
//!
//! The window lives on the main thread; its messages are dispatched by winit's loop.

use std::cell::RefCell;

use kernel::Rgba;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CAPTUREBLT, COLORONCOLOR, CreateCompatibleBitmap, CreateCompatibleDC,
    CreatePen, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, FillRect, GetDC, GetPixel, GetStockObject, HBITMAP, HDC, HGDIOBJ,
    InvalidateRect, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, Rectangle, ReleaseDC, SRCCOPY, SelectObject,
    SetBkMode, SetStretchBltMode, SetTextColor, StretchBlt, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos,
    GetSystemMetrics, IDC_CROSS, LoadCursorW, RegisterClassExW, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, SetForegroundWindow,
    ShowWindow, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::color_sampler::Done;

/// Pixels around the cursor shown in the loupe (odd: the cursor's pixel is the center).
const GRID: i32 = 11;
/// The loupe's size on screen.
const LOUPE: i32 = 132;
/// The hex label below the loupe.
const LABEL: i32 = 26;

/// The running eyedropper (one at a time, on the main thread).
struct State {
    window: HWND,
    /// The captured screen, selected into `dc`.
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    /// The cursor, in window (capture) coordinates.
    cursor: POINT,
    done: Option<Done>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

#[allow(unsafe_code)]
pub(crate) fn sample(done: Done) -> bool {
    if STATE.with(|s| s.borrow().is_some()) {
        return false;
    }
    // SAFETY: plain Win32 calls on the main thread; every handle created here is released
    // in `finish` (or right away if creating the window fails).
    unsafe {
        let origin = POINT {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
        };
        let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        // The screen, captured once (layered windows included).
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bitmap as HGDIOBJ);
        BitBlt(dc, 0, 0, w, h, screen, origin.x, origin.y, SRCCOPY | CAPTUREBLT);
        ReleaseDC(std::ptr::null_mut(), screen);

        let instance = GetModuleHandleW(std::ptr::null());
        let class: Vec<u16> =
            format!("{}Eyedropper\0", kernel::brand::NAME).encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_CROSS),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        // Registering twice fails harmlessly (the class stays registered).
        RegisterClassExW(&wc);
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            origin.x,
            origin.y,
            w,
            h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if window.is_null() {
            SelectObject(dc, old);
            DeleteObject(bitmap as HGDIOBJ);
            DeleteDC(dc);
            return false;
        }
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        let cursor = POINT { x: cursor.x - origin.x, y: cursor.y - origin.y };
        STATE.with(|s| {
            *s.borrow_mut() = Some(State { window, dc, bitmap, old, cursor, done: Some(done) });
        });
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        SetFocus(window);
    }
    true
}

/// Ends the eyedropper with `color` (`None`: cancelled) and frees everything.
#[allow(unsafe_code)]
fn finish(color: Option<Rgba>) {
    let Some(mut state) = STATE.with(|s| s.borrow_mut().take()) else { return };
    // SAFETY: the handles were created in `sample` and are released once, here.
    unsafe {
        DestroyWindow(state.window);
        SelectObject(state.dc, state.old);
        DeleteObject(state.bitmap as HGDIOBJ);
        DeleteDC(state.dc);
    }
    if let Some(done) = state.done.take() {
        done(color);
    }
}

/// The captured pixel at `p` (window coordinates).
#[allow(unsafe_code)]
fn pixel(dc: HDC, p: POINT) -> Rgba {
    // SAFETY: `dc` is the live capture DC; GetPixel only reads it.
    let c = unsafe { GetPixel(dc, p.x, p.y) };
    let channel = |shift: u32| ((c >> shift) & 0xff) as f32 / 255.0;
    Rgba::new(channel(0), channel(8), channel(16), 1.0)
}

/// Where the loupe goes for the cursor at `c`: below-right, flipped near the edges.
fn loupe_rect(c: POINT, width: i32, height: i32) -> RECT {
    let total = LOUPE + LABEL;
    let x = if c.x + 24 + LOUPE > width { c.x - 24 - LOUPE } else { c.x + 24 };
    let y = if c.y + 24 + total > height { c.y - 24 - total } else { c.y + 24 };
    RECT { left: x, top: y, right: x + LOUPE, bottom: y + total }
}

#[allow(unsafe_code)]
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY (whole function): called by Windows on the main thread for our window; all
    // calls use handles owned by `STATE`.
    unsafe {
        let point = || POINT {
            x: (lparam & 0xffff) as i16 as i32,
            y: ((lparam >> 16) & 0xffff) as i16 as i32,
        };
        match message {
            WM_MOUSEMOVE => {
                let (w, h) =
                    (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
                let moved = STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let state = s.as_mut()?;
                    let before = loupe_rect(state.cursor, w, h);
                    state.cursor = point();
                    Some((before, loupe_rect(state.cursor, w, h)))
                });
                if let Some((before, after)) = moved {
                    InvalidateRect(window, &before, 0);
                    InvalidateRect(window, &after, 0);
                }
                0
            }
            WM_LBUTTONDOWN => {
                let color = STATE.with(|s| s.borrow().as_ref().map(|st| pixel(st.dc, point())));
                finish(color);
                0
            }
            WM_RBUTTONDOWN => {
                finish(None);
                0
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
                finish(None);
                0
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(window, &mut ps);
                STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        paint(hdc, &ps.rcPaint, state);
                    }
                });
                EndPaint(window, &ps);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

/// The capture for the invalid area, then the loupe on top (into a back buffer, so
/// moving the loupe doesn't flicker).
#[allow(unsafe_code)]
unsafe fn paint(hdc: HDC, area: &RECT, state: &State) {
    // SAFETY: GDI drawing on DCs owned by this call or by `state`.
    unsafe {
        let (w, h) = (area.right - area.left, area.bottom - area.top);
        if w <= 0 || h <= 0 {
            return;
        }
        let back = CreateCompatibleDC(hdc);
        let bitmap = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(back, bitmap as HGDIOBJ);
        BitBlt(back, 0, 0, w, h, state.dc, area.left, area.top, SRCCOPY);

        let (sw, sh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        let r = loupe_rect(state.cursor, sw, sh);
        let (lx, ly) = (r.left - area.left, r.top - area.top);
        // The magnified pixels around the cursor.
        SetStretchBltMode(back, COLORONCOLOR);
        let half = GRID / 2;
        StretchBlt(
            back,
            lx,
            ly,
            LOUPE,
            LOUPE,
            state.dc,
            state.cursor.x - half,
            state.cursor.y - half,
            GRID,
            GRID,
            SRCCOPY,
        );
        let cell = LOUPE / GRID;
        let null_brush = GetStockObject(NULL_BRUSH);
        let old_brush = SelectObject(back, null_brush);
        // The loupe's frame and the cursor's pixel (white over black: visible on anything).
        for (color, inset) in [(0x0000_0000u32, 0), (0x00ff_ffffu32, 1)] {
            let pen = CreatePen(PS_SOLID, 1, color);
            let old_pen = SelectObject(back, pen as HGDIOBJ);
            Rectangle(back, lx + inset, ly + inset, lx + LOUPE - inset, ly + LOUPE - inset);
            let c = lx + half * cell;
            let m = ly + half * cell;
            Rectangle(back, c - inset, m - inset, c + cell + inset, m + cell + inset);
            SelectObject(back, old_pen);
            DeleteObject(pen as HGDIOBJ);
        }
        SelectObject(back, old_brush);
        // The color's hex value, on a dark strip below.
        let color = pixel(state.dc, state.cursor);
        let hex = color.to_hex();
        let mut label =
            RECT { left: lx, top: ly + LOUPE, right: lx + LOUPE, bottom: ly + LOUPE + LABEL };
        let brush = CreateSolidBrush(0x0026_2320);
        FillRect(back, &label, brush);
        DeleteObject(brush as HGDIOBJ);
        SetBkMode(back, TRANSPARENT as i32);
        SetTextColor(back, 0x00ff_ffff);
        let text: Vec<u16> = hex.encode_utf16().collect();
        DrawTextW(
            back,
            text.as_ptr(),
            text.len() as i32,
            &mut label,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        BitBlt(hdc, area.left, area.top, w, h, back, 0, 0, SRCCOPY);
        SelectObject(back, old);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(back);
    }
}
