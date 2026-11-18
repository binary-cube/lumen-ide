use super::*;
use render_api::{GlyphInstance, Point};

/// Test rasterizer: every glyph is a solid 4x4 square above the baseline.
struct SquareGlyphs {
    calls: usize,
}

impl GlyphRasterizer for SquareGlyphs {
    fn rasterize(&mut self, _key: GlyphKey) -> Option<RasterizedGlyph> {
        self.calls += 1;
        Some(RasterizedGlyph {
            left: 0,
            top: 4,
            width: 4,
            height: 4,
            format: GlyphFormat::Mask,
            data: vec![255; 16],
        })
    }
}

fn renderer(w: u32, h: u32) -> SoftwareRenderer {
    SoftwareRenderer::new(PhysicalSize::new(w, h)).unwrap()
}

#[test]
fn clears_to_background() {
    let mut r = renderer(4, 4);
    r.render(&Scene::new(Rgba::rgb(0x102030)), &mut SquareGlyphs { calls: 0 }).unwrap();
    assert_eq!(r.pixel(3, 3), Some([0x10, 0x20, 0x30, 0xff]));
}

#[test]
fn draws_quad_inside_bounds_only() {
    let mut r = renderer(10, 10);
    let mut scene = Scene::new(Rgba::BLACK);
    scene.push_layer(None).quad(Quad::filled(Rect::new(2.0, 2.0, 4.0, 4.0), Rgba::WHITE));
    r.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();

    assert_eq!(r.pixel(3, 3), Some([255, 255, 255, 255]));
    assert_eq!(r.pixel(1, 1), Some([0, 0, 0, 255]));
    assert_eq!(r.pixel(6, 6), Some([0, 0, 0, 255]));
}

#[test]
fn layer_clip_limits_drawing() {
    let mut r = renderer(10, 10);
    let mut scene = Scene::new(Rgba::BLACK);
    scene
        .push_layer(Some(Rect::new(0.0, 0.0, 5.0, 10.0)))
        .quad(Quad::filled(Rect::new(0.0, 0.0, 10.0, 10.0), Rgba::WHITE));
    r.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();

    assert_eq!(r.pixel(4, 5), Some([255, 255, 255, 255]));
    assert_eq!(r.pixel(5, 5), Some([0, 0, 0, 255]));
}

#[test]
fn rounded_corners_are_transparent() {
    let mut r = renderer(20, 20);
    let mut scene = Scene::new(Rgba::BLACK);
    scene
        .push_layer(None)
        .quad(Quad::filled(Rect::new(0.0, 0.0, 20.0, 20.0), Rgba::WHITE).rounded(8.0));
    r.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();

    assert_eq!(r.pixel(0, 0), Some([0, 0, 0, 255]));
    assert_eq!(r.pixel(10, 10), Some([255, 255, 255, 255]));
}

#[test]
fn border_is_drawn_inside_bounds() {
    let mut r = renderer(10, 10);
    let mut scene = Scene::new(Rgba::BLACK);
    scene.push_layer(None).quad(
        Quad::filled(Rect::new(0.0, 0.0, 10.0, 10.0), Rgba::BLACK)
            .bordered(2.0, Rgba::rgb(0xff0000)),
    );
    r.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();

    assert_eq!(r.pixel(0, 5), Some([255, 0, 0, 255]));
    assert_eq!(r.pixel(1, 5), Some([255, 0, 0, 255]));
    assert_eq!(r.pixel(5, 5), Some([0, 0, 0, 255]));
}

#[test]
fn glyphs_are_placed_above_baseline_and_cached() {
    let mut r = renderer(10, 10);
    let mut scene = Scene::new(Rgba::BLACK);
    let layer = scene.push_layer(None);
    for _ in 0..3 {
        layer.glyph(GlyphInstance {
            key: GlyphKey(1),
            origin: Point::new(2.0, 8.0),
            color: Rgba::rgb(0x00ff00),
        });
    }
    let mut glyphs = SquareGlyphs { calls: 0 };
    r.render(&scene, &mut glyphs).unwrap();

    assert_eq!(r.pixel(2, 4), Some([0, 255, 0, 255])); // top row of the glyph
    assert_eq!(r.pixel(2, 8), Some([0, 0, 0, 255])); // the baseline is not covered
    assert_eq!(glyphs.calls, 1);
}

#[test]
fn encodes_png() {
    let mut r = renderer(4, 4);
    r.render(&Scene::new(Rgba::WHITE), &mut SquareGlyphs { calls: 0 }).unwrap();
    let png = r.encode_png().unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

/// The window's corners cut: transparent outside the arc, half covered on its edge,
/// untouched inside and along the sides; square corners when not asked.
#[test]
fn the_window_corners_are_cut() {
    let mut renderer = SoftwareRenderer::new(PhysicalSize::new(64, 48)).unwrap();
    let mut scene = Scene::new(Rgba::rgb(0x336699));
    scene.window_corners = 16.0;
    renderer.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();
    let alpha = |r: &SoftwareRenderer, x: u32, y: u32| r.pixmap.pixel(x, y).unwrap().alpha();
    for (x, y) in [(0, 0), (63, 0), (0, 47), (63, 47)] {
        assert_eq!(alpha(&renderer, x, y), 0, "corner ({x}, {y})");
    }
    let edge = alpha(&renderer, 4, 4);
    assert!(edge > 0 && edge < 255, "the arc antialiased: {edge}");
    assert_eq!(alpha(&renderer, 32, 0), 255, "the top edge");
    assert_eq!(alpha(&renderer, 0, 24), 255, "the left edge");
    assert_eq!(alpha(&renderer, 32, 24), 255, "inside");
    scene.window_corners = 0.0;
    renderer.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();
    assert_eq!(alpha(&renderer, 0, 0), 255, "square corners");
}

/// Glyphs of a few shapes (by key): boxes left of / right of the pen, a color one, and key 0
/// with no pixels (a space).
struct VariedGlyphs;

impl GlyphRasterizer for VariedGlyphs {
    fn rasterize(&mut self, key: GlyphKey) -> Option<RasterizedGlyph> {
        let k = key.0;
        if k == 0 {
            return None;
        }
        let (width, height) = (3 + (k % 4) as u32, 5 + (k % 3) as u32);
        let color = k.is_multiple_of(5);
        let n = (width * height) as usize;
        let data = if color {
            (0..n).flat_map(|i| [200, (i * 37 % 256) as u8, 90, 160 + (i % 90) as u8]).collect()
        } else {
            (0..n).map(|i| (i * 53 % 256) as u8).collect()
        };
        Some(RasterizedGlyph {
            left: (k % 3) as i32 - 1,
            top: height as i32 - 1,
            width,
            height,
            format: if color { GlyphFormat::Color } else { GlyphFormat::Mask },
            data,
        })
    }
}

/// Deterministic pseudo-random numbers (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn coord(&mut self, max: u64) -> f32 {
        self.below(max * 4) as f32 / 4.0 - 3.0
    }
    fn color(&mut self) -> Rgba {
        let c = |r: &mut Rng| r.below(256) as f32 / 255.0;
        let (r, g, b) = (c(self), c(self), c(self));
        Rgba::new(r, g, b, [1.0, 0.6, 0.25][self.below(3) as usize])
    }
}

/// A test layer: its clip, quads, images (which, where, how opaque) and glyphs.
type TestLayer = (Option<Rect>, Vec<Quad>, Vec<(usize, Rect, f32)>, Vec<GlyphInstance>);

/// A scene like the workbench's: a background, panels with text (clipped), a caret,
/// images, rounded and bordered quads, fractional positions.
struct World {
    layers: Vec<TestLayer>,
    images: Vec<render_api::ImageData>,
    corners: f32,
}

impl World {
    fn new(rng: &mut Rng) -> Self {
        let images = (0..3)
            .map(|i| {
                let (w, h) = (3 + i, 2 + i);
                let px = (0..w * h * 4).map(|j| (j * 41 + i * 7) as u8).collect();
                render_api::ImageData::new(w, h, px)
            })
            .collect();
        let mut world = Self { layers: Vec::new(), images, corners: 6.0 };
        world.layers.push((
            None,
            // Not over the whole surface: the background shows elsewhere.
            vec![Quad::filled(Rect::new(0.0, 0.0, 60.0, 64.0), Rgba::rgb(0x202428))],
            Vec::new(),
            Vec::new(),
        ));
        for _ in 0..4 {
            let layer = world.random_layer(rng);
            world.layers.push(layer);
        }
        world
    }

    fn random_layer(&self, rng: &mut Rng) -> TestLayer {
        let clip = (rng.below(3) > 0).then(|| {
            Rect::new(rng.coord(60), rng.coord(40), 10.0 + rng.coord(50), 8.0 + rng.coord(40))
        });
        let quads = (0..rng.below(4)).map(|_| self.random_quad(rng)).collect();
        let images = (0..rng.below(3)).map(|_| self.random_image(rng)).collect();
        let glyphs = (0..rng.below(12)).map(|_| Self::random_glyph(rng)).collect();
        (clip, quads, images, glyphs)
    }

    fn random_quad(&self, rng: &mut Rng) -> Quad {
        let mut q = Quad::filled(
            Rect::new(rng.coord(90), rng.coord(60), 1.0 + rng.coord(40), 1.0 + rng.coord(30)),
            rng.color(),
        );
        if rng.below(2) == 0 {
            q = q.rounded(rng.below(8) as f32);
        }
        if rng.below(3) == 0 {
            q = q.bordered(1.0 + rng.below(2) as f32, rng.color());
        }
        q
    }

    fn random_image(&self, rng: &mut Rng) -> (usize, Rect, f32) {
        let bounds =
            Rect::new(rng.coord(90), rng.coord(60), 2.0 + rng.coord(20), 2.0 + rng.coord(20));
        (rng.below(3) as usize, bounds, [1.0, 0.5][rng.below(2) as usize])
    }

    fn random_glyph(rng: &mut Rng) -> GlyphInstance {
        GlyphInstance {
            key: GlyphKey(rng.below(11)),
            origin: Point::new(rng.coord(96), rng.coord(64) + 8.0),
            color: rng.color(),
        }
    }

    /// One small change, like a frame where something blinks, moves or appears.
    fn mutate(&mut self, rng: &mut Rng) {
        let n = self.layers.len();
        let i = 1 + rng.below(n as u64 - 1) as usize;
        match rng.below(14) {
            0 => {
                let q = self.random_quad(rng);
                self.layers[i].1.push(q);
            }
            1 => {
                self.layers[i].1.pop();
            }
            2 => {
                if let Some(q) = self.layers[i].1.first_mut() {
                    q.color = rng.color();
                }
            }
            3 => {
                if let Some(q) = self.layers[i].1.last_mut() {
                    q.bounds.origin.x += 0.25 + rng.below(3) as f32;
                }
            }
            4 => {
                let g = Self::random_glyph(rng);
                let at = rng.below(self.layers[i].3.len() as u64 + 1) as usize;
                self.layers[i].3.insert(at, g);
            }
            5 => {
                if !self.layers[i].3.is_empty() {
                    let at = rng.below(self.layers[i].3.len() as u64) as usize;
                    self.layers[i].3.remove(at);
                }
            }
            6 => {
                if let Some(g) = self.layers[i].3.last_mut() {
                    g.origin.y += 1.0;
                    g.color = rng.color();
                }
            }
            7 => {
                let image = self.random_image(rng);
                self.layers[i].2.push(image);
            }
            8 => {
                if let Some(image) = self.layers[i].2.first_mut() {
                    image.2 = 1.5 - image.2;
                    image.0 = (image.0 + 1) % 3;
                }
            }
            9 => {
                self.layers[i].0 = self.layers[i].0.map(|c| {
                    Rect::new(c.min_x() + 2.0, c.min_y(), c.size.width, c.size.height + 1.5)
                });
            }
            10 => {
                let layer = self.random_layer(rng);
                self.layers.insert(i, layer);
            }
            11 if n > 2 => {
                self.layers.remove(i);
            }
            12 => self.corners = [0.0, 6.0, 9.5][rng.below(3) as usize],
            _ => {} // Nothing changes.
        }
    }

    fn scene(&self) -> Scene {
        let mut scene = Scene::new(Rgba::rgb(0x101418));
        scene.window_corners = self.corners;
        for (clip, quads, images, glyphs) in &self.layers {
            let layer = scene.push_layer(*clip);
            for q in quads {
                layer.quad(*q);
            }
            for &(image, bounds, opacity) in images {
                layer.image(&self.images[image], bounds);
                layer.images.last_mut().unwrap().opacity = opacity;
            }
            for g in glyphs {
                layer.glyph(*g);
            }
        }
        scene
    }
}

/// Frames drawn only where they changed come out exactly as the same frames drawn whole:
/// a few hundred random small changes (primitives appearing, going, moving, recolored,
/// layers inserted, removed or reclipped, the window's corners), every frame compared.
#[test]
fn partial_frames_equal_full_redraws() {
    let size = PhysicalSize::new(96, 64);
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut world = World::new(&mut rng);
    let mut partial = SoftwareRenderer::new(size).unwrap();
    partial.set_partial(true);
    let mut full = SoftwareRenderer::new(size).unwrap();
    full.set_partial(false);
    let whole = u64::from(size.width * size.height);
    let (mut partial_frames, mut empty_frames) = (0, 0);
    for frame in 0..600 {
        if frame > 0 {
            world.mutate(&mut rng);
        }
        let scene = world.scene();
        let stats = partial.render(&scene, &mut VariedGlyphs).unwrap();
        full.render(&scene, &mut VariedGlyphs).unwrap();
        assert!(
            partial.pixmap.data() == full.pixmap.data(),
            "frame {frame}: the partial redraw differs from the full one"
        );
        match stats.drawn_pixels {
            0 => empty_frames += 1,
            n if n < whole => partial_frames += 1,
            _ => {}
        }
    }
    // The partial path was really taken (not full frames every time).
    assert!(partial_frames > 200, "{partial_frames} partial frames");
    assert!(empty_frames > 20, "{empty_frames} frames with nothing to draw");
}

/// A caret blinking redraws only its own pixels.
#[test]
fn a_blink_redraws_only_the_caret() {
    let mut r = renderer(64, 32);
    let frame = |caret: bool| {
        let mut scene = Scene::new(Rgba::BLACK);
        let layer = scene.push_layer(None);
        layer.quad(Quad::filled(Rect::new(0.0, 0.0, 64.0, 32.0), Rgba::rgb(0x223344)));
        if caret {
            layer.quad(Quad::filled(Rect::new(10.0, 4.0, 2.0, 16.0), Rgba::WHITE));
        }
        scene
    };
    assert_eq!(
        r.render(&frame(true), &mut SquareGlyphs { calls: 0 }).unwrap().drawn_pixels,
        64 * 32
    );
    assert_eq!(r.render(&frame(false), &mut SquareGlyphs { calls: 0 }).unwrap().drawn_pixels, 32);
    assert_eq!(r.pixel(10, 5), Some([0x22, 0x33, 0x44, 0xff]));
    assert_eq!(r.render(&frame(false), &mut SquareGlyphs { calls: 0 }).unwrap().drawn_pixels, 0);
    r.resize(PhysicalSize::new(64, 33));
    assert_eq!(
        r.render(&frame(false), &mut SquareGlyphs { calls: 0 }).unwrap().drawn_pixels,
        64 * 33,
        "resized: whole"
    );
}

/// A scene: a white bar on black, a layer over it that blurs it (opacity `shown`), and
/// optionally a caret-like square in the overlay or a change under it.
fn blur_scene(scene: &mut Scene, shown: f32, caret: bool, moved: bool) {
    scene.reset(Rgba::BLACK);
    let x = if moved { 22.0 } else { 20.0 };
    scene.push_layer(None).quad(Quad::filled(Rect::new(x, 0.0, 8.0, 40.0), Rgba::WHITE));
    let over = scene.push_layer(None);
    over.backdrop_blur(Rect::new(0.0, 0.0, 64.0, 40.0), 8.0, shown);
    if caret {
        over.quad(Quad::filled(Rect::new(60.0, 36.0, 2.0, 2.0), Rgba::rgb(0xff0000)));
    }
}

/// The blur spreads a sharp edge (a known image: a white bar on black): softer edges,
/// light reaching past the bar, the sum of light kept; at half its opacity, between the
/// sharp and the blurred.
#[test]
fn the_backdrop_blur_softens_a_known_image() {
    let mut r = renderer(64, 40);
    let mut scene = Scene::default();
    let mut glyphs = SquareGlyphs { calls: 0 };
    blur_scene(&mut scene, 1.0, false, false);
    let stats = r.render(&scene, &mut glyphs).unwrap();
    assert_eq!(stats.blurs, 1);
    let row: Vec<u8> = (0..64).map(|x| r.pixel(x, 20).unwrap()[0]).collect();
    // Light past the bar's edges, the middle no longer full white, symmetric.
    assert!(row[16] > 20 && row[16] < 200, "{row:?}");
    assert!(row[31] > 20, "{row:?}");
    assert!(row[24] < 255 && row[24] > 150, "{row:?}");
    assert!(row[2] < 5 && row[60] < 5, "{row:?}");
    let total: u32 = row.iter().map(|&v| u32::from(v)).sum();
    assert!((total as f32 - 8.0 * 255.0).abs() < 8.0 * 255.0 * 0.1, "{total}");
    // Half shown: the sharp bar shows through.
    blur_scene(&mut scene, 0.5, false, false);
    r.render(&scene, &mut glyphs).unwrap();
    let half = r.pixel(24, 20).unwrap()[0];
    assert!(half > row[24] && half < 255, "{half}");
}

/// The blur is made once while what's under it stays: the overlay changing (its caret, its
/// fade) reuses it, partial frames equal full ones; a change under it makes it again; no
/// blur in the scene: nothing kept.
#[test]
fn the_backdrop_blur_is_kept_until_what_is_under_it_changes() {
    let mut r = renderer(64, 40);
    let mut full = renderer(64, 40);
    full.set_partial(false);
    let mut scene = Scene::default();
    let mut glyphs = SquareGlyphs { calls: 0 };
    let frame = |r: &mut SoftwareRenderer, scene: &Scene, glyphs: &mut SquareGlyphs| {
        r.render(scene, glyphs).unwrap()
    };
    blur_scene(&mut scene, 1.0, false, false);
    frame(&mut r, &scene, &mut glyphs);
    assert_eq!(r.blurs_made(), 1);
    assert!(r.blur_memory() > 0);
    for (shown, caret) in [(1.0, true), (1.0, false), (0.6, false)] {
        blur_scene(&mut scene, shown, caret, false);
        let stats = frame(&mut r, &scene, &mut glyphs);
        assert_eq!(stats.blurs, 0);
        assert_eq!(r.blurs_made(), 1);
        frame(&mut full, &scene, &mut glyphs);
        assert_eq!(r.encode_png().unwrap(), full.encode_png().unwrap(), "{shown} {caret}");
    }
    blur_scene(&mut scene, 0.6, false, true);
    assert_eq!(frame(&mut r, &scene, &mut glyphs).blurs, 1);
    assert_eq!(r.blurs_made(), 2);
    frame(&mut full, &scene, &mut glyphs);
    assert_eq!(r.encode_png().unwrap(), full.encode_png().unwrap());
    // Closed.
    scene.reset(Rgba::BLACK);
    scene.push_layer(None).quad(Quad::filled(Rect::new(22.0, 0.0, 8.0, 40.0), Rgba::WHITE));
    frame(&mut r, &scene, &mut glyphs);
    assert_eq!(r.blur_memory(), 0);
    assert_eq!(r.pixel(24, 20), Some([255, 255, 255, 255]));
}

/// A known image: a red half and a green half, an overlay square in blue, the left half
/// kept by the emphasis (`gray`, `dim`) when `muted`.
fn emphasis_scene(muted: Option<(f32, f32)>, caret: bool) -> Scene {
    let mut scene = Scene::new(Rgba::BLACK);
    let layer = scene.push_layer(None);
    layer.quad(Quad::filled(Rect::new(0.0, 0.0, 32.0, 16.0), Rgba::rgb(0xff0000)));
    layer.quad(Quad::filled(Rect::new(32.0, 0.0, 32.0, 16.0), Rgba::rgb(0x00ff00)));
    let text = scene.push_layer(None);
    if caret {
        text.quad(Quad::filled(Rect::new(40.0, 2.0, 2.0, 8.0), Rgba::WHITE));
    }
    if let Some((gray, dim)) = muted {
        let mut e = Emphasis::new(gray, dim, 0.0);
        e.keep(render_api::Keep::new(Rect::new(0.0, 0.0, 32.0, 16.0), 0.0));
        scene.emphasize(e);
    }
    scene.push_layer(None).quad(Quad::filled(Rect::new(56.0, 8.0, 8.0, 8.0), Rgba::rgb(0x0000ff)));
    scene
}

#[test]
fn the_emphasis_mutes_a_known_image_outside_what_it_keeps() {
    let mut r = renderer(64, 16);
    let glyphs = &mut SquareGlyphs { calls: 0 };
    r.render(&emphasis_scene(Some((1.0, 0.0)), false), glyphs).unwrap();
    // Kept: as drawn. Muted: green's luma (0.7152 · 255 = 182). The overlay: as drawn.
    assert_eq!(r.pixel(4, 4), Some([255, 0, 0, 255]));
    assert_eq!(r.pixel(40, 4), Some([182, 182, 182, 255]));
    assert_eq!(r.pixel(60, 12), Some([0, 0, 255, 255]));
    // Half gray and dimmed by a fifth: (0 + 0.5 · (182 − 0)) · 0.8 = 73, (255 + 0.5 · (182 − 255)) · 0.8 = 175.
    r.render(&emphasis_scene(Some((0.5, 0.2)), false), glyphs).unwrap();
    assert_eq!(r.pixel(40, 4), Some([73, 175, 73, 255]));
    // Off: everything back as drawn.
    r.render(&emphasis_scene(None, false), glyphs).unwrap();
    assert_eq!(r.pixel(40, 4), Some([0, 255, 0, 255]));
    assert_eq!(r.pixel(4, 4), Some([255, 0, 0, 255]));
}

#[test]
fn with_an_emphasis_partial_frames_still_equal_full_ones() {
    let glyphs = &mut SquareGlyphs { calls: 0 };
    let mut partial = renderer(64, 16);
    let mut full = renderer(64, 16);
    full.set_partial(false);
    for (i, caret) in [true, false, true, true].into_iter().enumerate() {
        let scene = emphasis_scene(Some((0.75, 0.1)), caret);
        let stats = partial.render(&scene, glyphs).unwrap();
        full.render(&scene, glyphs).unwrap();
        assert!(partial.pixmap.data() == full.pixmap.data(), "frame {i}");
        if i > 0 {
            // The emphasis is the same: only the caret is drawn again (or nothing).
            assert!(stats.drawn_pixels < 64 * 16, "frame {i}: {}", stats.drawn_pixels);
        }
    }
}

/// What the emphasis costs the CPU renderer (release: `cargo test --release -p
/// render-software -- --ignored --nocapture measure_the_emphasis`): muting a whole frame
/// of 2400 × 1400 with three kept areas.
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_the_emphasis() {
    let mut r = renderer(2400, 1400);
    let mut e = Emphasis::new(1.0, 0.1, 12.0);
    for k in 0..3 {
        e.keep(render_api::Keep::new(
            Rect::new(100.0 + k as f32 * 700.0, 80.0, 600.0, 1200.0),
            16.0,
        ));
    }
    let mut times = Vec::new();
    for _ in 0..9 {
        let start = std::time::Instant::now();
        r.mute(&e, PixelRect::of_size(r.size()));
        times.push(start.elapsed());
    }
    times.sort();
    eprintln!("muting 2400 x 1400 on the CPU (3 kept areas): {:?}", times[times.len() / 2]);
}

#[test]
fn masks_draw_their_coverage_in_their_color() {
    let mut r = renderer(4, 2);
    let mut scene = Scene::new(Rgba::BLACK);
    // Full coverage on the left half, none on the right.
    let mask = render_api::ImageData::mask(
        4,
        2,
        vec![255, 255, 0, 0, 255, 255, 0, 0],
        Rgba::rgb(0xff0000),
    );
    scene.push_layer(None).image(&mask, Rect::new(0.0, 0.0, 4.0, 2.0));
    r.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();
    assert_eq!(r.pixel(0, 0), Some([0xff, 0, 0, 0xff]));
    assert_eq!(r.pixel(3, 1), Some([0, 0, 0, 0xff]));
    assert_eq!(mask.bytes_per_pixel(), 1);
}

/// A capture is the frame made small, given on another thread.
#[test]
fn a_capture_is_the_frame_made_small() {
    let mut r = renderer(40, 20);
    let mut scene = Scene::new(Rgba::rgb(0xff0000));
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 20.0, 20.0), Rgba::rgb(0x0000ff)));
    r.render(&scene, &mut SquareGlyphs { calls: 0 }).unwrap();
    let (sent, got) = std::sync::mpsc::channel();
    assert!(r.capture(10, Box::new(move |picture| sent.send(picture).unwrap())));
    let picture = got.recv_timeout(std::time::Duration::from_secs(5)).expect("captured");
    assert_eq!((picture.width, picture.height), (10, 5));
    assert_eq!(&picture.rgba[..4], &[0, 0, 255, 255]);
    assert_eq!(&picture.rgba[9 * 4..10 * 4], &[255, 0, 0, 255]);
}

/// One capture at a time: while one is made small, another isn't started.
#[test]
fn one_capture_at_a_time() {
    let mut r = renderer(8, 8);
    r.render(&Scene::new(Rgba::BLACK), &mut SquareGlyphs { calls: 0 }).unwrap();
    r.capturing.store(true, std::sync::atomic::Ordering::Release);
    assert!(!r.capture(4, Box::new(|_| {})), "one is under way");
    r.capturing.store(false, std::sync::atomic::Ordering::Release);
    let (sent, got) = std::sync::mpsc::channel();
    assert!(r.capture(4, Box::new(move |p| sent.send(p).unwrap())));
    got.recv_timeout(std::time::Duration::from_secs(5)).expect("captured");
    let free = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while r.capturing.load(std::sync::atomic::Ordering::Acquire) && std::time::Instant::now() < free
    {
        std::thread::yield_now();
    }
    assert!(r.capture(4, Box::new(|_| {})), "free again once done");
}
