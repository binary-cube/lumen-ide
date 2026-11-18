//! Tweaks to the Metal layer on macOS so that window resizing doesn't produce
//! "ghosts" (doubled text).
//!
//! Without them, during a resize macOS shows the old frame stretched to the new
//! size (`contentsGravity = resize`), and the new frame appears out of sync with
//! the window animation; the two images overlap for a few frames.

use objc2_quartz_core::kCAGravityBottomLeft;

/// Configures the surface's `CAMetalLayer`:
/// - `presentsWithTransaction` off (see [`set_transactional`]: on only while resizing);
/// - `contentsGravity`: old content is no longer stretched to the new size but stays
///   visually anchored to the top-left corner (on macOS the layer's origin is bottom-left,
///   so the matching constant is `BottomLeft`).
#[allow(unsafe_code)]
pub(crate) fn configure_layer(surface: &wgpu::Surface<'_>) {
    // SAFETY: we use the hal handle only to read the layer and set two properties
    // on it; we don't destroy or replace the resource, and the guard is released immediately.
    let Some(hal) = (unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }) else {
        tracing::debug!("the surface doesn't use Metal; the layer stays as is");
        return;
    };
    let layer = hal.render_layer().lock();
    layer.setPresentsWithTransaction(false);
    // SAFETY: `kCAGravityBottomLeft` is an NSString constant exported by QuartzCore, valid
    // for the whole lifetime of the process.
    layer.setContentsGravity(unsafe { kCAGravityBottomLeft });
}

/// `presentsWithTransaction`: the frame is presented in sync with the window's Core
/// Animation transaction (wgpu-hal reads the property every frame and does `commit` +
/// `waitUntilScheduled` + `present`). Needed only while the window is being resized (no
/// stretched frame); otherwise every frame would wait on the main thread.
#[allow(unsafe_code)]
pub(crate) fn set_transactional(surface: &wgpu::Surface<'_>, on: bool) {
    // SAFETY: as in `configure_layer`: one property set on the layer, guard released at once.
    let Some(hal) = (unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }) else { return };
    hal.render_layer().lock().setPresentsWithTransaction(on);
}

/// The layer taken out of the window (its pool emptied first): once the surface holding it
/// goes, so do all its images — the one on screen too, and one presented while the window
/// was covered, which never reaches the screen and is otherwise held until it's shown again.
/// Making the surface smaller isn't enough there: a covered window gets no image to present.
#[allow(unsafe_code)]
pub(crate) fn detach_layer(surface: &wgpu::Surface<'_>) {
    // SAFETY: as in `configure_layer`: the layer read, changed and taken off its parent; the
    // surface still owns it and drops it; the guard is released at once.
    let Some(hal) = (unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }) else { return };
    let layer = hal.render_layer().lock();
    layer.setDrawableSize(objc2_core_foundation::CGSize::new(1.0, 1.0));
    layer.removeFromSuperlayer();
}

/// A new surface's layer put under everything else in the window: the engine adds it on
/// top of the view's layers (`addSublayer`), so a surface made again (a window given its
/// images back) would cover the pages shown over the views (a draw.io tab, a response's
/// preview) — they'd show only what the app drew under them. The first one was alone there.
#[allow(unsafe_code)]
pub(crate) fn lower_layer(surface: &wgpu::Surface<'_>) {
    // SAFETY: as in `configure_layer`: the layer read and moved among its parent's
    // sublayers (to the first place); the guard is released at once.
    let Some(hal) = (unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }) else { return };
    let layer = hal.render_layer().lock();
    if let Some(parent) = layer.superlayer() {
        parent.insertSublayer_atIndex(&layer, 0);
    }
}

/// The layer's images not on screen are given back: a change of size empties its pool, and
/// the image shown stays up until the next frame, which sets the size again.
#[allow(unsafe_code)]
pub(crate) fn release_spare_drawables(surface: &wgpu::Surface<'_>) {
    // SAFETY: as in `configure_layer`: one property set on the layer, guard released at once.
    let Some(hal) = (unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }) else { return };
    hal.render_layer().lock().setDrawableSize(objc2_core_foundation::CGSize::new(1.0, 1.0));
}
