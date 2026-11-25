use super::*;
use crate::Offscreen;
use kernel::{PhysicalSize, Rect, Rgba};
use render_api::{GlyphKey, GlyphRasterizer, ImageData, PixelRect, RasterizedGlyph, Scene};

struct NoGlyphs;

impl GlyphRasterizer for NoGlyphs {
    fn rasterize(&mut self, _key: GlyphKey) -> Option<RasterizedGlyph> {
        None
    }
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d { width, height, depth_or_array_layers: 1 }
}

/// A texture made now is copied whole; a kept one only where the frame changed, cut to the
/// frame; a frame saying nothing (or nothing within it) whole.
#[test]
fn a_frame_is_copied_where_it_changed() {
    let size = extent(100, 50);
    let whole = Areas::of(&[PixelRect::new(10, 10, 20, 20)], size, true);
    assert_eq!(whole.rects(), &[Area { origin: wgpu::Origin3d::ZERO, size }]);
    assert_eq!(whole.copied(size).changed, None);

    let parts =
        Areas::of(&[PixelRect::new(10, 5, 20, 15), PixelRect::new(90, 40, 120, 70)], size, false);
    assert_eq!(
        parts.rects(),
        &[
            Area { origin: wgpu::Origin3d { x: 10, y: 5, z: 0 }, size: extent(10, 10) },
            Area { origin: wgpu::Origin3d { x: 90, y: 40, z: 0 }, size: extent(10, 10) },
        ],
        "cut to the frame"
    );
    let copied = parts.copied(size);
    assert_eq!(copied.changed, Some(PixelRect::new(10, 5, 100, 50)), "their bounds");
    assert_eq!(copied.bytes, 100 * 50 * 4);

    assert_eq!(Areas::of(&[], size, false).copied(size).changed, None, "nothing said");
    let outside = Areas::of(&[PixelRect::new(200, 200, 210, 210)], size, false);
    assert_eq!(outside.copied(size).changed, None, "nothing within it");
}

/// A frame given as pixels changed in one corner: only that is written into the texture
/// kept (the rest stays the frame before), and the image is told where it changed.
#[test]
fn a_frame_given_as_pixels_is_written_where_it_changed() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(8, 4)) else { return };
    let importer = gpu.frame_importer();
    let page = ImageData::live();
    let fill = |bgra: [u8; 4]| bgra.repeat(8 * 4);
    let blue = fill([255, 0, 0, 255]);
    fn frame<'a>(bgra: &'a [u8], changed: &'a [PixelRect]) -> render_api::PixelFrame<'a> {
        render_api::PixelFrame { bgra, size: (8, 4), stride: 8 * 4, changed }
    }
    assert!(importer.write(&page, frame(&blue, &[PixelRect::new(0, 0, 1, 1)])), "the first");
    let first = page.generation();
    // Red everywhere, but said changed only in the top left 2 × 2.
    let red = fill([0, 0, 255, 255]);
    assert!(importer.write(&page, frame(&red, &[PixelRect::new(0, 0, 2, 2)])));
    assert_eq!(page.generation(), first + 1);
    let mut scene = Scene::default();
    scene.reset(Rgba::rgb(0x000000));
    scene.push_layer(None).image(&page, Rect::new(0.0, 0.0, 8.0, 4.0));
    gpu.draw(&scene, &mut NoGlyphs, true);
    let (_, _, pixels) = gpu.pixels().expect("drawn");
    let at = |x: usize, y: usize| &pixels[(y * 8 + x) * 4..][..4];
    assert_eq!(at(0, 0), [255, 0, 0, 255], "the part changed: red");
    assert_eq!(at(5, 3), [0, 0, 255, 255], "the rest: the frame before's blue");
}

/// A live page's texture unshown a while is freed, its frame then lost (its owner asks for
/// a fresh one); one shown stays; one let go (a popup closed) goes at the next frame. Its
/// texture's bytes are counted while it's there.
#[test]
fn a_live_images_texture_goes_once_unshown() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(8, 4)) else { return };
    let importer = gpu.frame_importer();
    let page = ImageData::live();
    let popup = ImageData::live();
    let bgra = [255u8, 0, 0, 255].repeat(8 * 4);
    let frame = render_api::PixelFrame { bgra: &bgra, size: (8, 4), stride: 32, changed: &[] };
    assert!(importer.write(&page, frame));
    assert!(importer.write(&popup, frame));
    assert_eq!(page.gpu_memory(), 8 * 4 * 4, "its texture's bytes");
    let mut scene = Scene::default();
    scene.reset(Rgba::rgb(0x000000));
    scene.push_layer(None).image(&page, Rect::new(0.0, 0.0, 8.0, 4.0));
    gpu.draw(&scene, &mut NoGlyphs, true);
    assert!(popup.has_frame(), "unshown, but not long");
    let idle = gpu.unshown_release_at().expect("the popup's, unshown");
    assert!(idle > std::time::Instant::now(), "after a while");
    popup.let_frame_go();
    let now = gpu.unshown_release_at().expect("the popup's, let go");
    assert!(now <= std::time::Instant::now(), "at once: what the wake frees");
    gpu.draw(&scene, &mut NoGlyphs, true);
    assert!(!popup.has_frame() && popup.frame_lost(), "let go: freed at the next frame");
    assert_eq!(popup.gpu_memory(), 0);
    assert!(page.has_frame(), "shown: kept");
    // Unshown long enough: freed.
    gpu.age_textures(crate::instances::IMAGE_IDLE);
    let empty = {
        let mut s = Scene::default();
        s.reset(Rgba::rgb(0x000000));
        s
    };
    gpu.draw(&empty, &mut NoGlyphs, true);
    assert!(page.frame_lost() && !page.has_frame(), "its owner asks for a fresh frame");
    // A fresh frame: whole into a new texture.
    assert!(importer.write(&page, frame));
    assert!(page.has_frame() && !page.frame_lost());
}

/// A texture freed while a frame was being copied into it (a popup let go, a tab unshown a
/// while) took the frame with it: the image isn't told a frame is in, so it stays lost
/// (its owner asks for a fresh one) and counts no bytes; the next frame goes whole into a
/// new texture.
#[test]
fn a_frame_copied_into_a_texture_freed_meanwhile_is_not_taken() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(8, 4)) else { return };
    let importer = gpu.importer();
    let page = ImageData::live();
    let bgra = [255u8, 0, 0, 255].repeat(8 * 4);
    let frame = render_api::PixelFrame { bgra: &bgra, size: (8, 4), stride: 32, changed: &[] };
    assert!(importer.write(&page, frame), "a first frame");
    let generation = page.generation();
    // The next frame's texture looked up…
    let size = extent(8, 4);
    let (target, made) = importer.target(&page, size);
    assert!(!made, "the one kept");
    // …the page let go meanwhile: its texture freed at the next frame drawn.
    page.let_frame_go();
    let mut empty = Scene::default();
    empty.reset(Rgba::rgb(0x000000));
    gpu.draw(&empty, &mut NoGlyphs, true);
    assert!(page.frame_lost());
    // The copy done: not taken.
    let copied = importer.copied(&page, &target, Areas::of(&[], size, false).copied(size));
    assert!(matches!(copied, Err(Refused::TextureFreed)), "{copied:?}");
    assert_eq!(page.generation(), generation, "no frame counted");
    assert!(page.frame_lost() && !page.has_frame(), "still asks for a fresh frame");
    assert_eq!(page.gpu_memory(), 0, "nothing on the GPU");
    // The next frame: a new texture, whole.
    let (_, made) = importer.target(&page, size);
    assert!(made);
}

/// A frame's copy failing.
fn fails(_: &wgpu::Texture, _: &Areas) -> Result<(), Refused> {
    Err(Refused::CopyTimedOut(COPY_LIMIT))
}

/// An 8 × 4 frame of `bgra` pixels, said changed only in its top left pixel.
fn corner_frame(bgra: &[u8]) -> render_api::PixelFrame<'_> {
    const CORNER: &[PixelRect] = &[PixelRect::new(0, 0, 1, 1)];
    render_api::PixelFrame { bgra, size: (8, 4), stride: 32, changed: CORNER }
}

/// `page`'s pixel at (5, 3) once drawn over black.
fn drawn_at(gpu: &mut Offscreen, page: &ImageData) -> [u8; 4] {
    let mut scene = Scene::default();
    scene.reset(Rgba::rgb(0x000000));
    scene.push_layer(None).image(page, Rect::new(0.0, 0.0, 8.0, 4.0));
    gpu.draw(&scene, &mut NoGlyphs, true);
    let (_, _, pixels) = gpu.pixels().expect("drawn");
    let mut at = [0; 4];
    at.copy_from_slice(&pixels[(3 * 8 + 5) * 4..][..4]);
    at
}

/// A texture made for a frame whose copy failed holds nothing: it isn't drawn, and the next
/// frame, though said changed in a corner only, is copied whole into it.
#[test]
fn a_texture_never_filled_is_not_drawn_and_filled_whole() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(8, 4)) else { return };
    let importer = gpu.importer();
    let page = ImageData::live();
    let failed = importer.fill(&page, extent(8, 4), &[], fails);
    assert!(matches!(failed, Err(Refused::CopyTimedOut(_))), "{failed:?}");
    assert!(!page.has_frame());
    assert!(!gpu.drawable(&page), "its empty texture not drawn");
    let red = [0u8, 0, 255, 255].repeat(8 * 4);
    if let Err(why) = importer.try_write(&page, corner_frame(&red)) {
        panic!("the next frame refused: {why}");
    }
    assert!(gpu.drawable(&page));
    assert_eq!(drawn_at(&mut gpu, &page), [255, 0, 0, 255], "red past the corner: whole");
}

/// A copy failing into a texture holding a frame leaves it drawn as it is, and the next
/// frame is copied whole (what it changed since the one that failed isn't all that's new).
#[test]
fn a_frame_after_a_failed_copy_is_copied_whole() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(8, 4)) else { return };
    let importer = gpu.importer();
    let page = ImageData::live();
    let blue = [255u8, 0, 0, 255].repeat(8 * 4);
    assert!(importer.try_write(&page, corner_frame(&blue)).is_ok(), "the first: whole");
    assert!(importer.fill(&page, extent(8, 4), &[], fails).is_err());
    assert!(gpu.drawable(&page), "still drawn: the frame before");
    assert_eq!(drawn_at(&mut gpu, &page), [0, 0, 255, 255], "the frame before's blue");
    let red = [0u8, 0, 255, 255].repeat(8 * 4);
    assert!(importer.try_write(&page, corner_frame(&red)).is_ok());
    assert_eq!(drawn_at(&mut gpu, &page), [255, 0, 0, 255], "red past the corner: whole");
}

/// A live image shown keeps its texture even with its frame let go (a window still draws
/// it); unshown, it goes at once.
#[test]
fn a_live_image_shown_is_kept_though_let_go() {
    let Ok(gpu) = Offscreen::new(PhysicalSize::new(8, 4)) else { return };
    let importer = gpu.importer();
    let page = ImageData::live();
    let bgra = [255u8, 0, 0, 255].repeat(8 * 4);
    assert!(importer.try_write(&page, corner_frame(&bgra)).is_ok());
    page.let_frame_go();
    assert!(page.frame_unwanted());
    assert_eq!(gpu.kept(page.id, true), Some(true), "shown: kept");
    assert_eq!(gpu.kept(page.id, false), Some(false), "unshown: gone");
}

/// A page's frames are premultiplied: half-transparent white over black comes out as half
/// white (straight alpha would darken it to a quarter).
#[test]
fn a_live_images_premultiplied_colors_are_drawn_as_they_are() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(4, 4)) else { return };
    let importer = gpu.frame_importer();
    let page = ImageData::live();
    // Premultiplied BGRA: white at half its alpha.
    let bgra = [128u8, 128, 128, 128].repeat(4 * 4);
    let frame = render_api::PixelFrame { bgra: &bgra, size: (4, 4), stride: 16, changed: &[] };
    assert!(importer.write(&page, frame));
    let mut scene = Scene::default();
    scene.reset(Rgba::rgb(0x000000));
    scene.push_layer(None).image(&page, Rect::new(0.0, 0.0, 4.0, 4.0));
    gpu.draw(&scene, &mut NoGlyphs, true);
    let (_, _, pixels) = gpu.pixels().expect("drawn");
    let pixel = &pixels[(2 * 4 + 2) * 4..][..4];
    for channel in &pixel[..3] {
        assert!((127..=129).contains(channel), "half white, not darker: {pixel:?}");
    }
}

#[cfg(target_os = "macos")]
mod surfaces {
    #![allow(unsafe_code)]

    use super::*;
    use render_api::{ImageInstance, ImportedFrame, NativeFrame};

    /// `outcome` taken, or the test fails saying why not.
    fn taken(outcome: Result<render_api::CopiedFrame, Refused>) {
        if let Err(why) = outcome {
            panic!("the frame refused: {why}");
        }
    }

    /// A source of the sources' tests: its name.
    type Named = &'static str;

    /// An image's sources: a few kept, the least recently read let go first; those of
    /// another size let go when one of a new size comes; another image's untouched.
    #[test]
    fn an_images_sources_are_kept_a_few_the_oldest_let_go() {
        let mut sources = Sources::<Named>::default();
        let size = extent(8, 8);
        sources.put(9, 100, size, "other image");
        for id in 0..SOURCES_PER_IMAGE as u64 {
            sources.put(1, id, size, "first");
        }
        assert_eq!(sources.len(), SOURCES_PER_IMAGE + 1);
        // Read again: now the latest; 1 becomes the oldest.
        assert_eq!(sources.get(1, 0, size), Some("first"));
        sources.put(1, 50, size, "past the limit");
        assert_eq!(sources.len(), SOURCES_PER_IMAGE + 1, "one let go");
        assert_eq!(sources.get(1, 1, size), None, "the least recently read");
        assert_eq!(sources.get(1, 0, size), Some("first"), "read again: kept");
        assert_eq!(sources.get(1, 0, extent(4, 4)), None, "another size under its id");
        assert_eq!(sources.get(1, 0, size), None, "and let go");
        sources.put(1, 60, extent(16, 16), "resized");
        assert_eq!(sources.get(1, 50, size), None, "the old size let go");
        assert_eq!(sources.get(9, 100, size), Some("other image"), "another image's stays");
        sources.keep_for(|image| image == 9);
        assert_eq!(sources.len(), 1, "only the images kept");
    }

    /// `kCFNumberSInt32Type`: a CoreFoundation number made from an `i32`.
    const CF_NUMBER_SINT32: isize = 9;

    #[link(name = "IOSurface", kind = "framework")]
    unsafe extern "C" {
        fn IOSurfaceCreate(properties: *const std::ffi::c_void) -> *mut std::ffi::c_void;
        fn IOSurfaceCreateMachPort(surface: *mut std::ffi::c_void) -> u32;
        fn IOSurfaceLock(surface: *mut std::ffi::c_void, options: u32, seed: *mut u32) -> i32;
        fn IOSurfaceUnlock(surface: *mut std::ffi::c_void, options: u32, seed: *mut u32) -> i32;
        fn IOSurfaceGetBaseAddress(surface: *mut std::ffi::c_void) -> *mut u8;
        fn IOSurfaceGetBytesPerRow(surface: *mut std::ffi::c_void) -> usize;
        static kIOSurfaceWidth: *const std::ffi::c_void;
        static kIOSurfaceHeight: *const std::ffi::c_void;
        static kIOSurfaceBytesPerElement: *const std::ffi::c_void;
        static kIOSurfacePixelFormat: *const std::ffi::c_void;
    }

    unsafe extern "C" {
        /// The task's own port (what `mach_task_self()` reads).
        static mach_task_self_: u32;
        fn mach_port_deallocate(task: u32, name: u32) -> i32;
    }

    /// The send right `IOSurfaceCreateMachPort` gave, given back (the engine's side does the
    /// same once its frame is handed over).
    fn release_port(port: u32) {
        // SAFETY: a send right made here, given back once.
        let done = unsafe { mach_port_deallocate(mach_task_self_, port) };
        assert_eq!(done, 0, "the port given back");
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNumberCreate(
            allocator: *const std::ffi::c_void,
            kind: isize,
            value: *const std::ffi::c_void,
        ) -> *const std::ffi::c_void;
        fn CFDictionaryCreate(
            allocator: *const std::ffi::c_void,
            keys: *const *const std::ffi::c_void,
            values: *const *const std::ffi::c_void,
            count: isize,
            key_callbacks: *const std::ffi::c_void,
            value_callbacks: *const std::ffi::c_void,
        ) -> *const std::ffi::c_void;
        fn CFRelease(object: *const std::ffi::c_void);
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
    }

    /// A BGRA surface of `width` × `height` filled with `bgra`, and a send right to it.
    fn surface(width: i32, height: i32, bgra: [u8; 4]) -> (*mut std::ffi::c_void, u32) {
        surface_of(width, height, *b"BGRA", bgra)
    }

    /// [`surface`] of the pixel format `format` names.
    fn surface_of(
        width: i32,
        height: i32,
        format: [u8; 4],
        bgra: [u8; 4],
    ) -> (*mut std::ffi::c_void, u32) {
        // SAFETY: CoreFoundation and IOSurface calls on values made here, released after.
        unsafe {
            let number = |v: &i32| {
                CFNumberCreate(std::ptr::null(), CF_NUMBER_SINT32, std::ptr::from_ref(v).cast())
            };
            let format = i32::from_be_bytes(format);
            let four = 4;
            let keys = [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfacePixelFormat,
            ];
            let values = [number(&width), number(&height), number(&four), number(&format)];
            let properties = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                4,
                std::ptr::from_ref(&kCFTypeDictionaryKeyCallBacks).cast(),
                std::ptr::from_ref(&kCFTypeDictionaryValueCallBacks).cast(),
            );
            let surface = IOSurfaceCreate(properties);
            CFRelease(properties);
            for v in values {
                CFRelease(v);
            }
            assert!(!surface.is_null());
            fill(surface, width, height, bgra);
            (surface, IOSurfaceCreateMachPort(surface))
        }
    }

    /// Every pixel of `surface` (`width` × `height`) made `bgra`, as an engine draws a frame.
    fn fill(surface: *mut std::ffi::c_void, width: i32, height: i32, bgra: [u8; 4]) {
        // SAFETY: a surface made here, locked while written within its rows.
        unsafe {
            IOSurfaceLock(surface, 0, std::ptr::null_mut());
            let base = IOSurfaceGetBaseAddress(surface);
            let row = IOSurfaceGetBytesPerRow(surface);
            for y in 0..height as usize {
                for x in 0..width as usize {
                    std::ptr::copy_nonoverlapping(bgra.as_ptr(), base.add(y * row + x * 4), 4);
                }
            }
            IOSurfaceUnlock(surface, 0, std::ptr::null_mut());
        }
    }

    /// A frame made elsewhere (an `IOSurface`, as the web engine hands it over) copied into a
    /// live image is drawn like any image: blue where the page is; another size remakes its
    /// texture; a released image takes no frame.
    #[test]
    fn a_frame_from_another_process_is_drawn_as_an_image() {
        let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(32, 16)) else { return };
        let importer = gpu.importer();
        let page = ImageData::live();
        let mut scene = Scene::default();
        scene.reset(Rgba::rgb(0x000000));
        scene.push_layer(None).images.push(ImageInstance::new(
            page.clone(),
            Rect::new(0.0, 0.0, 32.0, 16.0),
            1.0,
        ));
        // Before its first frame: nothing of it drawn.
        gpu.draw(&scene, &mut NoGlyphs, true);
        let (_, _, before) = gpu.pixels().expect("drawn");
        assert_eq!(&before[(8 * 32 + 16) * 4..][..4], [0, 0, 0, 255]);

        let (blue, port) = surface(32, 16, [255, 0, 0, 255]);
        taken(importer.try_import(&page, mac_frame(port, (32, 16))));
        gpu.draw(&scene, &mut NoGlyphs, true);
        let (_, _, pixels) = gpu.pixels().expect("drawn");
        assert_eq!(&pixels[(8 * 32 + 16) * 4..][..4], [0, 0, 255, 255], "the page's blue");

        // Another size: its texture made again, still drawn.
        assert_eq!(gpu.sources(), 1);
        let (red, port2) = surface(16, 8, [0, 0, 255, 255]);
        taken(importer.try_import(&page, mac_frame(port2, (16, 8))));
        assert_eq!(gpu.sources(), 1, "the surface of the old size let go");
        gpu.draw(&scene, &mut NoGlyphs, true);
        let (_, _, pixels) = gpu.pixels().expect("drawn");
        assert_eq!(&pixels[(8 * 32 + 16) * 4..][..4], [255, 0, 0, 255], "the new frame's red");

        page.release();
        let released = importer.try_import(&page, mac_frame(port2, (16, 8)));
        assert!(matches!(released, Err(Refused::NotLive)), "released: {released:?}");
        release_port(port);
        release_port(port2);
        // SAFETY: the surfaces made here, released once.
        unsafe {
            CFRelease(blue.cast_const());
            CFRelease(red.cast_const());
        }
    }

    fn mac_frame(port: u32, size: (u32, u32)) -> ImportedFrame<'static> {
        ImportedFrame { native: NativeFrame::MacSurfacePort(port), size, changed: &[] }
    }

    /// A surface not of the size or format the engine said is refused (another process's
    /// handle isn't trusted): nothing read past its pixels.
    #[test]
    fn a_surface_not_as_told_is_refused() {
        let Ok(gpu) = Offscreen::new(PhysicalSize::new(8, 8)) else { return };
        let importer = gpu.importer();
        let page = ImageData::live();
        let (small, port) = surface(8, 8, [255, 0, 0, 255]);
        let not_as_told = |outcome: Result<render_api::CopiedFrame, Refused>, what: &str| {
            assert!(matches!(outcome, Err(Refused::NotAsTold)), "{what}: {outcome:?}");
        };
        not_as_told(importer.try_import(&page, mac_frame(port, (64, 64))), "larger than it is");
        not_as_told(importer.try_import(&page, mac_frame(port, (8, 4))), "of another size");
        assert!(!page.has_frame());
        let (other, other_port) = surface_of(8, 8, *b"RGBA", [255, 0, 0, 255]);
        not_as_told(importer.try_import(&page, mac_frame(other_port, (8, 8))), "another format");
        taken(importer.try_import(&page, mac_frame(port, (8, 8))));
        release_port(port);
        release_port(other_port);
        // SAFETY: the surfaces made here, released once.
        unsafe {
            CFRelease(small.cast_const());
            CFRelease(other.cast_const());
        }
    }

    /// A surface the engine draws into again is opened once: its next frames read it as it is
    /// then (its new content copied), and it goes with its image's texture.
    #[test]
    fn a_surface_is_opened_once_and_read_at_each_frame() {
        let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(8, 8)) else { return };
        let importer = gpu.importer();
        let page = ImageData::live();
        let (shared, port) = surface(8, 8, [255, 0, 0, 255]);
        taken(importer.try_import(&page, mac_frame(port, (8, 8))));
        assert_eq!(gpu.sources(), 1);
        // The engine draws red into the same surface.
        fill(shared, 8, 8, [0, 0, 255, 255]);
        taken(importer.try_import(&page, mac_frame(port, (8, 8))));
        assert_eq!(gpu.sources(), 1, "the same, kept");
        let mut scene = Scene::default();
        scene.reset(Rgba::rgb(0x000000));
        scene.push_layer(None).image(&page, Rect::new(0.0, 0.0, 8.0, 8.0));
        gpu.draw(&scene, &mut NoGlyphs, true);
        let (_, _, pixels) = gpu.pixels().expect("drawn");
        assert_eq!(&pixels[(4 * 8 + 4) * 4..][..4], [255, 0, 0, 255], "its new content: red");
        page.release();
        gpu.draw(&scene, &mut NoGlyphs, true);
        assert_eq!(gpu.sources(), 0, "gone with the texture");
        release_port(port);
        // SAFETY: the surface made here, released once.
        unsafe { CFRelease(shared.cast_const()) };
    }
}
