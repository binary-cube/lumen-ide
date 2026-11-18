//! A window's decorations drawn: the art made once and kept (the same images frame after
//! frame, made again only for another scale or when the GPU let them go), a handful of
//! images a frame, nothing drawn under the cards, nothing allocated once warmed up, and the
//! budgets (plan 41): a frame in ≤ 0.2 ms, everything held in ≤ 1 MB — for the Christmas
//! decorations and for every preset.

use super::*;
use render_api::Scene;
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::cell::Cell;
use theming::decorations::{
    CornerSpec, CornerStyle, GarlandSpec, LayerSpec, ParticleSpec, presets,
};

fn christmas() -> Arc<Decorations> {
    Arc::new(Decorations {
        layers: vec![
            LayerSpec::Garland(GarlandSpec::default()),
            LayerSpec::Corners(CornerSpec::default()),
            LayerSpec::Particles(ParticleSpec { density: 0.5, ..ParticleSpec::default() }),
        ],
        dark: true,
    })
}

/// Every effect of plan 42, alone (the ones behind the panels too).
fn effects() -> Vec<Arc<Decorations>> {
    use theming::decorations::{EffectSpec, LayerKind, Zone};
    LayerKind::ALL
        .iter()
        .filter(|k| matches!(LayerSpec::new(**k), LayerSpec::Effect(_)))
        .flat_map(|k| {
            [Zone::Edges, Zone::Panels].map(|zone| {
                let spec = EffectSpec { zone, ..EffectSpec::new(*k) };
                Arc::new(Decorations { layers: vec![LayerSpec::Effect(spec)], dark: true })
            })
        })
        .collect()
}

/// The app's icons with the module's (Matrix's characters), made once.
fn icons() -> &'static icons::Icons {
    static ICONS: std::sync::OnceLock<icons::Icons> = std::sync::OnceLock::new();
    ICONS.get_or_init(|| {
        let mut icons = icons::Icons::builtin();
        for g in crate::glyphs::all() {
            icons.register(g.name, g.svg);
        }
        icons
    })
}

/// A preset's decorations.
fn preset(id: &str) -> Arc<Decorations> {
    Arc::new(Decorations { layers: presets::layers(id, true).unwrap(), dark: true })
}

/// The id of the first garland's swag image.
fn swag(painter: &Painter) -> u64 {
    painter
        .layers
        .iter()
        .find_map(|l| match l {
            LayerPainter::Garland(p) => p.swag.as_ref().map(|i| i.id),
            _ => None,
        })
        .unwrap()
}

/// A 1200 × 700 window at `scale`, its top bar's content and its cards.
struct Window {
    scale: f32,
    w: f32,
    h: f32,
    clear: Vec<Rect>,
    cards: Vec<Rect>,
}

impl Window {
    fn new(w: f32, h: f32, scale: f32) -> Self {
        let s = scale;
        let (w, h) = (w * s, h * s);
        Self {
            scale,
            w,
            h,
            clear: vec![
                Rect::new(18.0 * s, 6.0 * s, 110.0 * s, 28.0 * s),
                Rect::new(w - 80.0 * s, 6.0 * s, 64.0 * s, 28.0 * s),
            ],
            cards: vec![
                Rect::new(37.0 * s, 33.0 * s, 230.0 * s, h - 60.0 * s),
                Rect::new(283.0 * s, 33.0 * s, w - 320.0 * s, h - 60.0 * s),
            ],
        }
    }

    fn frame(&self, time: f32) -> DecorationFrame<'_> {
        DecorationFrame {
            geometry: Geometry {
                window: Rect::new(0.0, 0.0, self.w, self.h),
                scale: self.scale,
                top_inset: 0.0,
                top_bar: 40.0 * self.scale,
                keep_clear: &self.clear,
            },
            covered: &self.cards,
            editor: &self.cards[1..],
            time,
            icons: Some(icons()),
        }
    }
}

fn draw(painter: &mut Painter, scene: &mut Scene, frame: &DecorationFrame<'_>) -> usize {
    scene.reset(kernel::Rgba::BLACK);
    let layer = scene.push_layer(None);
    painter.paint(frame, layer);
    // What it drew: images, quads (the stars, the sparks, the neon) and glyphs (Matrix).
    layer.images.len() + layer.quads.len() + layer.glyphs.len()
}

#[test]
fn the_art_is_made_once_and_kept() {
    let window = Window::new(1200.0, 700.0, 2.0);
    let mut painter = Painter::new(christmas());
    let mut scene = Scene::default();
    draw(&mut painter, &mut scene, &window.frame(1.0));
    let ids: Vec<u64> = scene.layers()[0].images.iter().map(|i| i.image.id).collect();
    let first = swag(&painter);
    let all: Vec<u64> = painter.images().map(|i| i.id).collect();
    for t in [1.1, 2.0, 30.0] {
        draw(&mut painter, &mut scene, &window.frame(t));
        assert_eq!(swag(&painter), first, "not drawn again");
    }
    assert_eq!(painter.images().map(|i| i.id).collect::<Vec<_>>(), all, "none drawn again");
    assert!(!ids.is_empty());
    // A few pixels wider: the same swag image, stretched; another scale: drawn again.
    let wider = Window::new(1203.0, 700.0, 2.0);
    draw(&mut painter, &mut scene, &wider.frame(1.0));
    assert_eq!(swag(&painter), first);
    let sharper = Window::new(1200.0, 700.0, 3.0);
    draw(&mut painter, &mut scene, &sharper.frame(1.0));
    assert_ne!(swag(&painter), first);
}

#[test]
fn the_gpu_letting_the_art_go_makes_it_again() {
    let window = Window::new(1200.0, 700.0, 2.0);
    let mut painter = Painter::new(christmas());
    let mut scene = Scene::default();
    draw(&mut painter, &mut scene, &window.frame(1.0));
    let before = swag(&painter);
    // Uploaded (its pixels given up), then evicted: lost.
    for image in painter.images() {
        image.uploaded();
        image.evicted();
    }
    draw(&mut painter, &mut scene, &window.frame(1.0));
    assert_ne!(swag(&painter), before);
    assert!(!painter.images().any(ImageData::is_lost));
}

/// A frame draws a few dozen images at most (one per swag, not per bulb; only the lights
/// swelling now; only the flakes not under a card), and none under the cards.
#[test]
fn a_frame_is_a_handful_of_images_none_under_the_cards() {
    let window = Window::new(1200.0, 700.0, 2.0);
    let mut painter = Painter::new(christmas());
    let mut scene = Scene::default();
    let mut most = 0;
    for step in 0..100 {
        let n = draw(&mut painter, &mut scene, &window.frame(step as f32 * 0.37));
        most = most.max(n);
        for image in &scene.layers()[0].images {
            assert!(!hidden(&image.bounds, &window.cards), "{image:?} under a card");
        }
    }
    let lights = painter.garland().unwrap().lights.len();
    assert!(most < 70, "{most} images");
    assert!(most < lights + painter.particles(), "fewer than one per light and flake");
}

/// Static: the same frame every time (the snapshots).
#[test]
fn the_same_time_draws_the_same_frame() {
    let window = Window::new(1200.0, 700.0, 2.0);
    let mut a = Painter::new(christmas());
    let mut b = Painter::new(christmas());
    let (mut sa, mut sb) = (Scene::default(), Scene::default());
    draw(&mut a, &mut sa, &window.frame(7.3));
    draw(&mut b, &mut sb, &window.frame(7.3));
    let bounds =
        |s: &Scene| s.layers()[0].images.iter().map(|i| (i.bounds, i.opacity)).collect::<Vec<_>>();
    assert_eq!(bounds(&sa), bounds(&sb));
}

/// Everything held — the layout and every image before the GPU takes it — stays under
/// 1 MB, on a large window at a high density too; once uploaded, nothing but the layout.
#[test]
fn everything_held_stays_under_a_megabyte() {
    let all: Vec<Arc<Decorations>> = std::iter::once(christmas())
        .chain(presets::ALL.iter().map(|id| preset(id)))
        .chain(effects())
        .collect();
    for (w, h, scale) in
        [(1200.0, 700.0, 2.0), (1920.0, 1080.0, 2.0), (2560.0, 1440.0, 1.5), (1600.0, 900.0, 3.0)]
    {
        for decorations in &all {
            let window = Window::new(w, h, scale);
            let mut painter = Painter::new(Arc::clone(decorations));
            let mut scene = Scene::default();
            draw(&mut painter, &mut scene, &window.frame(1.0));
            let held = painter.memory();
            eprintln!("decorations: {w}x{h}@{scale}: {held} bytes before the upload");
            assert!(held <= 1 << 20, "{w}x{h}@{scale}: {held} bytes");
            for image in painter.images() {
                image.uploaded();
            }
            let after = painter.memory();
            assert!(after < 64 * 1024, "{w}x{h}@{scale}: {after} bytes after the upload");
            assert!(painter.gpu_memory() <= 1 << 20);
        }
    }
}

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// The system's allocator, counting what this thread allocates while asked to.
struct Counting;

fn counted() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

// SAFETY: every call goes to the system's allocator as it came; counting only reads and
// writes this thread's own cells (const-initialized: touching them allocates nothing).
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        counted();
        // SAFETY: the caller's layout, handed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: AllocLayout) -> *mut u8 {
        counted();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        // SAFETY: a pointer this allocator gave, with its layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: AllocLayout, new_size: usize) -> *mut u8 {
        counted();
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Animated frames, once warmed up (the layout worked out, the art made, the layer's lists
/// grown to their most), allocate nothing.
#[test]
fn an_animated_frame_allocates_nothing() {
    // The counter counts.
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let probe: Vec<u8> = Vec::with_capacity(16);
    COUNTING.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 1);
    drop(probe);
    let all: Vec<Arc<Decorations>> = std::iter::once(christmas())
        .chain(presets::ALL.iter().map(|id| preset(id)))
        .chain(effects())
        .collect();
    for (scale, decorations) in [1.0, 2.0].into_iter().flat_map(|s| all.iter().map(move |d| (s, d)))
    {
        let window = Window::new(1200.0, 700.0, scale);
        let mut painter = Painter::new(Arc::clone(decorations));
        let mut scene = Scene::default();
        // Warm up over a whole cycle of the lights and the snow.
        for step in 0..400 {
            draw(&mut painter, &mut scene, &window.frame(step as f32 / 15.0));
        }
        ALLOCATIONS.with(|n| n.set(0));
        COUNTING.with(|on| on.set(true));
        for step in 0..400 {
            draw(&mut painter, &mut scene, &window.frame(step as f32 / 15.0));
        }
        COUNTING.with(|on| on.set(false));
        let allocations = ALLOCATIONS.with(Cell::get);
        assert_eq!(
            allocations, 0,
            "{allocations} allocations in 400 frames at {scale}: {decorations:?}"
        );
    }
}

/// The budget (plan 41): an animated frame's decorations in at most 0.2 ms — measured in a
/// release build, the median of 301 frames, on a 1200 × 700 window at scale 2 and a large
/// one.
#[test]
#[cfg_attr(debug_assertions, ignore = "a budget: cargo test -p theme-decorations --release budget")]
fn budget_an_animated_frame_is_drawn_in_time() {
    let all: Vec<(String, Arc<Decorations>)> =
        std::iter::once(("christmas-theme".to_owned(), christmas()))
            .chain(presets::ALL.iter().map(|id| ((*id).to_owned(), preset(id))))
            .chain(
                effects()
                    .into_iter()
                    .map(|d| (format!("{:?}/{:?}", d.layers[0].kind(), d.layers[0].zone()), d)),
            )
            .collect();
    for ((w, h), (name, decorations)) in [(1200.0, 700.0), (2560.0, 1440.0)]
        .into_iter()
        .flat_map(|s| all.iter().map(move |d| (s, d)))
    {
        let window = Window::new(w, h, 2.0);
        let mut painter = Painter::new(Arc::clone(decorations));
        let mut scene = Scene::default();
        for step in 0..30 {
            draw(&mut painter, &mut scene, &window.frame(step as f32 / 15.0));
        }
        let mut times: Vec<std::time::Duration> = (0..301)
            .map(|step| {
                let start = std::time::Instant::now();
                draw(&mut painter, &mut scene, &window.frame(2.0 + step as f32 / 15.0));
                start.elapsed()
            })
            .collect();
        times.sort();
        let median = times[times.len() / 2];
        eprintln!(
            "budget: {name} decorations frame {w}x{h}@2: median {median:?}, max {:?}",
            times[times.len() - 1]
        );
        assert!(median <= std::time::Duration::from_micros(200), "{median:?}");
    }
    // The art made the first time (once per scale), for the record.
    let window = Window::new(1200.0, 700.0, 2.0);
    let start = std::time::Instant::now();
    let mut painter = Painter::new(christmas());
    let mut scene = Scene::default();
    draw(&mut painter, &mut scene, &window.frame(0.0));
    eprintln!("budget: the first frame (layout and art made) {:?}", start.elapsed());
}

/// Each preset draws (at least some of its layers show around the cards), the same at the
/// same time; a corner may be an icon of the app's (a glyph, none without the icons).
#[test]
fn every_preset_draws_and_corners_may_be_icons() {
    let window = Window::new(1200.0, 700.0, 2.0);
    for id in presets::ALL {
        let mut painter = Painter::new(preset(id));
        let mut scene = Scene::default();
        let n = draw(&mut painter, &mut scene, &window.frame(7.3));
        assert!(n > 0, "{id}");
        let first: Vec<(Rect, f32)> =
            scene.layers()[0].images.iter().map(|i| (i.bounds, i.opacity)).collect();
        draw(&mut painter, &mut scene, &window.frame(7.3));
        let again: Vec<(Rect, f32)> =
            scene.layers()[0].images.iter().map(|i| (i.bounds, i.opacity)).collect();
        assert_eq!(first, again, "{id}");
    }
    let icon = Arc::new(Decorations {
        layers: vec![LayerSpec::Corners(CornerSpec {
            style: CornerStyle::Icon,
            icon: "bell".into(),
            ..CornerSpec::default()
        })],
        dark: true,
    });
    let mut painter = Painter::new(Arc::clone(&icon));
    let mut scene = Scene::default();
    let mut bare = window.frame(1.0);
    bare.icons = None;
    draw(&mut painter, &mut scene, &bare);
    assert!(scene.layers()[0].glyphs.is_empty(), "no icons given: nothing");
    let icons = icons::Icons::builtin();
    let mut frame = window.frame(1.0);
    frame.icons = Some(&icons);
    scene.reset(kernel::Rgba::BLACK);
    let layer = scene.push_layer(None);
    painter.paint(&frame, layer);
    assert_eq!(layer.glyphs.len(), 2, "both corners");
    assert!(layer.images.is_empty());
}

/// What a frame of each effect redraws (plan 42f, measured): the share of a 1200 × 700 @2
/// window the renderer draws again from one frame to the next at 20 frames a second (its
/// damage, as the renderers' tracker finds it, the cards hiding what they cover). The neon
/// edge keeps to its four edges.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "a measure: cargo test -p theme-decorations --release budget"
)]
fn budget_the_share_a_frame_redraws() {
    use render_api::{Damage, DamageTracker, GlyphBox};
    let window = Window::new(1200.0, 700.0, 2.0);
    let size = kernel::PhysicalSize::new(2400, 1400);
    let surface = 2400.0 * 1400.0;
    let mut shares = Vec::new();
    for decorations in effects() {
        let mut painter = Painter::new(Arc::clone(&decorations));
        let mut scene = Scene::default();
        let mut tracker = DamageTracker::new();
        let mut damage = Damage::default();
        let mut boxes = |key: render_api::GlyphKey| {
            let size = (key.0 & 0xffff) as u32;
            Some(GlyphBox { left: 0, top: size as i32, width: size, height: size })
        };
        let mut drawn = 0.0;
        let frames = 40;
        for step in 0..=frames {
            draw(&mut painter, &mut scene, &window.frame(4.0 + step as f32 / 20.0));
            if step > 0 {
                tracker.damage(&scene, size, &mut boxes, &mut damage);
                drawn += if damage.is_full() {
                    surface
                } else {
                    damage.rects().iter().map(|r| r.area() as f32).sum::<f32>()
                };
            }
            tracker.commit(&scene, size);
        }
        let share = drawn / frames as f32 / surface * 100.0;
        let name = format!("{:?}/{:?}", decorations.layers[0].kind(), decorations.layers[0].zone());
        eprintln!("budget: {name} redraws {share:.1}% of the window a frame");
        shares.push((name, share));
    }
    let neon = shares.iter().find(|(n, _)| n.starts_with("Neon")).map(|(_, s)| *s).unwrap();
    assert!(neon < 10.0, "the neon edge redraws only its edges: {neon:.1}%");
}
