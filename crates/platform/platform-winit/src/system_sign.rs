//! The app's sign outside its windows (macOS): an item in the menu bar, by the clock — its
//! icon a template (the system draws it in the bar's color), its tooltip, its menu (the
//! rows' ids come back as commands, through muda's events) — and the sign's mark over the
//! Dock's icon (the same icon as in the app, not a character: the Dock's own badge is text
//! only), drawn by a view of the Dock's tile. Made when the sign comes, removed when it goes;
//! nothing kept between.

use std::cell::RefCell;

use muda::ContextMenu;
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage, NSImageView, NSMenu,
    NSStatusBar, NSStatusItem, NSVariableStatusItemLength, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use platform_api::{MenuIcon, SystemSign};

/// The menu bar's item and the menu it shows (kept alive here: the item doesn't own the
/// menu muda made).
struct Shown {
    item: Retained<NSStatusItem>,
    _menu: muda::Menu,
}

thread_local! {
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// Shows `sign` (replacing the one shown), or removes it (`None`).
pub(crate) fn set(sign: Option<&SystemSign>) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    mark_dock(&app, sign.map(|s| &s.badge), mtm);
    let Some(sign) = sign else {
        if let Some(shown) = SHOWN.with(|s| s.borrow_mut().take()) {
            NSStatusBar::systemStatusBar().removeStatusItem(&shown.item);
        }
        return;
    };
    let menu = match crate::context_menu::build(&sign.menu) {
        Ok(menu) => menu,
        Err(e) => {
            tracing::warn!("can't make the menu bar item's menu: {e}");
            return;
        }
    };
    let item = SHOWN.with(|s| s.borrow_mut().take()).map_or_else(
        || NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength),
        |shown| shown.item,
    );
    if let Some(button) = item.button(mtm) {
        if let Some(image) = image(&sign.icon, true) {
            button.setImage(Some(&image));
        }
        button.setToolTip(Some(&NSString::from_str(&sign.tooltip)));
    }
    // SAFETY: muda's menu is an `NSMenu` it keeps alive (held in `Shown`, next to the item
    // showing it); retaining it here only adds a reference.
    #[allow(unsafe_code)]
    let ns_menu = unsafe { Retained::retain(menu.ns_menu().cast::<NSMenu>()) };
    item.setMenu(ns_menu.as_deref());
    SHOWN.with(|s| *s.borrow_mut() = Some(Shown { item, _menu: menu }));
}

/// The Dock's tile: the app's icon with `badge` over its top right corner, or (`None`) the
/// icon alone again.
fn mark_dock(app: &NSApplication, badge: Option<&MenuIcon>, mtm: MainThreadMarker) {
    let tile = app.dockTile();
    let Some(badge) = badge.and_then(|b| image(b, false)) else {
        tile.setContentView(None);
        tile.display();
        return;
    };
    let size = tile.size();
    let whole = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let view = NSView::initWithFrame(NSView::alloc(mtm), whole);
    if let Some(icon) = app.applicationIconImage() {
        let art = NSImageView::imageViewWithImage(&icon, mtm);
        art.setFrame(whole);
        view.addSubview(&art);
    }
    // About two fifths of the tile, in its top right corner (the Dock's badges' place).
    let side = size.width * 0.42;
    let corner =
        NSRect::new(NSPoint::new(size.width - side, size.height - side), NSSize::new(side, side));
    let mark = NSImageView::imageViewWithImage(&badge, mtm);
    mark.setFrame(corner);
    view.addSubview(&mark);
    tile.setContentView(Some(&view));
    tile.display();
}

/// `icon` (RGBA, at 2×) as an image of half its size in points: a template (drawn by the
/// system in the menu bar's color) or as it is.
fn image(icon: &MenuIcon, template: bool) -> Option<Retained<NSImage>> {
    let image = image_of(icon)?;
    image.setTemplate(template);
    Some(image)
}

/// `icon` (RGBA, at 2×) as an image of half its size in points.
fn image_of(icon: &MenuIcon) -> Option<Retained<NSImage>> {
    let (width, height) = (icon.width as isize, icon.height as isize);
    if icon.rgba.len() != (icon.width * icon.height * 4) as usize {
        return None;
    }
    // SAFETY: no planes given: the rep allocates its own buffer, of `height` rows of
    // `width * 4` bytes, which is what is copied into it below.
    #[allow(unsafe_code)]
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            width,
            height,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            width * 4,
            32,
        )
    }?;
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    // The rep wants its colors premultiplied by alpha.
    let premultiplied: Vec<u8> = icon
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u16::from(p[3]);
            let mul = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
            [mul(p[0]), mul(p[1]), mul(p[2]), p[3]]
        })
        .collect();
    // SAFETY: `data` is the rep's own buffer of exactly this many bytes (asked above).
    #[allow(unsafe_code)]
    unsafe {
        std::ptr::copy_nonoverlapping(premultiplied.as_ptr(), data, premultiplied.len());
    }
    let points = NSSize::new(icon.width as f64 / 2.0, icon.height as f64 / 2.0);
    rep.setSize(points);
    let image = NSImage::initWithSize(NSImage::alloc(), points);
    image.addRepresentation(&rep);
    Some(image)
}
