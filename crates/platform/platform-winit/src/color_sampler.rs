//! The screen eyedropper: pick a color anywhere on screen, with a magnifier.
//!
//! - macOS: the system's `NSColorSampler` (the native loupe; no screen-recording
//!   permission needed).
//! - Windows: see `windows.rs` — a transparent full-screen layer catches the mouse and a
//!   loupe window next to the cursor shows the magnified pixels; click picks, Esc cancels.
//! - elsewhere: not available (the app hides the eyedropper).

use kernel::Rgba;

/// Called once with the picked color (`None`: cancelled).
pub(crate) type Done = Box<dyn Fn(Option<Rgba>) + Send + 'static>;

#[cfg(target_os = "macos")]
pub(crate) const SUPPORTED: bool = true;
#[cfg(target_os = "windows")]
pub(crate) const SUPPORTED: bool = true;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) const SUPPORTED: bool = false;

/// Starts the eyedropper; `done` gets the result (on the main thread). `false` if it
/// couldn't start.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn sample(done: Done) -> bool {
    use block2::RcBlock;
    use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};

    let sampler = NSColorSampler::new();
    let handler = RcBlock::new(move |color: *mut NSColor| {
        // SAFETY: AppKit passes a valid (autoreleased) NSColor, or nil when cancelled; it
        // is only read during this call.
        let color = unsafe { color.as_ref() };
        let rgba =
            color.and_then(|c| c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())).map(|c| {
                Rgba::new(
                    c.redComponent() as f32,
                    c.greenComponent() as f32,
                    c.blueComponent() as f32,
                    c.alphaComponent() as f32,
                )
            });
        done(rgba);
    });
    // SAFETY: called on the main thread (from the event loop); the sampler retains itself
    // until the session ends and calls the handler on the main thread.
    unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
    true
}

#[cfg(target_os = "windows")]
pub(crate) fn sample(done: Done) -> bool {
    crate::color_sampler_windows::sample(done)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn sample(done: Done) -> bool {
    drop(done);
    false
}
