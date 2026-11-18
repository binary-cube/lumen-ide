//! Native macOS menu (top bar), built with muda.

use muda::accelerator::Accelerator;
use platform_api::{Menu, MenuItem, NativeItem};
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

/// The menu bar, kept alive while the app runs, and its commands' rows by id (greyed out
/// when they don't apply).
pub(crate) struct NativeMenu {
    _bar: muda::Menu,
    items: Vec<(String, muda::MenuItem)>,
}

impl NativeMenu {
    /// The rows of `disabled` greyed out, every other one enabled.
    pub(crate) fn set_disabled(&self, disabled: &[String]) {
        for (id, enabled) in states(self.items.iter().map(|(id, _)| id.as_str()), disabled) {
            if let Some((_, item)) = self.items.iter().find(|(i, _)| i == id) {
                item.set_enabled(enabled);
            }
        }
    }
}

/// Each row's state: enabled unless it's in `disabled`.
fn states<'a>(ids: impl Iterator<Item = &'a str>, disabled: &[String]) -> Vec<(&'a str, bool)> {
    ids.map(|id| (id, !disabled.iter().any(|d| d == id))).collect()
}

/// Creates the menu bar and attaches it to the app. Commands reach the event
/// loop as [`UserEvent::Command`].
pub(crate) fn install(
    menus: &[Menu],
    proxy: EventLoopProxy<UserEvent>,
) -> Result<NativeMenu, muda::Error> {
    let bar = muda::Menu::new();
    let mut items = Vec::new();
    let mut windows = None;
    let mut help = None;
    for menu in menus {
        let submenu = muda::Submenu::new(&menu.title, true);
        fill(&submenu, &menu.items, &mut items)?;
        bar.append(&submenu)?;
        if menu.windows {
            windows = Some(submenu);
        } else if menu.help {
            help = Some(submenu);
        }
    }
    bar.init_for_nsapp();
    // The window list (after the menu bar is set: setting it resets the list): the open
    // windows go in it, and in the Dock's menu.
    if let Some(windows) = windows {
        windows.set_as_windows_menu_for_nsapp();
    }
    // The Help menu: the system puts its search over every menu's rows in it.
    if let Some(help) = help {
        help.set_as_help_menu_for_nsapp();
    }
    crate::context_menu::forward_events(proxy);
    Ok(NativeMenu { _bar: bar, items })
}

/// Appends `items` to `submenu`, their submenus made the same way; the commands' rows kept
/// in `rows`.
fn fill(
    submenu: &muda::Submenu,
    items: &[MenuItem],
    rows: &mut Vec<(String, muda::MenuItem)>,
) -> Result<(), muda::Error> {
    for item in items {
        match item {
            MenuItem::Command { id, label, shortcut, .. } => {
                let accelerator = shortcut.as_deref().and_then(|s| {
                    s.parse::<Accelerator>()
                        .inspect_err(|e| tracing::warn!("invalid shortcut `{s}`: {e}"))
                        .ok()
                });
                let item = muda::MenuItem::with_id(id.as_str(), label, true, accelerator);
                submenu.append(&item)?;
                rows.push((id.clone(), item));
            }
            MenuItem::Separator => submenu.append(&muda::PredefinedMenuItem::separator())?,
            MenuItem::Submenu { title, items, .. } => {
                let inner = muda::Submenu::new(title, true);
                fill(&inner, items, rows)?;
                submenu.append(&inner)?;
            }
            // The system's own rows (what they do, their keys), with the app's labels.
            MenuItem::Native { item, label } => {
                let text = Some(label.as_str());
                submenu.append(&match item {
                    NativeItem::Services => muda::PredefinedMenuItem::services(text),
                    NativeItem::Hide => muda::PredefinedMenuItem::hide(text),
                    NativeItem::HideOthers => muda::PredefinedMenuItem::hide_others(text),
                    NativeItem::ShowAll => muda::PredefinedMenuItem::show_all(text),
                    NativeItem::Minimize => muda::PredefinedMenuItem::minimize(text),
                    NativeItem::Zoom => muda::PredefinedMenuItem::maximize(text),
                    NativeItem::BringAllToFront => {
                        muda::PredefinedMenuItem::bring_all_to_front(text)
                    }
                })?;
            }
        }
    }
    Ok(())
}

/// The menu bar about to show (its tracking began: a click on it, its keys) reaches the
/// loop as [`UserEvent::MenuOpening`] — the app reads its rows' states again then, so a row
/// whose module's own state changed without the window's (Revert Change after typing) is
/// right when the menu shows. Only the menu bar's (not a context menu's); observed once,
/// for the app's lifetime. The loop's events arrive while the menu tracks (its source is
/// in the run loop's common modes), and a row's state changes while the menu is open.
#[allow(unsafe_code)]
pub(crate) fn observe_opening(proxy: EventLoopProxy<UserEvent>) {
    use std::ptr::NonNull;
    use std::sync::Mutex;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSMenuDidBeginTrackingNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let proxy = Mutex::new(proxy);
    let block = block2::RcBlock::new(move |notification: NonNull<NSNotification>| {
        let Some(mtm) = MainThreadMarker::new() else { return };
        // SAFETY: the notification center hands a valid notification for the call.
        let notification = unsafe { notification.as_ref() };
        let tracked = notification.object();
        let bar = NSApplication::sharedApplication(mtm).mainMenu();
        let is_bar = match (&tracked, &bar) {
            (Some(tracked), Some(bar)) => {
                std::ptr::eq(&**tracked as *const _ as *const u8, &**bar as *const _ as *const u8)
            }
            _ => false,
        };
        if is_bar && let Ok(proxy) = proxy.lock() {
            let _ = proxy.send_event(UserEvent::MenuOpening);
        }
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: a notification name AppKit defines; no object filter, the main queue's
    // thread (no queue: the posting thread, AppKit's main one).
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSMenuDidBeginTrackingNotification),
            None,
            None,
            &block,
        )
    };
    // Observed for the app's lifetime.
    std::mem::forget(token);
}

#[cfg(test)]
#[path = "../tests/unit/menu.rs"]
mod tests;
