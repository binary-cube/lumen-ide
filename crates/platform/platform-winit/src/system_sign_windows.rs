//! The app's sign outside its windows (Windows): an icon in the notification area, by the
//! clock — its tooltip, a menu on a click (the rows' ids come back as commands, through
//! muda's events) — and an overlay on the app's taskbar buttons. The icon is made when the
//! sign comes and destroyed when it goes (the shell keeps copies of its own); the hidden
//! window that hears the clicks is made once, the first time a sign shows.

use std::cell::RefCell;
use std::ffi::c_void;

use muda::ContextMenu;
use platform_api::{MenuIcon, SystemSign};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIcon, CreateWindowExW, DefWindowProcW, DestroyIcon, HICON, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

/// The message the notification area sends the hidden window (a click on the icon).
const CLICKED: u32 = WM_APP + 0x51;
/// The icon's number among the app's (it has one).
const ICON_ID: u32 = 1;

/// What is shown now.
struct State {
    /// The hidden window hearing the icon's clicks (made once).
    hwnd: HWND,
    /// The shell's "the taskbar was made again" message (Explorer restarted): the icon is
    /// added again.
    taskbar_created: u32,
    /// The icon shown (null: none).
    icon: HICON,
    /// The taskbar buttons' overlay: the sign's mark (null: none).
    mark: HICON,
    tooltip: String,
    menu: Option<muda::Menu>,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            hwnd: std::ptr::null_mut(),
            taskbar_created: 0,
            icon: std::ptr::null_mut(),
            mark: std::ptr::null_mut(),
            tooltip: String::new(),
            menu: None,
        })
    };
}

/// Shows `sign` (replacing the one shown) on the notification area and on `windows`'
/// taskbar buttons, or removes it (`None`).
pub(crate) fn set<'a>(
    sign: Option<&SystemSign>,
    windows: impl Iterator<Item = &'a winit::window::Window>,
) {
    let windows: Vec<HWND> = windows.filter_map(hwnd_of).collect();
    let (old, old_mark) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        (
            std::mem::replace(&mut s.icon, std::ptr::null_mut()),
            std::mem::replace(&mut s.mark, std::ptr::null_mut()),
        )
    });
    match sign {
        Some(sign) => {
            let Some(hwnd) = hidden_window() else { return };
            let icon = icon(&sign.icon);
            // The taskbar buttons' overlay: the sign's mark (the same icon, on its disc).
            let mark = self::icon(&sign.badge);
            let menu = crate::context_menu::build(&sign.menu)
                .inspect_err(
                    |e| tracing::warn!(error = %e, "can't make the notification icon's menu"),
                )
                .ok();
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.icon = icon;
                s.mark = mark;
                s.tooltip.clone_from(&sign.tooltip);
                s.menu = menu;
            });
            notify(if old.is_null() { NIM_ADD } else { NIM_MODIFY }, hwnd);
            for window in &windows {
                overlay(*window, mark, &sign.tooltip);
            }
        }
        None => {
            if !old.is_null() {
                let hwnd = STATE.with(|s| s.borrow().hwnd);
                notify(NIM_DELETE, hwnd);
            }
            for window in windows {
                overlay(window, std::ptr::null_mut(), "");
            }
            STATE.with(|s| s.borrow_mut().menu = None);
        }
    }
    for gone in [old, old_mark].into_iter().filter(|i| !i.is_null()) {
        // SAFETY: an icon this module made; the shell and the taskbar keep copies.
        #[allow(unsafe_code)]
        unsafe {
            DestroyIcon(gone);
        }
    }
}

/// A window opened while a sign shows: its taskbar button gets the overlay too.
pub(crate) fn window_opened(window: &winit::window::Window) {
    let (mark, tooltip) = STATE.with(|s| {
        let s = s.borrow();
        (s.mark, s.tooltip.clone())
    });
    if let (false, Some(hwnd)) = (mark.is_null(), hwnd_of(window)) {
        overlay(hwnd, mark, &tooltip);
    }
}

fn hwnd_of(window: &winit::window::Window) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The hidden window hearing the icon's clicks: a top-level one never shown (a menu needs a
/// window it can bring to the front), made the first time.
fn hidden_window() -> Option<HWND> {
    let made = STATE.with(|s| s.borrow().hwnd);
    if !made.is_null() {
        return Some(made);
    }
    let class = wide("system-sign");
    // SAFETY: a window class of our own, registered once on this (the UI) thread, with its
    // procedure below; the window is ours, never shown, alive as long as the app.
    #[allow(unsafe_code)]
    let (hwnd, taskbar_created) = unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(procedure);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        (hwnd, RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()))
    };
    if hwnd.is_null() {
        tracing::warn!("can't make the notification icon's window");
        return None;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.hwnd = hwnd;
        s.taskbar_created = taskbar_created;
    });
    Some(hwnd)
}

/// The hidden window's messages: a click on the icon shows its menu; the taskbar made again
/// gets the icon again.
extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = STATE.with(|s| s.borrow().taskbar_created);
    if message == CLICKED {
        let event = (lparam & 0xffff) as u32;
        if matches!(event, WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU) {
            show_menu(hwnd);
        }
        return 0;
    }
    if taskbar_created != 0 && message == taskbar_created {
        if !STATE.with(|s| s.borrow().icon.is_null()) {
            notify(NIM_ADD, hwnd);
        }
        return 0;
    }
    // SAFETY: the system's default handling of the window's other messages.
    #[allow(unsafe_code)]
    unsafe {
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

/// The icon's menu, at the cursor (the hidden window in front first, so a click elsewhere
/// closes it).
fn show_menu(hwnd: HWND) {
    let menu = STATE.with(|s| s.borrow().menu.clone());
    let Some(menu) = menu else { return };
    // SAFETY: our own window, on its thread; muda's menu shown synchronously over it.
    #[allow(unsafe_code)]
    unsafe {
        SetForegroundWindow(hwnd);
        menu.show_context_menu_for_hwnd(hwnd as isize, None);
    }
}

/// The notification area told (`action`: add, modify, delete) of the icon as it is now.
fn notify(action: u32, hwnd: HWND) {
    STATE.with(|s| {
        let s = s.borrow();
        // SAFETY: a plain struct of the shell's, zeroed then filled; the call copies it.
        #[allow(unsafe_code)]
        unsafe {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = hwnd;
            data.uID = ICON_ID;
            data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
            data.uCallbackMessage = CLICKED;
            data.hIcon = s.icon;
            for (slot, unit) in data.szTip.iter_mut().zip(s.tooltip.encode_utf16().take(127)) {
                *slot = unit;
            }
            if Shell_NotifyIconW(action, &data) == 0 && action != NIM_DELETE {
                tracing::debug!("the notification area didn't take the icon");
            }
        }
    });
}

/// `icon` (RGBA) as a Windows icon: its colors as they are, its alpha.
fn icon(icon: &MenuIcon) -> HICON {
    let (width, height) = (icon.width as usize, icon.height as usize);
    if icon.rgba.len() != width * height * 4 {
        return std::ptr::null_mut();
    }
    // 32 bits a pixel, blue first; the mask (rows of 16-bit words) all clear: the alpha says.
    let bgra: Vec<u8> =
        icon.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let mask = vec![0u8; width.div_ceil(16) * 2 * height];
    // SAFETY: both buffers have the sizes the call reads for these dimensions.
    #[allow(unsafe_code)]
    unsafe {
        CreateIcon(
            GetModuleHandleW(std::ptr::null()),
            width as i32,
            height as i32,
            1,
            32,
            mask.as_ptr(),
            bgra.as_ptr(),
        )
    }
}

/// The taskbar button of `window` gets `icon` as its overlay (null: none), `tooltip` read
/// to screen readers.
fn overlay(window: HWND, icon: HICON, tooltip: &str) {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::core::HSTRING;
    // SAFETY: COM on this (the UI) thread, initialized here if it isn't yet; the taskbar's
    // interface used as documented, on a window of this process.
    #[allow(unsafe_code)]
    let done = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER).and_then(
            |taskbar| {
                taskbar.HrInit()?;
                taskbar.SetOverlayIcon(
                    windows::Win32::Foundation::HWND(window.cast::<c_void>()),
                    windows::Win32::UI::WindowsAndMessaging::HICON(icon.cast::<c_void>()),
                    &HSTRING::from(tooltip),
                )
            },
        )
    };
    if let Err(e) = done {
        tracing::debug!(error = %e, "the taskbar didn't take the overlay");
    }
}
