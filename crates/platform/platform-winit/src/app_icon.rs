//! The app's icon where the system doesn't find it by itself: the windows' (title bar,
//! taskbar, Alt+Tab) on Windows and Linux, the Dock's on macOS when run outside the bundle.
//!
//! - Windows: the executable's icon resource (number 1, embedded by the app's `build.rs`),
//!   at the system's sizes for small and large icons; nothing decoded by us.
//! - Linux: the PNG given in `WindowOptions::icon`, decoded for each new window (opening
//!   windows is rare) and handed to winit — nothing kept.
//! - macOS: windows have no icons; the Dock shows the bundle's. Run without one (`cargo
//!   run`), the PNG becomes the Dock's icon once, at startup.

use winit::window::WindowAttributes;

/// The executable's icon resource (`crates/app/build.rs`).
#[cfg(target_os = "windows")]
const ICON_RESOURCE: u16 = 1;

/// The window's attributes with the app's icon.
#[cfg(target_os = "windows")]
pub(crate) fn with_icon(attributes: WindowAttributes, _png: Option<&[u8]>) -> WindowAttributes {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXICON, SM_CXSMICON};
    use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
    use winit::window::Icon;

    let load = |metric| {
        // SAFETY: GetSystemMetrics only reads a system value.
        #[allow(unsafe_code)]
        let side = u32::try_from(unsafe { GetSystemMetrics(metric) }).ok().filter(|&s| s > 0);
        Icon::from_resource(ICON_RESOURCE, side.map(|s| winit::dpi::PhysicalSize::new(s, s)))
            .inspect_err(|e| tracing::debug!(error = %e, "no app icon in the executable"))
            .ok()
    };
    match load(SM_CXSMICON) {
        Some(small) => attributes.with_window_icon(Some(small)).with_taskbar_icon(load(SM_CXICON)),
        None => attributes,
    }
}

/// The window's attributes with the app's icon.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn with_icon(attributes: WindowAttributes, png: Option<&[u8]>) -> WindowAttributes {
    let icon = png.and_then(|png| {
        let (rgba, width, height) = decode(png)
            .inspect_err(|e| tracing::warn!(error = %e, "can't decode the app icon"))
            .ok()?;
        winit::window::Icon::from_rgba(rgba, width, height)
            .inspect_err(|e| tracing::warn!(error = %e, "can't use the app icon"))
            .ok()
    });
    match icon {
        Some(icon) => attributes.with_window_icon(Some(icon)),
        None => attributes,
    }
}

/// A PNG's pixels, as 8-bit RGBA.
#[cfg(all(unix, not(target_os = "macos")))]
fn decode(png: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("too large")?];
    let frame = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    pixels.truncate(frame.buffer_size());
    // `cargo xtask icon` writes RGBA.
    if frame.color_type != png::ColorType::Rgba {
        return Err(format!("not RGBA: {:?}", frame.color_type));
    }
    Ok((pixels, frame.width, frame.height))
}

/// The window's attributes (macOS: windows have no icons).
#[cfg(target_os = "macos")]
pub(crate) fn with_icon(attributes: WindowAttributes, _png: Option<&[u8]>) -> WindowAttributes {
    attributes
}

/// The Dock's icon while the app runs: the round art itself, in the bundle too. The bundle's
/// `.icns` must be a full squircle for macOS 26 (anything else is put on a light tile of its
/// own), so it shows the art on black; set at run time the icon keeps its own round shape —
/// the Dock shows it so while the app runs (Finder and the Dock before launch show the
/// `.icns`).
#[cfg(target_os = "macos")]
pub(crate) fn set_dock_icon(png: Option<&[u8]>) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let (Some(png), Some(mtm)) = (png, MainThreadMarker::new()) else {
        return;
    };
    let data = NSData::with_bytes(png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        tracing::warn!("can't decode the app icon");
        return;
    };
    // SAFETY: on the main thread (checked above), with a valid image.
    #[allow(unsafe_code)]
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image))
    };
}
