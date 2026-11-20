//! The Dock's menu (macOS): under the open windows (which macOS lists itself, from the
//! windows menu), the app's own submenu — the recent projects. Its items send commands
//! like the menu bar's (muda's events). The menu is asked for each time the Dock shows it
//! (`applicationDockMenu:`, added to winit's application delegate).

use std::cell::RefCell;

use muda::ContextMenu;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSApplication;
use platform_api::IconMenu;

thread_local! {
    /// The menu the Dock shows (kept alive here: the Dock gets it without owning it).
    static DOCK: RefCell<Option<muda::Menu>> = const { RefCell::new(None) };
    /// The delegate method is added once.
    static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sets the Dock's menu: the projects as a submenu (none: no submenu).
pub(crate) fn set(menu: &IconMenu) {
    let built = (!menu.projects.is_empty()).then(|| build(menu)).transpose();
    match built {
        Ok(built) => DOCK.with(|d| *d.borrow_mut() = built),
        Err(e) => tracing::warn!(error = %e, "can't make the Dock's menu"),
    }
    if !INSTALLED.with(std::cell::Cell::get) && install() {
        INSTALLED.with(|i| i.set(true));
    }
}

fn build(menu: &IconMenu) -> Result<muda::Menu, muda::Error> {
    let submenu = muda::Submenu::new(&menu.title, true);
    for project in &menu.projects {
        let item = muda::MenuItem::with_id(project.command.as_str(), &project.label, true, None);
        submenu.append(&item)?;
    }
    let root = muda::Menu::new();
    root.append(&submenu)?;
    Ok(root)
}

/// Puts `window` in the app's window list (and the Dock's menu) under its title, or updates
/// it: at its opening, when its title changes, when the list is made again.
pub(crate) fn list_window(window: &winit::window::Window) {
    let (Some(mtm), Some(ns_window)) =
        (MainThreadMarker::new(), crate::titlebar::ns_window(window))
    else {
        return;
    };
    let title = ns_window.title();
    // Added, or its title updated (AppKit doesn't list winit's windows by itself).
    NSApplication::sharedApplication(mtm)
        .changeWindowsItem_title_filename(&ns_window, &title, false);
}

/// `applicationDockMenu:` on the application's delegate (winit's): the menu above.
extern "C-unwind" fn dock_menu(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
) -> *mut AnyObject {
    DOCK.with(|d| d.borrow().as_ref().map_or(std::ptr::null_mut(), |m| m.ns_menu().cast()))
}

/// Adds the method to the delegate's class. `false` if there's no delegate yet.
fn install() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else { return false };
    let object: &AnyObject = delegate.as_ref();
    let class: *const AnyClass = object.class();
    // SAFETY: the class is the live delegate's; the function matches the method's type
    // (`-(NSMenu *)applicationDockMenu:(NSApplication *)sender`: object, selector, object →
    // object), which the encoding says.
    #[allow(unsafe_code)]
    let added = unsafe {
        let imp: objc2::runtime::Imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> *mut AnyObject,
            objc2::runtime::Imp,
        >(dock_menu);
        objc2::ffi::class_addMethod(
            class.cast_mut(),
            sel!(applicationDockMenu:),
            imp,
            c"@@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        tracing::warn!("the Dock's menu can't be added (the delegate has one)");
    }
    true
}
