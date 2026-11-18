//! Native context menu (muda): NSMenu on macOS, Win32 menu on Windows.

use std::sync::Mutex;

use kernel::Point;
use muda::ContextMenu;
use muda::accelerator::Accelerator;
use platform_api::MenuItem;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

/// Sends menu selections (top bar and context menus) to the event loop.
pub(crate) fn forward_events(proxy: EventLoopProxy<UserEvent>) {
    let proxy = Mutex::new(proxy);
    muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
        if let Ok(proxy) = proxy.lock() {
            let _ = proxy.send_event(UserEvent::Command(event.id.0));
        }
    }));
}

/// Builds the menu from the given items (shortcuts are display-only).
pub(crate) fn build(items: &[MenuItem]) -> Result<muda::Menu, muda::Error> {
    let menu = muda::Menu::new();
    fill(&|item| menu.append(item), items)?;
    Ok(menu)
}

/// Appends `items` with `append` (a menu's or a submenu's), submenus made the same way;
/// the system's own rows (the menu bar's) are left out.
fn fill(
    append: &dyn Fn(&dyn muda::IsMenuItem) -> Result<(), muda::Error>,
    items: &[MenuItem],
) -> Result<(), muda::Error> {
    for item in items {
        match item {
            MenuItem::Command { id, label, shortcut, icon } => {
                let accelerator = shortcut.as_deref().and_then(|s| s.parse::<Accelerator>().ok());
                let icon = icon
                    .as_ref()
                    .and_then(|i| muda::Icon::from_rgba(i.rgba.clone(), i.width, i.height).ok());
                match icon {
                    Some(icon) => {
                        let item = muda::IconMenuItem::with_id(
                            id.as_str(),
                            label,
                            true,
                            None,
                            accelerator,
                        );
                        // macOS: "template" icon — the system tints it like the
                        // native ones (gray, white on the selected row). Windows: as is.
                        #[cfg(target_os = "macos")]
                        item.set_icon_templated(Some(icon));
                        #[cfg(not(target_os = "macos"))]
                        item.set_icon(Some(icon));
                        append(&item)?
                    }
                    None => {
                        append(&muda::MenuItem::with_id(id.as_str(), label, true, accelerator))?
                    }
                }
            }
            MenuItem::Separator => append(&muda::PredefinedMenuItem::separator())?,
            MenuItem::Submenu { title, items, .. } => {
                let submenu = muda::Submenu::new(title, true);
                fill(&|item| submenu.append(item), items)?;
                append(&submenu)?;
            }
            MenuItem::Native { .. } => {}
        }
    }
    Ok(())
}

/// Shows the menu and blocks until it closes. `false` if it couldn't be shown.
pub(crate) fn show(window: &winit::window::Window, items: &[MenuItem], position: Point) -> bool {
    let menu = match build(items) {
        Ok(menu) => menu,
        Err(e) => {
            tracing::warn!("can't create the context menu: {e}");
            return false;
        }
    };
    #[cfg_attr(target_os = "macos", allow(unused_variables))]
    let position = Some(muda::dpi::Position::Physical(muda::dpi::PhysicalPosition::new(
        position.x as i32,
        position.y as i32,
    )));
    let Ok(handle) = window.window_handle() else { return false };
    match handle.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(h) => {
            // SAFETY: `ns_view` comes from the winit window handle, which lives for the duration
            // of the call (we hold `&Window`); the menu is shown synchronously, on the main
            // thread.
            #[allow(unsafe_code)]
            // No position: the menu appears at the cursor. With a position, muda flips the Y axis,
            // but the winit view is already "flipped", so the menu would appear mirrored.
            unsafe {
                menu.show_context_menu_for_nsview(h.ns_view.as_ptr(), None);
            }
            true
        }
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(h) => {
            // SAFETY: `hwnd` is the winit window, valid as long as we hold `&Window`.
            #[allow(unsafe_code)]
            unsafe {
                menu.show_context_menu_for_hwnd(h.hwnd.get(), position);
            }
            true
        }
        _ => false,
    }
}
