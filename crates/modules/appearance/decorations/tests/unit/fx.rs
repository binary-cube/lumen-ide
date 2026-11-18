//! Plan 42's effects: their layouts (sized to the window, at most so many), their motion
//! (deterministic, moving the way each should), what hides them (their zone), and Matrix's
//! characters drawn as glyphs of the app's icons.

use super::*;
use theming::decorations::Zone;
use ui_components::decorations::STATIC_TIME;

const WINDOW: Rect =
    Rect { origin: Point { x: 0.0, y: 0.0 }, size: kernel::Size { width: 2400.0, height: 1400.0 } };

fn spec(kind: LayerKind) -> EffectSpec {
    EffectSpec::new(kind)
}

fn icons() -> icons::Icons {
    let mut icons = icons::Icons::builtin();
    for g in crate::glyphs::all() {
        icons.register(g.name, g.svg);
    }
    icons
}

/// Every glyph is a valid icon of the app's set, square; each style has its characters
/// (katakana with the digits, as in the film).
#[test]
fn matrix_s_characters_are_icons_of_the_app_s_set() {
    let icons = icons();
    for g in crate::glyphs::all() {
        let id = icons.id(g.name).expect(g.name);
        let key = icons.key_styled(id, 28, icons::IconStyle::Regular);
        let glyph = icons.rasterize_key(key).unwrap_or_else(|| panic!("{} draws", g.name));
        assert_eq!((glyph.width, glyph.height), (28, 28));
        assert!(glyph.data.iter().any(|a| *a > 128), "{} has strokes", g.name);
    }
    let katakana = crate::glyphs::of_style("katakana");
    assert!(
        katakana.iter().any(|n| n.starts_with("fx-k-"))
            && katakana.iter().any(|n| n.starts_with("fx-d-"))
    );
    assert_eq!(crate::glyphs::of_style("binary"), ["fx-d-0", "fx-d-1"]);
}

/// Matrix: columns across the window (more when denser), each head falling down and
/// coming back at the top; the head brightest, the trail fading; drawn as glyphs, none
/// under the cards that hide it.
#[test]
fn matrix_rains_down_its_columns() {
    let e = spec(LayerKind::Matrix);
    let (cell, columns) = matrix_layout(&e, WINDOW, 2.0);
    assert_eq!(cell, 28.0);
    let dense = matrix_layout(&EffectSpec { density: 1.0, ..e.clone() }, WINDOW, 2.0).1;
    assert!(dense.len() > columns.len() && dense.len() <= MAX_COLUMNS);
    let rows = WINDOW.size.height / cell;
    let col = &columns[0];
    let a = matrix_head(col, rows, 1.0, e.speed);
    let b = matrix_head(col, rows, 1.2, e.speed);
    assert!(b > a || b < 1.0, "falls (or starts over at the top)");
    let icons = icons();
    let mut painter = MatrixPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    let mut layer = Layer::default();
    let f = FxFrame {
        window: WINDOW,
        scale: 2.0,
        time: STATIC_TIME,
        covered: &[],
        icons: Some(&icons),
    };
    painter.paint(&e, &f, &mut layer);
    assert!(layer.glyphs.len() > 100, "{}", layer.glyphs.len());
    assert!(layer.images.is_empty() && layer.quads.is_empty(), "glyphs only: one pass");
    let brightest = layer.glyphs.iter().map(|g| g.color.a).fold(0.0, f32::max);
    assert!(brightest <= 1.0 && brightest > e.opacity);
    // Hidden under a card covering the left half: nothing drawn there.
    let card = [Rect::new(0.0, 0.0, 1200.0, 1400.0)];
    let mut covered = Layer::default();
    painter.paint(&e, &FxFrame { covered: &card, ..f }, &mut covered);
    assert!(covered.glyphs.iter().all(|g| g.origin.x + cell > 1200.0));
    assert!(covered.glyphs.len() < layer.glyphs.len());
    // Without the icons: nothing.
    let mut none = Layer::default();
    painter.paint(&e, &FxFrame { icons: None, ..f }, &mut none);
    assert!(none.glyphs.is_empty());
}

/// The stars: warp ones come out from the middle, bigger as they come; drifting ones stay
/// in their row; all inside the window.
#[test]
fn the_stars_come_toward_you_or_drift() {
    let warp = spec(LayerKind::Starfield);
    let stars = star_layout(&warp, WINDOW, 2.0);
    assert!(!stars.is_empty() && stars.len() <= MAX_STARS);
    let s = &stars[3];
    let mid = Point::new(1200.0, 700.0);
    let (p0, d0, _) = star_at(s, &warp, WINDOW, 2.0, 0.0);
    let (p1, d1, _) = star_at(s, &warp, WINDOW, 2.0, 0.5);
    let far = |p: Point| (p.x - mid.x).hypot(p.y - mid.y);
    assert!(far(p1) > far(p0) || far(p1) < 40.0, "outward (or born again in the middle)");
    assert!(d1 >= d0 || far(p1) < 40.0);
    let drift = EffectSpec { style: "drift", ..warp };
    let (a, ..) = star_at(s, &drift, WINDOW, 2.0, 0.0);
    let (b, ..) = star_at(s, &drift, WINDOW, 2.0, 5.0);
    assert_eq!(a.y, b.y, "its row");
    assert!(WINDOW.contains(a) && WINDOW.contains(b));
}

/// The aurora: its bands at the top, repeated across the window (images a period wide,
/// joined), moving.
#[test]
fn the_aurora_waves_at_the_top() {
    let e = spec(LayerKind::Aurora);
    let mut painter = AuroraPainter::default();
    painter.sync_art(&e, 2.0);
    let mut layer = Layer::default();
    let f = FxFrame { window: WINDOW, scale: 2.0, time: 3.0, covered: &[], icons: None };
    painter.paint(&e, &f, &mut layer);
    assert!(!layer.images.is_empty());
    let band = &layer.images[0];
    assert_eq!(band.bounds.size.width, (AURORA_PERIOD * 2.0).round());
    assert!(layer.images.iter().all(|i| i.bounds.min_y() < 400.0), "at the top");
    // Across the whole width.
    let first = layer.images.iter().filter(|i| i.image.id == band.image.id);
    let (min, max) = first
        .fold((f32::MAX, f32::MIN), |(a, b), i| (a.min(i.bounds.min_x()), b.max(i.bounds.max_x())));
    assert!(min <= 0.0 && max >= 2400.0);
    let mut later = Layer::default();
    painter.paint(&e, &FxFrame { time: 4.0, ..f }, &mut later);
    assert_ne!(later.images[0].bounds.min_x(), layer.images[0].bounds.min_x(), "it moves");
}

/// Fireflies wander near home and pulse; bubbles rise and pop at the top.
#[test]
fn fireflies_wander_and_bubbles_rise() {
    let e = spec(LayerKind::Fireflies);
    let flies = fly_layout(&e, WINDOW, 2.0);
    assert!(!flies.is_empty() && flies.len() <= MAX_FLIES);
    let levels: Vec<f32> =
        (0..40).map(|t| fly_at(&flies[0], &e, WINDOW, t as f32 * 0.25).1).collect();
    let (lo, hi) = levels.iter().fold((1.0f32, 0.0f32), |(a, b), l| (a.min(*l), b.max(*l)));
    assert!(hi - lo > 0.2, "it pulses: {lo}..{hi}");
    let b = spec(LayerKind::Bubbles);
    let bubbles = bubble_layout(&b, WINDOW, 2.0);
    assert!(!bubbles.is_empty() && bubbles.len() <= MAX_BUBBLES);
    let (p0, ..) = bubble_at(&bubbles[0], &b, WINDOW, 2.0, 1.0);
    let (p1, ..) = bubble_at(&bubbles[0], &b, WINDOW, 2.0, 1.1);
    assert!(p1.y < p0.y || p1.y > 1300.0, "up (or born again at the bottom)");
    // At the top of its way it swells and fades.
    let (_, swell, alpha) = (0..2000)
        .map(|t| bubble_at(&bubbles[0], &b, WINDOW, 2.0, t as f32 * 0.01))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(swell > 1.2 && alpha < 0.5);
}

/// Fireworks: rare bursts — one in bloom in the still frame, none in the quiet between —
/// its sparks spreading and falling.
#[test]
fn fireworks_burst_now_and_then() {
    let e = spec(LayerKind::Fireworks);
    let mut painter = FireworkPainter::default();
    painter.sync_art(&e, 2.0);
    let draw = |t: f32| {
        let mut layer = Layer::default();
        painter.paint(
            &e,
            &FxFrame { window: WINDOW, scale: 2.0, time: t, covered: &[], icons: None },
            &mut layer,
        );
        layer
    };
    let still = draw(STATIC_TIME);
    assert!(still.quads.len() > 20, "in bloom: {}", still.quads.len());
    let period = burst_period(e.density);
    assert!(period > BURST_LIFE, "a quiet time between bursts");
    let quiet = draw(STATIC_TIME + BURST_LIFE + 0.05);
    assert!(quiet.quads.is_empty() || period < BURST_LIFE + 0.1);
    let early = draw(STATIC_TIME - 0.4);
    let spread = |l: &Layer| {
        let xs: Vec<f32> = l.quads.iter().map(|q| q.bounds.min_x()).collect();
        xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min)
    };
    assert!(spread(&still) > spread(&early), "spreading");
}

/// The zones: around the panels an effect hides under every card; behind the panels only
/// under the editor; everywhere under none.
#[test]
fn an_effect_s_zone_says_what_hides_it() {
    use std::sync::Arc;
    use theming::decorations::{Decorations, LayerSpec};
    use ui_components::decorations::{DecorationFrame, Geometry, WindowDecorations};
    let side = Rect::new(0.0, 100.0, 500.0, 1200.0);
    let editor = Rect::new(520.0, 100.0, 1860.0, 1200.0);
    let cards = [editor, side];
    let count = |zone: Zone| {
        let e = EffectSpec { zone, density: 1.0, ..spec(LayerKind::Starfield) };
        let mut painter = crate::paint::Painter::new(Arc::new(Decorations {
            layers: vec![LayerSpec::Effect(e)],
            dark: true,
        }));
        let mut layer = Layer::default();
        let frame = DecorationFrame {
            geometry: Geometry {
                window: WINDOW,
                scale: 2.0,
                top_inset: 0.0,
                top_bar: 80.0,
                keep_clear: &[],
            },
            covered: &cards,
            editor: &cards[..1],
            time: STATIC_TIME,
            icons: None,
        };
        painter.paint(&frame, &mut layer);
        layer.quads.len()
    };
    let (edges, panels, everywhere) =
        (count(Zone::Edges), count(Zone::Panels), count(Zone::Everywhere));
    assert!(edges < panels && panels < everywhere, "{edges} < {panels} < {everywhere}");
}

fn frame(time: f32) -> FxFrame<'static> {
    FxFrame { window: WINDOW, scale: 2.0, time, covered: &[], icons: None }
}

/// A shooting star crosses now and then (one half way in the still frame), its streak
/// fading behind its head.
#[test]
fn a_shooting_star_crosses_now_and_then() {
    let e = spec(LayerKind::ShootingStars);
    let mut still = Layer::default();
    paint_streaks(&e, &frame(STATIC_TIME), &mut still);
    assert!(still.quads.len() > 40, "{}", still.quads.len());
    let alphas: Vec<f32> = still.quads.iter().map(|q| q.color.a).collect();
    assert!(alphas.first() > alphas.last(), "the head brightest");
    let mut later = Layer::default();
    paint_streaks(&e, &frame(STATIC_TIME + streak_life(e.speed) + 0.1), &mut later);
    assert!(later.quads.is_empty() || streak_period(e.density) < streak_life(e.speed) * 2.0);
}

/// The waves sit on the bottom edge, across the whole width, moving.
#[test]
fn the_waves_move_along_the_bottom() {
    let e = spec(LayerKind::Waves);
    let mut painter = WavePainter::default();
    painter.sync_art(&e, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(3.0), &mut layer);
    assert!(!layer.images.is_empty());
    assert!(layer.images.iter().all(|i| i.bounds.max_y() > 1300.0 && i.bounds.min_y() > 900.0));
    let mut later = Layer::default();
    painter.paint(&e, &frame(4.0), &mut later);
    assert_ne!(layer.images[0].bounds.min_x(), later.images[0].bounds.min_x());
    assert_eq!(WavePainter::count(&EffectSpec { density: 1.0, ..e.clone() }), 3);
}

/// The neon edge: four sides a ring (never one quad over the window), its halo fainter;
/// pulsing, or its hue turning through its colors.
#[test]
fn the_neon_edge_pulses_or_turns_its_hue() {
    let e = spec(LayerKind::Neon);
    let mut layer = Layer::default();
    paint_neon(&e, &frame(1.0), &mut layer);
    assert_eq!(layer.quads.len() % 4, 0);
    assert!(
        layer.quads.iter().all(|q| q.bounds.size.width < 100.0 || q.bounds.size.height < 100.0),
        "edges only"
    );
    let (_, a) = neon_color(&e, 0.0);
    let (_, b) = neon_color(&e, 1.0);
    assert_ne!(a, b, "it pulses");
    let hue = EffectSpec { style: "hue", ..e };
    assert_ne!(neon_color(&hue, 0.0).0, neon_color(&hue, 2.0).0, "its hue turns");
}

/// A constellation: drifting nodes, joined by dotted lines only when near (a couple each).
#[test]
fn a_constellation_joins_its_near_stars() {
    let e = spec(LayerKind::Constellation);
    let mut painter = ConstellationPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut layer);
    let nodes = node_layout(&e, WINDOW, 2.0).len();
    assert!(nodes > 5 && nodes <= MAX_NODES);
    assert!(layer.quads.len() > nodes, "nodes and their links");
    let far = EffectSpec { size: 0.0, ..e.clone() };
    let mut fewer = Layer::default();
    painter.paint(&far, &frame(STATIC_TIME), &mut fewer);
    assert!(fewer.quads.len() < layer.quads.len(), "a shorter reach: fewer links");
}

/// The synthwave grid: its sun on the horizon, its fan below it, lines coming toward you.
#[test]
fn the_synthwave_grid_flows_toward_you() {
    let e = EffectSpec { glow: 0.0, ..spec(LayerKind::Synthwave) };
    let mut painter = SynthwavePainter::default();
    painter.sync_art(&e, WINDOW, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(1.0), &mut layer);
    assert_eq!(layer.images.len(), 2, "the sun and the fan");
    let horizon = layer.quads[0].bounds.min_y();
    assert!(layer.quads.iter().all(|q| q.bounds.min_y() >= horizon));
    let mut later = Layer::default();
    painter.paint(&e, &frame(1.5), &mut later);
    let ys = |l: &Layer| l.quads.iter().map(|q| q.bounds.min_y()).sum::<f32>();
    assert_ne!(ys(&layer), ys(&later), "flowing");
    let grid = EffectSpec { style: "grid", ..e };
    assert!(later.quads.iter().all(|q| q.bounds.min_y().fract() == 0.0), "at whole pixels");
    let mut alone = SynthwavePainter::default();
    alone.sync_art(&grid, WINDOW, 2.0);
    let mut l = Layer::default();
    alone.paint(&grid, &frame(1.0), &mut l);
    assert_eq!(l.images.len(), 1, "no sun");
}

/// Lightning: a strike now and then (one lit in the still frame: the sky, its halo, a
/// forked bolt down from the top), nothing at all between strikes; the flash alone without
/// a bolt.
#[test]
fn lightning_strikes_now_and_then() {
    let e = spec(LayerKind::Lightning);
    let mut painter = LightningPainter::default();
    painter.sync_art(&e);
    let mut still = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut still);
    assert_eq!(still.images.len(), 2, "the sky and the halo");
    assert!(still.quads.len() > 100, "the bolt: {}", still.quads.len());
    let top = still.quads.iter().map(|q| q.bounds.min_y()).fold(f32::MAX, f32::min);
    let low = still.quads.iter().map(|q| q.bounds.max_y()).fold(f32::MIN, f32::max);
    assert!(top < 50.0 && low > 400.0, "down from the top: {top}..{low}");
    let period = strike_period(e.density);
    assert!(period > STRIKE_LIFE * 3.0, "rare");
    let mut quiet = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME + STRIKE_LIFE + 0.1), &mut quiet);
    assert!(quiet.images.is_empty() && quiet.quads.is_empty(), "nothing between strikes");
    assert!(strike_level(0.0) > strike_level(0.1), "it flickers and fades");
    assert!(strike_level(0.34) > strike_level(0.3), "and flashes again");
    let flash = EffectSpec { style: "flash", ..e };
    let mut alone = Layer::default();
    painter.paint(&flash, &frame(STATIC_TIME), &mut alone);
    assert!(alone.quads.is_empty() && !alone.images.is_empty(), "no bolt");
}

/// Raindrops: still a while, then sliding down (faster and faster), a trail of dots above
/// them while they slide.
#[test]
fn raindrops_slide_down_the_glass() {
    let e = spec(LayerKind::Raindrops);
    let drops = drop_layout(&e, WINDOW, 2.0);
    assert!(!drops.is_empty() && drops.len() <= MAX_DROPS);
    let d = &drops[0];
    let period = d.period / (0.4 + 1.2 * e.speed);
    let at = |share: f32| {
        let t = (share - d.phase).rem_euclid(1.0) * period;
        drop_at(d, &e, WINDOW, 2.0, t)
    };
    assert_eq!(at(0.2).0, at(0.4).0, "still at first");
    let (a, b, c) = (at(0.6).0.y, at(0.7).0.y, at(0.8).0.y);
    assert!(b > a && c - b > b - a, "down, faster and faster: {a} {b} {c}");
    let mut painter = RaindropPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut layer);
    assert!(!layer.images.is_empty(), "the drops");
    assert!(!layer.quads.is_empty(), "the trails of those sliding");
}

/// Clouds float along the top, the far ones smaller and fainter, at whole pixels (a frame
/// they moved by less is the same frame).
#[test]
fn clouds_float_along_the_top() {
    let e = spec(LayerKind::Clouds);
    let clouds = cloud_layout(&e, WINDOW, 2.0);
    assert!(!clouds.is_empty() && clouds.len() <= MAX_CLOUDS);
    assert!(clouds.windows(2).all(|w| w[0].depth <= w[1].depth), "the far ones first");
    let mut painter = CloudPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(3.0), &mut layer);
    assert!(!layer.images.is_empty());
    assert!(layer.images.iter().all(|i| i.bounds.min_y() < 400.0), "at the top");
    assert!(layer.images.iter().all(|i| i.bounds.min_x().fract() == 0.0));
    let (near, far) = (&clouds[clouds.len() - 1], &clouds[0]);
    let (rn, on) = cloud_at(near, &e, WINDOW, 2.0, 0.0);
    let (rf, of) = cloud_at(far, &e, WINDOW, 2.0, 0.0);
    assert!(on >= of && rn.size.width * 0.8 >= rf.size.width * 0.6);
    let (r0, _) = cloud_at(near, &e, WINDOW, 2.0, 0.0);
    let (r1, _) = cloud_at(near, &e, WINDOW, 2.0, 2.0);
    assert_ne!(r0.min_x(), r1.min_x(), "it floats");
    // Small images, stretched: a cloud is drawn at a quarter of its pixels or less.
    let image = &layer.images[0];
    assert!(image.image.width as f32 * 3.0 < image.bounds.size.width);
}

/// Fog lies along the bottom (or veils the window), its bands repeated across the width,
/// drifting by whole pixels.
#[test]
fn fog_drifts_along_the_bottom() {
    let e = spec(LayerKind::Fog);
    let mut painter = FogPainter::default();
    painter.sync_art(&e);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(3.0), &mut layer);
    assert!(!layer.images.is_empty());
    assert!(layer.images.iter().all(|i| i.bounds.max_y() >= 1390.0), "on the ground");
    let max = layer.images.iter().map(|i| i.bounds.max_x()).fold(f32::MIN, f32::max);
    assert!(max >= 2400.0, "across");
    let mut later = Layer::default();
    painter.paint(&e, &frame(6.0), &mut later);
    assert_ne!(layer.images[0].bounds.min_x(), later.images[0].bounds.min_x(), "drifting");
    assert!(later.images.iter().all(|i| i.bounds.min_x().fract() == 0.0));
    let veil = EffectSpec { style: "veil", ..e };
    let mut v = FogPainter::default();
    v.sync_art(&veil);
    let mut l = Layer::default();
    v.paint(&veil, &frame(3.0), &mut l);
    assert!(l.images.iter().any(|i| i.bounds.max_y() < 1200.0), "veils across");
}

/// Butterflies rest (wings slowly opening and closing), then fly a short way, flapping.
#[test]
fn butterflies_rest_and_fly() {
    let e = spec(LayerKind::Butterflies);
    let flock = butterfly_layout(&e, WINDOW, 2.0);
    assert!(!flock.is_empty() && flock.len() <= MAX_BUTTERFLIES);
    let b = &flock[0];
    let pace = 0.5 + 1.2 * e.speed;
    let cycle = b.rest + b.flight;
    // The start of a cycle (resting), then the middle of its flight.
    let start = (10.0 * cycle - b.phase * cycle) / pace;
    let (rest_a, _) = butterfly_at(b, &e, WINDOW, 2.0, start + 0.1 / pace);
    let (rest_b, _) = butterfly_at(b, &e, WINDOW, 2.0, start + (b.rest - 0.1) / pace);
    assert_eq!(rest_a, rest_b, "resting in place");
    let flying = start + (b.rest + b.flight / 2.0) / pace;
    let frames: Vec<usize> =
        (0..12).map(|k| butterfly_at(b, &e, WINDOW, 2.0, flying + k as f32 * 0.02).1).collect();
    assert!(frames.iter().any(|f| *f >= 2), "flapping: {frames:?}");
    let (mid, _) = butterfly_at(b, &e, WINDOW, 2.0, flying);
    assert!(WINDOW.contains(mid) && (mid.x - rest_a.x).hypot(mid.y - rest_a.y) < 700.0, "short");
    let mut painter = ButterflyPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e, 2.0);
    assert_eq!(painter.images().count(), e.colors.len() * WING_FRAMES);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut layer);
    assert_eq!(layer.images.len(), flock.len());
}

/// Balloons rise, swaying, each with its string below it.
#[test]
fn balloons_rise_on_their_strings() {
    let e = spec(LayerKind::Balloons);
    let balloons = balloon_layout(&e, WINDOW, 2.0);
    assert!(!balloons.is_empty() && balloons.len() <= MAX_BALLOONS);
    let (p0, _) = balloon_at(&balloons[0], &e, WINDOW, 2.0, 1.0);
    let (p1, _) = balloon_at(&balloons[0], &e, WINDOW, 2.0, 1.2);
    assert!(p1.y < p0.y || p1.y > 1400.0, "up (or born again below)");
    let mut painter = BalloonPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut layer);
    let shown = layer.images.len();
    assert!(shown > 0);
    assert!(layer.quads.len() > shown * 10, "a string each");
    let body = &layer.images[0].bounds;
    assert!(
        layer.quads.iter().any(|q| q.bounds.min_y() >= body.max_y() - 2.0
            && (q.bounds.min_x() - (body.min_x() + body.size.width / 2.0)).abs() < body.size.width),
        "under its balloon"
    );
}

/// Hearts float up, fading in and out, swelling with each beat.
#[test]
fn hearts_float_up_and_beat() {
    let e = spec(LayerKind::Hearts);
    let hearts = heart_layout(&e, WINDOW, 2.0);
    assert!(!hearts.is_empty() && hearts.len() <= MAX_HEARTS);
    let h = &hearts[0];
    let sizes: Vec<f32> =
        (0..60).map(|k| heart_at(h, &e, WINDOW, 2.0, k as f32 * 0.05).1).collect();
    let (lo, hi) = sizes.iter().fold((2.0f32, 0.0f32), |(a, b), s| (a.min(*s), b.max(*s)));
    assert!(hi - lo > 0.04 && hi <= 1.0, "it beats: {lo}..{hi}");
    let (p0, ..) = heart_at(h, &e, WINDOW, 2.0, 1.0);
    let (p1, ..) = heart_at(h, &e, WINDOW, 2.0, 1.5);
    assert!(p1.y < p0.y || p1.y > 1400.0, "up");
    let mut painter = HeartPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e, 2.0);
    assert_eq!(painter.images().count(), e.colors.len() * 2);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut layer);
    assert!(!layer.images.is_empty());
}

/// Bats cross now and then (off the window between their crossings), flapping.
#[test]
fn bats_flit_across() {
    let e = spec(LayerKind::Bats);
    let bats = bat_layout(&e, WINDOW, 2.0);
    assert!(!bats.is_empty() && bats.len() <= MAX_BATS);
    let b = &bats[0];
    let seen: Vec<Option<(Point, usize)>> =
        (0..400).map(|k| bat_at(b, &e, WINDOW, 2.0, k as f32 * 0.1)).collect();
    assert!(seen.iter().any(Option::is_some) && seen.iter().any(Option::is_none));
    let frames: Vec<usize> = seen.iter().flatten().map(|(_, f)| *f).collect();
    assert!((0..BAT_FRAMES).all(|f| frames.contains(&f)), "flapping");
    let xs: Vec<f32> = seen.iter().flatten().map(|(p, _)| p.x).take(5).collect();
    assert!(xs.windows(2).all(|w| (w[1] > w[0]) == b.left), "across, its way");
    let mut painter = BatPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e, 2.0);
    assert_eq!(painter.images().count(), e.colors.len() * BAT_FRAMES);
}

/// The lava lamp: blobs rise and sink; two of a color near each other get a neck between
/// them (more images than blobs), none when they're far apart.
#[test]
fn the_lava_lamp_s_blobs_merge_and_part() {
    let e = spec(LayerKind::LavaLamp);
    let blobs = blob_layout(&e, WINDOW, 2.0);
    assert!(blobs.len() >= 2 && blobs.len() <= MAX_BLOBS);
    let ys: Vec<f32> =
        (0..40).map(|k| blob_at(&blobs[0], &e, WINDOW, 2.0, k as f32 * 3.0).0.y).collect();
    let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
    assert!(hi - lo > 400.0, "it rises and sinks: {lo}..{hi}");
    let mut painter = LavaPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    painter.sync_art(&e);
    let necks = (0..200)
        .map(|k| {
            let mut layer = Layer::default();
            painter.paint(&e, &frame(k as f32 * 1.7), &mut layer);
            layer.images.len() - blobs.len()
        })
        .collect::<Vec<_>>();
    assert!(necks.iter().any(|n| *n > 0), "they merge now and then");
    assert!(necks.contains(&0), "and part");
    // One small image per color, stretched to every size.
    assert_eq!(painter.images().count(), e.colors.len());
}

/// The retro CRT: scanlines over its zone (a few tiles, the rows exact), the curved screen
/// shading its edges with the lines only there, a rare glitch; never behind the editor.
#[test]
fn the_crt_keeps_to_its_zone() {
    use theming::decorations::{FieldValue, LayerSpec};
    let e = spec(LayerKind::Crt);
    let mut painter = CrtPainter::default();
    painter.sync_art(&e, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(3.0), &mut layer);
    assert!(!layer.images.is_empty() && layer.images.len() < 10, "{}", layer.images.len());
    let tile = &layer.images[0];
    assert_eq!(tile.bounds.size.height, tile.image.height as f32, "rows drawn one to one");
    assert_eq!(tile.bounds.size.width, 2400.0);
    // The glitch: in the still frame, then gone.
    let mut still = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut still);
    assert!(!still.quads.is_empty());
    let mut after = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME + GLITCH_LIFE + 0.05), &mut after);
    assert!(after.quads.is_empty());
    // Curved: the lines and the glitch only near the edges.
    let curved = EffectSpec { style: "curved", ..e.clone() };
    let mut p = CrtPainter::default();
    p.sync_art(&curved, 2.0);
    let mut l = Layer::default();
    p.paint(&curved, &frame(STATIC_TIME), &mut l);
    let reach = CrtPainter::reach(&curved, 2.0);
    assert!(l.quads.iter().all(|q| q.bounds.size.width <= reach));
    // Behind the editor: refused, and read as behind the panels.
    let mut layer = LayerSpec::new(LayerKind::Crt);
    assert!(layer.set("zone", &FieldValue::Text("everywhere".into())).is_err());
    assert!(layer.set("zone", &FieldValue::Text("panels".into())).is_ok());
    let sneaked = LayerSpec::Effect(EffectSpec { zone: Zone::Everywhere, ..e });
    assert_eq!(sneaked.zone(), Zone::Panels);
    assert_eq!(sneaked.clamped().zone(), Zone::Panels);
}

/// The grid's horizon glows (plan 45): fainter, wider bands round the line, still from one
/// frame to the next; the city style puts a skyline on the horizon instead of the sun.
#[test]
fn the_synthwave_horizon_glows_and_a_city_stands_on_it() {
    let e = spec(LayerKind::Synthwave);
    let mut painter = SynthwavePainter::default();
    painter.sync_art(&e, WINDOW, 2.0);
    let mut glowing = Layer::default();
    painter.paint(&e, &frame(1.0), &mut glowing);
    let horizon = glowing.quads[0].bounds.min_y();
    let halo: Vec<&Quad> = glowing.quads.iter().filter(|q| q.bounds.min_y() < horizon).collect();
    assert_eq!(halo.len(), 3, "three bands of glow");
    assert!(halo.iter().all(|q| q.color.a < glowing.quads[0].color.a));
    let mut later = Layer::default();
    painter.paint(&e, &frame(1.4), &mut later);
    let still = |l: &Layer| {
        l.quads.iter().filter(|q| q.bounds.min_y() < horizon).map(|q| q.bounds).collect::<Vec<_>>()
    };
    assert_eq!(still(&glowing), still(&later), "the glow doesn't move");
    let city = EffectSpec { style: "city", ..e };
    let mut p = SynthwavePainter::default();
    p.sync_art(&city, WINDOW, 2.0);
    assert_eq!(p.images().count(), 2, "the fan and the skyline, no sun");
    let mut l = Layer::default();
    p.paint(&city, &frame(1.0), &mut l);
    let skyline = &l.images[0];
    assert_eq!(skyline.bounds.max_y(), horizon, "standing on the horizon");
    assert_eq!(skyline.bounds.size.width, WINDOW.size.width);
}

/// A glitch (plan 45): rare — one half way in the still frame, nothing at all between
/// glitches —, its bands split in two colors and jumping; blocks are shorter than the
/// window; never behind the editor.
#[test]
fn a_glitch_comes_now_and_then_and_leaves_nothing() {
    use theming::decorations::{FieldValue, LayerSpec};
    let e = spec(LayerKind::Glitch);
    assert!(glitch_every(e.density) >= 4.0 && glitch_every(0.0) == 30.0);
    let (_, age) = glitch_at(e.density, STATIC_TIME).expect("one in the still frame");
    assert!((age - GLITCH_FLASH / 2.0).abs() < 1e-3);
    let mut still = Layer::default();
    paint_glitch(&e, &frame(STATIC_TIME), &mut still);
    assert!(still.quads.len() >= 4, "{}", still.quads.len());
    assert!(still.images.is_empty() && still.glyphs.is_empty());
    let colors: Vec<Rgba> = still.quads.iter().map(|q| q.color.with_alpha(1.0)).collect();
    assert!(colors.contains(&e.colors[0]) && colors.contains(&e.colors[1]), "split in two colors");
    let mut jumped = Layer::default();
    paint_glitch(&e, &frame(STATIC_TIME + 0.04), &mut jumped);
    let xs = |l: &Layer| l.quads.iter().map(|q| q.bounds.min_x()).collect::<Vec<_>>();
    assert_ne!(xs(&still), xs(&jumped), "it jumps");
    for t in [STATIC_TIME + GLITCH_FLASH, STATIC_TIME + 1.0, STATIC_TIME + 3.0] {
        let mut quiet = Layer::default();
        paint_glitch(&e, &frame(t), &mut quiet);
        assert!(quiet.quads.is_empty(), "nothing between glitches ({t})");
    }
    let blocks = EffectSpec { style: "blocks", ..e.clone() };
    let mut b = Layer::default();
    paint_glitch(&blocks, &frame(STATIC_TIME), &mut b);
    assert!(!b.quads.is_empty() && b.quads.iter().all(|q| q.bounds.size.width < 2400.0));
    let mut layer = LayerSpec::new(LayerKind::Glitch);
    assert!(layer.set("zone", &FieldValue::Text("everywhere".into())).is_err());
    let sneaked = LayerSpec::Effect(EffectSpec { zone: Zone::Everywhere, ..e });
    assert_eq!(sneaked.zone(), Zone::Panels);
    assert_eq!(sneaked.clamped().zone(), Zone::Panels);
}

/// Neon rain: thin streaks falling (their heads lower each frame), each landing in a splash
/// at the bottom (a reflection widening, drops jumping), none under the cards hiding it.
#[test]
fn neon_rain_falls_and_splashes() {
    let e = spec(LayerKind::NeonRain);
    let streaks = streak_layout(&e, WINDOW, 2.0);
    assert!(streaks.len() > 20 && streaks.len() <= MAX_STREAKS);
    let dense = streak_layout(&EffectSpec { density: 1.0, ..e.clone() }, WINDOW, 2.0);
    assert!(dense.len() > streaks.len());
    let s = &streaks[0];
    let states: Vec<RainAt> =
        (0..400).map(|k| streak_at(s, &e, WINDOW, 2.0, k as f32 * 0.01)).collect();
    assert!(states.iter().any(|r| matches!(r, RainAt::Splash { .. })), "it lands");
    let heads: Vec<f32> = states
        .iter()
        .filter_map(|r| match r {
            RainAt::Falling { head, .. } => Some(head.y),
            _ => None,
        })
        .take(3)
        .collect();
    assert!(heads.windows(2).all(|w| w[1] > w[0] || w[1] < 50.0), "down: {heads:?}");
    let mut painter = NeonRainPainter::default();
    painter.sync_layout(&e, WINDOW, 2.0);
    let mut layer = Layer::default();
    painter.paint(&e, &frame(STATIC_TIME), &mut layer);
    assert!(layer.quads.len() > streaks.len(), "a few quads a streak");
    assert!(layer.images.is_empty(), "no art");
    let thin = layer.quads.iter().filter(|q| q.bounds.size.width <= 2.0).count();
    assert!(thin * 2 > layer.quads.len(), "fine streaks");
    // Behind a card covering everything (and past the top, where streaks start): nothing.
    let card = [Rect::new(-10.0, -400.0, 2420.0, 1820.0)];
    let mut covered = Layer::default();
    painter.paint(&e, &FxFrame { covered: &card, ..frame(STATIC_TIME) }, &mut covered);
    assert!(covered.quads.is_empty());
}

/// Holographic signs: one to four, each in its corner clear of the bars, a frame, a halo and
/// two or three glyphs; steady, flickering now and then (dimmer and a little off).
#[test]
fn holographic_signs_stand_in_the_corners_and_flicker() {
    let e = spec(LayerKind::HoloSigns);
    let signs = sign_layout(&e);
    assert_eq!(sign_layout(&EffectSpec { density: 0.0, ..e.clone() }).len(), 1);
    assert_eq!(sign_layout(&EffectSpec { density: 1.0, ..e.clone() }).len(), MAX_SIGNS);
    let rects: Vec<Rect> = signs.iter().map(|s| sign_rect(s, &e, WINDOW, 2.0)).collect();
    for r in &rects {
        assert!(r.min_y() >= 100.0 && r.max_y() <= 1400.0 - 70.0, "clear of the bars: {r:?}");
        assert!(r.min_x() < 300.0 || r.max_x() > 2100.0, "in a corner: {r:?}");
    }
    let s = &signs[0];
    let levels: Vec<f32> = (0..2000).map(|k| sign_level(s, &e, k as f32 * 0.01)).collect();
    let steady = levels.iter().filter(|l| **l == 1.0).count();
    assert!(steady > 1800, "steady most of the time: {steady}");
    assert!(levels.iter().any(|l| *l < 0.3), "a flicker now and then");
    let icons = icons();
    let mut painter = HoloPainter::default();
    painter.sync_layout(&e);
    painter.sync_art(&e);
    assert_eq!(painter.images().count(), e.colors.len(), "a halo per color");
    let mut layer = Layer::default();
    let f = FxFrame { icons: Some(&icons), ..frame(STATIC_TIME) };
    painter.paint(&e, &f, &mut layer);
    assert_eq!(layer.quads.len(), signs.len(), "a frame each");
    assert_eq!(layer.images.len(), signs.len(), "a halo each");
    let glyphs: usize = signs.iter().map(|s| usize::from(s.count)).sum();
    assert_eq!(layer.glyphs.len(), glyphs);
    let mut none = Layer::default();
    painter.paint(&e, &FxFrame { icons: None, ..f }, &mut none);
    assert!(none.glyphs.is_empty() && none.quads.is_empty(), "no icons: nothing");
}
