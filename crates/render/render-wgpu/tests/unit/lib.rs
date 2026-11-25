use super::*;
use crate::instances::IMAGE_IDLE;

#[test]
fn gpu_structs_have_expected_layout() {
    assert_eq!(std::mem::size_of::<QuadGpu>(), 56);
    assert_eq!(std::mem::size_of::<PrimGpu>(), 76);
    // The viewport, the emphasis, eight kept areas' rects and parameters.
    assert_eq!(std::mem::size_of::<Globals>(), 16 + 16 + 2 * 8 * 16);
}

#[test]
fn shader_parses_and_validates() {
    for src in [
        concat!(include_str!("../../src/common.wgsl"), include_str!("../../src/shaders.wgsl")),
        concat!(include_str!("../../src/common.wgsl"), include_str!("../../src/image.wgsl")),
    ] {
        let module = naga_parse(src);
        assert!(module.is_ok(), "{module:?}");
    }
}

fn naga_parse(src: &str) -> Result<(), String> {
    let module = wgpu::naga::front::wgsl::parse_str(src).map_err(|e| e.emit_to_string(src))?;
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// A GPU without a window (`None`: no adapter here, e.g. a CI machine without one).
fn headless() -> Option<GpuContext> {
    crate::offscreen::headless().ok()
}

/// Glyphs of a few shapes (by key), a color one among them; key 0 has no pixels.
struct TestGlyphs;

impl GlyphRasterizer for TestGlyphs {
    fn rasterize(&mut self, key: render_api::GlyphKey) -> Option<render_api::RasterizedGlyph> {
        let k = key.0;
        if k == 0 {
            return None;
        }
        let (width, height) = (3 + (k % 4) as u32, 5 + (k % 3) as u32);
        let color = k.is_multiple_of(5);
        let n = (width * height) as usize;
        let data = if color {
            (0..n).flat_map(|i| [200, (i * 37 % 256) as u8, 90, 255]).collect()
        } else {
            (0..n).map(|i| (i * 53 % 256) as u8).collect()
        };
        Some(render_api::RasterizedGlyph {
            left: (k % 3) as i32 - 1,
            top: height as i32 - 1,
            width,
            height,
            format: if color {
                render_api::GlyphFormat::Color
            } else {
                render_api::GlyphFormat::Mask
            },
            data,
        })
    }
}

/// The kept frame's pixels, read back.
fn read_kept(ctx: &GpuContext, drawer: &Drawer) -> Vec<u8> {
    let (buffer, (w, h, row)) = drawer.copy_kept(ctx).expect("a kept frame");
    with_mapped(ctx, &buffer, |data| {
        (0..h as usize).flat_map(|y| data[y * row..][..w as usize * 4].to_vec()).collect()
    })
    .expect("read back")
}

/// Draws a frame into the drawer's kept texture: partly (where it changed) or whole.
fn draw_frame(
    ctx: &GpuContext,
    drawer: &mut Drawer,
    scene: &Scene,
    size: PhysicalSize,
    partial: bool,
) -> u64 {
    if !partial {
        drawer.tracker.invalidate();
    }
    drawer.compute_damage(ctx, scene, size, &mut TestGlyphs, true);
    if drawer.damage.is_empty() {
        return 0;
    }
    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let (_, drawn) = drawer.draw_kept(ctx, &mut encoder, scene, size, &mut TestGlyphs);
    ctx.queue.submit([encoder.finish()]);
    drawn
}

/// Deterministic pseudo-random numbers (xorshift).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
    fn coord(&mut self, max: u64) -> f32 {
        self.below(max * 4) as f32 / 4.0 - 3.0
    }
    fn color(&mut self) -> Rgba {
        let mut c = || self.below(256) as f32 / 255.0;
        let (r, g, b) = (c(), c(), c());
        Rgba::new(r, g, b, [1.0, 0.6, 0.25][self.below(3) as usize])
    }
    fn quad(&mut self) -> render_api::Quad {
        let bounds =
            Rect::new(self.coord(90), self.coord(60), 1.0 + self.coord(40), 1.0 + self.coord(30));
        render_api::Quad::filled(bounds, self.color())
            .rounded(self.below(6) as f32)
            .bordered(self.below(2) as f32, self.color())
    }
    fn glyph(&mut self) -> render_api::GlyphInstance {
        render_api::GlyphInstance {
            key: render_api::GlyphKey(self.below(11)),
            origin: kernel::Point::new(self.coord(96), self.coord(64) + 8.0),
            color: self.color(),
        }
    }
}

/// Frames drawn only where they changed come out exactly as the same frames drawn whole,
/// on the GPU (skipped where there's no adapter): a few hundred small random changes, every
/// frame read back and compared.
#[test]
fn partial_frames_equal_full_redraws_on_the_gpu() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 64);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    // Transparent: the window's corners are cut too.
    let (mut partial, mut full) =
        (Drawer::new(&ctx, format, true), Drawer::new(&ctx, format, true));
    let images: Vec<_> = (0..3u32)
        .map(|i| {
            let (w, h) = (3 + i, 2 + i);
            render_api::ImageData::new(
                w,
                h,
                (0..w * h * 4).map(|j| (j * 41 + i * 7) as u8).collect(),
            )
        })
        .collect();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    type Layer =
        (Option<Rect>, Vec<render_api::Quad>, Vec<(usize, Rect)>, Vec<render_api::GlyphInstance>);
    let mut layers: Vec<Layer> = vec![(
        None,
        // Not over the whole surface: the background shows elsewhere.
        vec![render_api::Quad::filled(Rect::new(0.0, 0.0, 60.0, 64.0), Rgba::rgb(0x202428))],
        Vec::new(),
        Vec::new(),
    )];
    let new_layer = |rng: &mut Rng| -> Layer {
        let clip = (rng.below(3) > 0).then(|| {
            Rect::new(rng.coord(60), rng.coord(40), 10.0 + rng.coord(50), 8.0 + rng.coord(40))
        });
        let quads = (0..rng.below(4)).map(|_| rng.quad()).collect();
        let images = (0..rng.below(3))
            .map(|_| {
                (
                    rng.below(3) as usize,
                    Rect::new(
                        rng.coord(90),
                        rng.coord(60),
                        2.0 + rng.coord(20),
                        2.0 + rng.coord(20),
                    ),
                )
            })
            .collect();
        let glyphs = (0..rng.below(12)).map(|_| rng.glyph()).collect();
        (clip, quads, images, glyphs)
    };
    for _ in 0..4 {
        let layer = new_layer(&mut rng);
        layers.push(layer);
    }
    let (mut partial_frames, mut empty_frames) = (0, 0);
    let mut corners = 6.0;
    for frame in 0..300 {
        if frame > 0 {
            let i = 1 + rng.below(layers.len() as u64 - 1) as usize;
            match rng.below(10) {
                0 => {
                    let q = rng.quad();
                    layers[i].1.push(q);
                }
                1 => drop(layers[i].1.pop()),
                2 => {
                    if let Some(q) = layers[i].1.first_mut() {
                        q.bounds.origin.x += 0.25 + rng.below(3) as f32;
                    }
                }
                3 => {
                    let g = rng.glyph();
                    let at = rng.below(layers[i].3.len() as u64 + 1) as usize;
                    layers[i].3.insert(at, g);
                }
                4 => {
                    if let Some(g) = layers[i].3.last_mut() {
                        g.origin.y += 1.0;
                    }
                }
                5 => {
                    let at =
                        (rng.below(3) as usize, Rect::new(rng.coord(90), rng.coord(60), 6.0, 5.0));
                    layers[i].2.push(at);
                }
                6 => {
                    layers[i].0 = layers[i].0.map(|c| {
                        Rect::new(c.min_x() + 2.0, c.min_y(), c.size.width, c.size.height + 1.5)
                    })
                }
                7 => {
                    let layer = new_layer(&mut rng);
                    layers.insert(i, layer);
                }
                8 if layers.len() > 2 => drop(layers.remove(i)),
                9 if rng.below(4) == 0 => corners = [0.0, 6.0, 9.5][rng.below(3) as usize],
                _ => {}
            }
        }
        let mut scene = Scene::new(Rgba::rgb(0x101418));
        scene.window_corners = corners;
        for (clip, quads, imgs, glyphs) in &layers {
            let layer = scene.push_layer(*clip);
            layer.quads.extend_from_slice(quads);
            for &(image, bounds) in imgs {
                layer.image(&images[image], bounds);
            }
            layer.glyphs.extend_from_slice(glyphs);
        }
        let drawn = draw_frame(&ctx, &mut partial, &scene, size, true);
        draw_frame(&ctx, &mut full, &scene, size, false);
        assert!(read_kept(&ctx, &partial) == read_kept(&ctx, &full), "frame {frame} differs");
        match drawn {
            0 => empty_frames += 1,
            n if n < u64::from(size.width * size.height) => partial_frames += 1,
            _ => {}
        }
    }
    assert!(partial_frames > 100, "{partial_frames} partial frames");
    assert!(empty_frames > 10, "{empty_frames} frames with nothing to draw");
}

#[test]
fn blur_shaders_parse_and_validate() {
    for src in [
        include_str!("../../src/blur.wgsl"),
        concat!(
            include_str!("../../src/common.wgsl"),
            include_str!("../../src/blur_composite.wgsl")
        ),
    ] {
        let module = naga_parse(src);
        assert!(module.is_ok(), "{module:?}");
    }
}

/// A white bar on black, a layer over it that blurs it (opacity `shown`); `moved`: the bar
/// elsewhere; `caret`: a small square in the overlay.
fn blur_scene(shown: f32, moved: bool, caret: bool) -> Scene {
    let mut scene = Scene::new(Rgba::BLACK);
    let x = if moved { 34.0 } else { 30.0 };
    scene
        .push_layer(None)
        .quad(render_api::Quad::filled(Rect::new(x, 0.0, 8.0, 64.0), Rgba::WHITE));
    let over = scene.push_layer(None);
    over.backdrop_blur(Rect::new(0.0, 0.0, 96.0, 64.0), 8.0, shown);
    if caret {
        over.quad(render_api::Quad::filled(Rect::new(90.0, 58.0, 2.0, 2.0), Rgba::rgb(0xff0000)));
    }
    scene
}

/// The blur on the GPU spreads the bar's light (edges softened, the light kept), made once
/// while what's under it stays (the overlay changing reuses it; partial frames equal whole
/// ones), again when it changes; its textures go when nothing blurs.
#[test]
fn the_backdrop_blur_is_made_once_and_kept_on_the_gpu() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 64);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let (mut partial, mut full) =
        (Drawer::new(&ctx, format, false), Drawer::new(&ctx, format, false));
    draw_frame(&ctx, &mut partial, &blur_scene(1.0, false, false), size, true);
    assert_eq!(partial.stale.len(), 1);
    assert!(partial.blur_bytes() > 0);
    let pixels = read_kept(&ctx, &partial);
    // Bgra: the red channel is the third byte.
    let row: Vec<u8> = (0..96).map(|x| pixels[(32 * 96 + x) * 4 + 2]).collect();
    assert!(row[26] > 20 && row[26] < 200, "{row:?}");
    assert!(row[41] > 20, "{row:?}");
    assert!(row[34] > 150 && row[34] < 255, "{row:?}");
    assert!(row[5] < 5 && row[90] < 5, "{row:?}");
    let total: u32 = row.iter().map(|&v| u32::from(v)).sum();
    assert!((total as f32 - 8.0 * 255.0).abs() < 8.0 * 255.0 * 0.1, "{total}");
    for (shown, moved, caret) in
        [(1.0, false, true), (0.6, false, false), (0.6, true, false), (0.6, true, true)]
    {
        let scene = blur_scene(shown, moved, caret);
        draw_frame(&ctx, &mut partial, &scene, size, true);
        let made = !partial.stale.is_empty();
        assert_eq!(made, moved && !caret, "{shown} {moved} {caret}");
        draw_frame(&ctx, &mut full, &scene, size, false);
        assert!(read_kept(&ctx, &partial) == read_kept(&ctx, &full), "{shown} {moved} {caret}");
    }
    let mut scene = Scene::new(Rgba::BLACK);
    scene
        .push_layer(None)
        .quad(render_api::Quad::filled(Rect::new(34.0, 0.0, 8.0, 64.0), Rgba::WHITE));
    draw_frame(&ctx, &mut partial, &scene, size, true);
    assert!(partial.blurs.is_empty() && partial.stale.is_empty());
}

/// Content scrolling in a clipped layer over a blur (a settings page over Glass): every
/// frame drawn only where it changed equals the frame drawn whole, the blur kept.
#[test]
fn content_scrolling_over_a_blur_is_drawn() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 64);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let (mut partial, mut full) =
        (Drawer::new(&ctx, format, false), Drawer::new(&ctx, format, false));
    for step in 0..12 {
        let mut scene = blur_scene(1.0, false, false);
        let content = scene.push_layer(Some(Rect::new(20.0, 10.0, 60.0, 44.0)));
        for i in 0..8 {
            let y = 10.0 + i as f32 * 9.0 - step as f32 * 3.0;
            content.quad(render_api::Quad::filled(
                Rect::new(24.0, y, 40.0, 6.0),
                Rgba::rgb(0x3080f0 + i * 8),
            ));
        }
        draw_frame(&ctx, &mut partial, &scene, size, true);
        if step > 0 {
            assert!(partial.stale.is_empty(), "{step}: the blur kept");
        }
        draw_frame(&ctx, &mut full, &scene, size, false);
        assert!(read_kept(&ctx, &partial) == read_kept(&ctx, &full), "step {step} differs");
    }
}

/// What a backdrop blur costs on this GPU (release: `cargo test --release -p render-wgpu
/// -- --ignored --nocapture measure`): at 4K, a frame without it, the frame it's made in,
/// frames while it stays (a caret blinking in the overlay), its memory, and nothing left
/// after.
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_the_backdrop_blur() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(3840, 2160);
    let mut drawer = Drawer::new(&ctx, wgpu::TextureFormat::Bgra8Unorm, false);
    let scene = |blur: Option<f32>, caret: bool| {
        let mut scene = Scene::new(Rgba::rgb(0x151618));
        let under = scene.push_layer(None);
        // A busy window: a few thousand quads.
        for i in 0..4000 {
            let (x, y) = ((i % 80) as f32 * 48.0, (i / 80) as f32 * 43.0);
            under.quad(
                render_api::Quad::filled(Rect::new(x, y, 40.0, 30.0), Rgba::rgb(0x2b2d30 + i))
                    .rounded(4.0),
            );
        }
        let over = scene.push_layer(None);
        if let Some(radius) = blur {
            over.backdrop_blur(Rect::new(0.0, 0.0, 3840.0, 2160.0), radius, 1.0);
        }
        over.quad(render_api::Quad::filled(
            Rect::new(0.0, 0.0, 3840.0, 2160.0),
            Rgba::BLACK.with_alpha(0.16),
        ));
        over.quad(render_api::Quad::filled(
            Rect::new(1200.0, 600.0, 1440.0, 960.0),
            Rgba::rgb(0x2b2d30),
        ));
        if caret {
            over.quad(render_api::Quad::filled(Rect::new(1300.0, 700.0, 4.0, 36.0), Rgba::WHITE));
        }
        scene
    };
    let time = |drawer: &mut Drawer, scene: &Scene| {
        let start = std::time::Instant::now();
        drawer.compute_damage(&ctx, scene, size, &mut TestGlyphs, true);
        let mut encoder =
            ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drawer.draw_kept(&ctx, &mut encoder, scene, size, &mut TestGlyphs);
        ctx.queue.submit([encoder.finish()]);
        ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finished");
        start.elapsed()
    };
    let median = |mut v: Vec<std::time::Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    for radius in [8.0, 24.0] {
        // Warm: the pipelines made, the kept frame there.
        time(&mut drawer, &scene(None, false));
        time(&mut drawer, &scene(Some(radius), false));
        time(&mut drawer, &scene(None, false));
        let (mut full, mut made, mut kept) = (Vec::new(), Vec::new(), Vec::new());
        let (mut making, mut staying) = (0, 0);
        for _ in 0..15 {
            drawer.tracker.invalidate();
            full.push(time(&mut drawer, &scene(None, false)));
            made.push(time(&mut drawer, &scene(Some(radius), false)));
            assert_eq!(drawer.stale.len(), 1);
            making = drawer.blur_bytes();
            for caret in [true, false, true] {
                kept.push(time(&mut drawer, &scene(Some(radius), caret)));
                assert!(drawer.stale.is_empty());
            }
            staying = drawer.blur_bytes();
            time(&mut drawer, &scene(None, false));
            assert!(drawer.blurs.is_empty() && drawer.blur_bytes() < 1 << 20);
        }
        eprintln!(
            "radius {radius} px at 4K: a whole frame without it {:?}; the frame it's made in {:?}; a frame while it stays (a caret blinking over it) {:?}; its memory {} KB while made, {} KB while it stays, none after",
            median(full),
            median(made),
            median(kept),
            making / 1024,
            staying / 1024
        );
    }
}

/// Red and green halves, a caret maybe, the left half kept by an emphasis (when `muted`),
/// a blue square over it.
fn emphasis_scene(muted: bool, caret: bool) -> Scene {
    let mut scene = Scene::new(Rgba::rgb(0x202020));
    let layer = scene.push_layer(None);
    layer.quad(render_api::Quad::filled(Rect::new(0.0, 0.0, 48.0, 32.0), Rgba::rgb(0xff0000)));
    layer.quad(render_api::Quad::filled(Rect::new(48.0, 0.0, 40.0, 32.0), Rgba::rgb(0x00ff00)));
    let text = scene.push_layer(None);
    if caret {
        text.quad(render_api::Quad::filled(Rect::new(60.0, 4.0, 2.0, 12.0), Rgba::WHITE));
    }
    if muted {
        let mut e = Emphasis::new(0.5, 0.2, 0.0);
        e.keep(render_api::Keep::new(Rect::new(0.0, 0.0, 48.0, 32.0), 0.0));
        scene.emphasize(e);
    }
    scene
        .push_layer(None)
        .quad(render_api::Quad::filled(Rect::new(80.0, 24.0, 8.0, 8.0), Rgba::rgb(0x0000ff)));
    scene
}

/// The emphasis on the GPU mutes as the software renderer does (its reference math), only
/// the layers under it, the background too; partial frames equal whole ones while it
/// stays; off, everything is as drawn again.
#[test]
fn the_emphasis_mutes_on_the_gpu_as_the_reference() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 32);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let (mut partial, mut full) =
        (Drawer::new(&ctx, format, false), Drawer::new(&ctx, format, false));
    // Bgra, at (x, y).
    let rgb = |pixels: &[u8], x: usize, y: usize| {
        let i = (y * 96 + x) * 4;
        [pixels[i + 2], pixels[i + 1], pixels[i]]
    };
    draw_frame(&ctx, &mut partial, &emphasis_scene(true, false), size, true);
    let pixels = read_kept(&ctx, &partial);
    assert_eq!(rgb(&pixels, 10, 10), [255, 0, 0], "kept");
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1);
    let muted = render_api::mute([0.0, 1.0, 0.0], 0.5, 0.2).map(|c| (c * 255.0).round() as u8);
    assert!(near(rgb(&pixels, 70, 10), muted), "{:?} {muted:?}", rgb(&pixels, 70, 10));
    assert_eq!(rgb(&pixels, 84, 28), [0, 0, 255], "the overlay");
    // The background (seen nowhere here but under the quads' edges) muted too: a frame
    // with only it.
    for caret in [true, false, true] {
        let scene = emphasis_scene(true, caret);
        let drawn = draw_frame(&ctx, &mut partial, &scene, size, true);
        assert!(drawn < 96 * 32, "only the caret: {drawn}");
        draw_frame(&ctx, &mut full, &scene, size, false);
        assert!(read_kept(&ctx, &partial) == read_kept(&ctx, &full), "caret {caret}");
    }
    draw_frame(&ctx, &mut partial, &emphasis_scene(false, false), size, true);
    let pixels = read_kept(&ctx, &partial);
    assert_eq!(rgb(&pixels, 70, 10), [0, 255, 0], "off: as drawn");
    assert_eq!(partial.muted_below, 0);
}

/// What the emphasis costs on this GPU (release: `cargo test --release -p render-wgpu --
/// --ignored --nocapture measure_the_emphasis`): at 2400 × 1400, a busy whole frame and a
/// partial one (a caret) without it and with it (one, three, eight kept areas).
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_the_emphasis() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(2400, 1400);
    let mut drawer = Drawer::new(&ctx, wgpu::TextureFormat::Bgra8Unorm, false);
    let scene = |keeps: usize, caret: bool| {
        let mut scene = Scene::new(Rgba::rgb(0x151618));
        let under = scene.push_layer(None);
        for i in 0..4000 {
            let (x, y) = ((i % 50) as f32 * 48.0, (i / 50) as f32 * 17.0);
            under.quad(
                render_api::Quad::filled(Rect::new(x, y, 40.0, 14.0), Rgba::rgb(0x2b2d30 + i))
                    .rounded(4.0),
            );
        }
        let text = scene.push_layer(None);
        if caret {
            text.quad(render_api::Quad::filled(Rect::new(1300.0, 700.0, 4.0, 36.0), Rgba::WHITE));
        }
        if keeps > 0 {
            let mut e = Emphasis::new(1.0, 0.1, 8.0);
            for k in 0..keeps {
                let r = Rect::new(100.0 + k as f32 * 280.0, 100.0, 250.0, 1200.0);
                e.keep(render_api::Keep::new(r, 12.0));
            }
            scene.emphasize(e);
        }
        scene
    };
    let time = |drawer: &mut Drawer, scene: &Scene| {
        let start = std::time::Instant::now();
        drawer.compute_damage(&ctx, scene, size, &mut TestGlyphs, true);
        let mut encoder =
            ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drawer.draw_kept(&ctx, &mut encoder, scene, size, &mut TestGlyphs);
        ctx.queue.submit([encoder.finish()]);
        ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finished");
        start.elapsed()
    };
    let median = |mut v: Vec<std::time::Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    for keeps in [0, 1, 3, 8] {
        time(&mut drawer, &scene(keeps, false));
        let (mut whole, mut caret) = (Vec::new(), Vec::new());
        for _ in 0..30 {
            drawer.tracker.invalidate();
            whole.push(time(&mut drawer, &scene(keeps, false)));
            caret.push(time(&mut drawer, &scene(keeps, true)));
            caret.push(time(&mut drawer, &scene(keeps, false)));
        }
        eprintln!(
            "at 2400 x 1400, {keeps} kept areas (0: no emphasis): a whole frame {:?}; a partial frame (a caret) {:?}",
            median(whole),
            median(caret)
        );
    }
}

#[test]
fn masks_and_images_draw_on_the_gpu() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(32, 16);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let mut drawer = Drawer::new(&ctx, format, false);
    let mut scene = Scene::new(Rgba::BLACK);
    // A mask (half covered) in red on the left; an RGBA image (blue) on the right.
    let coverage: Vec<u8> = (0..16 * 16).map(|i| if (i % 16) < 8 { 255 } else { 0 }).collect();
    let mask = render_api::ImageData::mask(16, 16, coverage, Rgba::rgb(0xff0000));
    let rgba = render_api::ImageData::new(16, 16, [0u8, 0, 255, 255].repeat(256));
    let layer = scene.push_layer(None);
    layer.image(&mask, Rect::new(0.0, 0.0, 16.0, 16.0));
    layer.image(&rgba, Rect::new(16.0, 0.0, 16.0, 16.0));
    draw_frame(&ctx, &mut drawer, &scene, size, false);
    let pixels = read_kept(&ctx, &drawer);
    let rgb = |x: usize, y: usize| {
        let i = (y * 32 + x) * 4;
        [pixels[i + 2], pixels[i + 1], pixels[i]]
    };
    assert_eq!(rgb(3, 8), [255, 0, 0], "covered: the mask's color");
    assert_eq!(rgb(12, 8), [0, 0, 0], "uncovered");
    assert_eq!(rgb(24, 8), [0, 0, 255], "the image");
}

/// The kept frame read back and made small (what `capture` does off the UI thread): a BGRA
/// frame comes out in RGBA, at most as wide as asked, its halves where they were.
#[test]
fn a_captured_frame_is_small_and_rgba() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 64);
    let mut drawer = Drawer::new(&ctx, wgpu::TextureFormat::Bgra8Unorm, false);
    assert!(drawer.copy_kept(&ctx).is_none(), "nothing kept, nothing to capture");
    let mut scene = Scene::new(Rgba::rgb(0xff0000));
    scene
        .push_layer(None)
        .quad(render_api::Quad::filled(Rect::new(0.0, 0.0, 48.0, 64.0), Rgba::rgb(0x0000ff)));
    draw_frame(&ctx, &mut drawer, &scene, size, false);
    let (buffer, layout) = drawer.copy_kept(&ctx).expect("a kept frame");
    let picture =
        with_mapped(&ctx, &buffer, |data| render_api::shrink(data, layout, drawer.bgra(), 24))
            .expect("read back");
    assert_eq!((picture.width, picture.height), (24, 16));
    assert_eq!(&picture.rgba[..4], &[0, 0, 255, 255], "blue on the left");
    assert_eq!(&picture.rgba[23 * 4..24 * 4], &[255, 0, 0, 255], "red on the right");
}

#[test]
fn windows_of_one_format_share_their_pipelines() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let one = Drawer::new(&ctx, wgpu::TextureFormat::Bgra8Unorm, false);
    let two = Drawer::new(&ctx, wgpu::TextureFormat::Bgra8Unorm, true);
    let other = Drawer::new(&ctx, wgpu::TextureFormat::Rgba8Unorm, false);
    assert!(Arc::ptr_eq(&one.pipelines, &two.pipelines));
    assert!(!Arc::ptr_eq(&one.pipelines, &other.pipelines));
}

/// Many new glyphs at once (a zoom, a cleared atlas) are uploaded together, from one buffer:
/// the frame is the same, pixel for pixel, as with the glyphs coming a few at a time (each
/// with its own upload), on the GPU (skipped where there's no adapter).
#[test]
fn glyphs_uploaded_together_draw_as_one_by_one() {
    let (Some(together), Some(apart)) = (headless(), headless()) else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 64);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let glyph = |k: u64| render_api::GlyphInstance {
        key: render_api::GlyphKey(k),
        origin: kernel::Point::new(((k * 9) % 90) as f32, 8.0 + ((k * 7) % 54) as f32),
        color: Rgba::rgb(0xe0c080),
    };
    let scene_of = |keys: std::ops::Range<u64>| {
        let mut scene = Scene::new(Rgba::rgb(0x101418));
        scene.push_layer(None).glyphs.extend(keys.map(glyph));
        scene
    };
    let all = 1..61;
    let mut first = Drawer::new(&together, format, false);
    draw_frame(&together, &mut first, &scene_of(all.clone()), size, false);
    assert!(together.state().atlas.len() >= 60);
    let mut second = Drawer::new(&apart, format, false);
    for end in (5..=60).step_by(5) {
        draw_frame(&apart, &mut second, &scene_of(1..end + 1), size, false);
    }
    draw_frame(&apart, &mut second, &scene_of(all), size, false);
    assert!(read_kept(&together, &first) == read_kept(&apart, &second));
}

/// An atlas grown (a zoom) gives its room back when the window goes idle, if what it holds
/// fits a smaller one; the glyphs are drawn again as they were (rasterized anew).
#[test]
fn a_grown_atlas_is_made_smaller_when_idle() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    assert_eq!(ctx.state().atlas.smaller_size(ATLAS_START), None, "it never grew");
    let size = PhysicalSize::new(96, 64);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let mut drawer = Drawer::new(&ctx, format, false);
    let mut scene = Scene::new(Rgba::rgb(0x101418));
    scene.push_layer(None).glyphs.extend((1..30).map(|k| render_api::GlyphInstance {
        key: render_api::GlyphKey(k),
        origin: kernel::Point::new((k * 3) as f32, 20.0),
        color: Rgba::WHITE,
    }));
    draw_frame(&ctx, &mut drawer, &scene, size, false);
    let before = read_kept(&ctx, &drawer);
    drawer::make_atlas_room(&ctx);
    drawer::make_atlas_room(&ctx);
    assert_eq!(ctx.state().atlas.size(), ATLAS_START * 4);
    draw_frame(&ctx, &mut drawer, &scene, size, false);
    drawer::shrink_atlas(&ctx);
    assert_eq!(ctx.state().atlas.size(), ATLAS_START, "back to its start");
    draw_frame(&ctx, &mut drawer, &scene, size, false);
    assert!(read_kept(&ctx, &drawer) == before, "drawn as before");
}

/// With no frames (the app in the background), an image on no window's screen is freed once
/// it's old enough; one still on a window's screen stays, however old.
#[test]
fn images_off_every_screen_are_freed_without_frames() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(32, 16);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let (mut left, mut right) =
        (Drawer::new(&ctx, format, false), Drawer::new(&ctx, format, false));
    let image = |seed: u8| render_api::ImageData::gpu_only(4, 4, vec![seed; 64]);
    let (gone, kept) = (image(10), image(200));
    let scene_with = |images: &[&render_api::ImageData]| {
        let mut scene = Scene::new(Rgba::BLACK);
        let layer = scene.push_layer(None);
        for i in images {
            layer.image(i, Rect::new(0.0, 0.0, 4.0, 4.0));
        }
        scene
    };
    draw_frame(&ctx, &mut left, &scene_with(&[&gone]), size, false);
    draw_frame(&ctx, &mut right, &scene_with(&[&kept]), size, false);
    // The left window's tab changed: its image is on no screen now.
    draw_frame(&ctx, &mut left, &scene_with(&[]), size, false);
    let old = std::time::Instant::now() - IMAGE_IDLE - std::time::Duration::from_secs(1);
    {
        let mut state = ctx.state();
        for t in state.textures.values_mut() {
            t.last_used = old;
        }
        assert_eq!(state.unshown_release_at(), Some(old + IMAGE_IDLE), "the left one's, only");
        state.release_unshown();
        assert!(!state.textures.contains_key(&gone.id), "off every screen: freed");
        assert!(state.textures.contains_key(&kept.id), "on the right window's: kept");
        assert_eq!(state.unshown_release_at(), None);
    }
    assert!(gone.is_lost() && !kept.is_lost());
    drop(right);
}

/// Whole frames drawn straight on screen (an animation over the window): the frame kept
/// falls behind and goes after a while; the next partial frame draws it whole again and
/// comes out exactly as a frame drawn whole (on the GPU; skipped without an adapter).
#[test]
fn frames_drawn_straight_leave_the_kept_frame_to_catch_up() {
    let Some(ctx) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = PhysicalSize::new(96, 64);
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let (mut drawer, mut full) = (Drawer::new(&ctx, format, true), Drawer::new(&ctx, format, true));
    let screen = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screen"),
        size: wgpu::Extent3d { width: 96, height: 64, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = screen.create_view(&wgpu::TextureViewDescriptor::default());
    // A sky over the whole surface (its shade by `shift`: every frame changes it all), bars
    // moved by `shift`, a square moved by `moved`.
    let scene = |shift: f32, moved: f32| {
        let mut scene = Scene::default();
        let layer = scene.push_layer(None);
        let sky = Rgba::new(0.05, 0.08 + (shift % 30.0) / 300.0, 0.12, 1.0);
        layer.quad(render_api::Quad::filled(Rect::new(0.0, 0.0, 96.0, 64.0), sky));
        for i in 0..12 {
            let x = (i as f32 * 9.0 + shift) % 90.0;
            layer.quad(render_api::Quad::filled(
                Rect::new(x, (i * 5) as f32, 6.0, 4.0),
                Rgba::rgb(0xd0a040),
            ));
        }
        layer.quad(render_api::Quad::filled(
            Rect::new(40.0 + moved, 30.0, 5.0, 5.0),
            Rgba::rgb(0x40c0f0),
        ));
        scene
    };
    draw_frame(&ctx, &mut drawer, &scene(0.0, 0.0), size, true);
    // Whole frames: each everything moved, straight on screen.
    for frame in 1..=70 {
        let s = scene(frame as f32 * 3.0, 0.0);
        drawer.compute_damage(&ctx, &s, size, &mut TestGlyphs, true);
        assert!(drawer.damage.is_full(), "frame {frame}: whole");
        let mut encoder =
            ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drawer.draw_straight(&ctx, &mut encoder, &s, size, &mut TestGlyphs, &view);
        ctx.queue.submit([encoder.finish()]);
        drawer.shown = Some(size);
    }
    assert!(drawer.kept_stale, "behind");
    assert!(drawer.kept.is_none(), "its memory gone after a while of them");
    // The animation stops (a small change, then the same frame): taken whole a few frames
    // more (checked every eighth), then partial again, against the last frame shown — the
    // kept frame drawn whole first, equal to the same frame drawn whole.
    let last = scene(70.0 * 3.0, 4.0);
    let mut whole_after = 0;
    loop {
        drawer.compute_damage(&ctx, &last, size, &mut TestGlyphs, true);
        if !drawer.damage.is_full() {
            break;
        }
        whole_after += 1;
        assert!(whole_after <= 8, "partial again within a check");
        let mut encoder =
            ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drawer.draw_straight(&ctx, &mut encoder, &last, size, &mut TestGlyphs, &view);
        ctx.queue.submit([encoder.finish()]);
    }
    // Drawn straight meanwhile: the frame shown is `last` already, nothing left to change —
    // a last small change brings the partial frame.
    let last = scene(70.0 * 3.0, 8.0);
    drawer.compute_damage(&ctx, &last, size, &mut TestGlyphs, true);
    assert!(!drawer.damage.is_full(), "only the moved square");
    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    drawer.draw_kept(&ctx, &mut encoder, &last, size, &mut TestGlyphs);
    ctx.queue.submit([encoder.finish()]);
    assert!(!drawer.kept_stale);
    draw_frame(&ctx, &mut full, &last, size, false);
    assert!(read_kept(&ctx, &drawer) == read_kept(&ctx, &full), "the kept frame caught up");
}

#[test]
fn the_meter_counts_the_surface_images_a_window_holds() {
    let retina = PhysicalSize::new(2560, 1600);
    let image = 2560 * 1600 * 4;
    // One image drawn while the other is on screen; a third while resizing.
    assert_eq!(surface_bytes(retina, 1, false, false), 2 * image);
    assert_eq!(surface_bytes(retina, 2, false, false), 3 * image);
    // Standing still: only the one on screen.
    assert_eq!(surface_bytes(retina, 1, true, false), image);
    // Hidden a while: none on macOS (the layer went), a pixel's worth elsewhere.
    let hidden = surface_bytes(retina, 1, false, true);
    if cfg!(target_os = "macos") {
        assert_eq!(hidden, 0);
    } else {
        assert_eq!(hidden, 2 * 4);
    }
    assert_eq!(surface_bytes(retina, 1, true, true), if cfg!(target_os = "macos") { 0 } else { 4 });
}

#[test]
fn a_size_told_again_is_no_resize() {
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8Unorm,
        width: 2560,
        height: 1600,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 1,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: Vec::new(),
        color_space: wgpu::SurfaceColorSpace::default(),
    };
    assert!(!resizes(&config, PhysicalSize::new(2560, 1600)));
    assert!(resizes(&config, PhysicalSize::new(2560, 1601)));
    assert!(resizes(&config, PhysicalSize::new(0, 0)), "minimized");
}

/// The same image drawn again in a row is one run (one draw); another between breaks it.
#[test]
fn the_same_image_in_a_row_is_one_run() {
    let runs: Vec<_> = drawer::same_image_runs(&[7, 7, 7, 3, 7, 7]).collect();
    assert_eq!(runs, vec![0..3, 3..4, 4..6]);
    assert_eq!(drawer::same_image_runs(&[]).count(), 0);
}
