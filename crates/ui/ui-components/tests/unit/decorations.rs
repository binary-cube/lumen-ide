//! The decorations' model: each kind's layout keeps to the window's edges and clear of what
//! the top bar shows, at every size and scale; the animation is deterministic and bounded,
//! every particle stays in the window; the clock
//! asks for frames only while the window is in use, at most [`FPS`] a second, and stops
//! (still frame, nothing asked) when hidden, unfocused or idle.

use super::*;
use theming::decorations::{GarlandSpec, Items, ParticleShape, ParticleSpec};

/// A whole Christmas: a garland, the corners, snow.
struct All {
    garland: GarlandLayout,
    corners: Vec<(Point, bool)>,
    flakes: Vec<Particle>,
    window: Rect,
}

fn layout(g: &Geometry<'_>) -> All {
    All {
        garland: garland_layout(&GarlandSpec::default(), g),
        corners: corners_layout(g),
        flakes: particles_layout(&ParticleSpec::default(), g),
        window: g.window,
    }
}

fn geometry(w: f32, h: f32, scale: f32, keep_clear: &[Rect]) -> Geometry<'_> {
    Geometry {
        window: Rect::new(0.0, 0.0, w, h),
        scale,
        top_inset: 0.0,
        top_bar: 40.0 * scale,
        keep_clear,
    }
}

/// What the top bar shows in a 1200-wide window at `scale`: the project on the left, two
/// buttons and the window's controls on the right.
fn top_bar_content(w: f32, scale: f32) -> Vec<Rect> {
    let s = scale;
    vec![
        Rect::new(18.0 * s, 6.0 * s, 120.0 * s, 28.0 * s),
        Rect::new(150.0 * s, 12.0 * s, 60.0 * s, 16.0 * s),
        Rect::new(w - 200.0 * s, 6.0 * s, 28.0 * s, 28.0 * s),
        Rect::new(w - 168.0 * s, 6.0 * s, 28.0 * s, 28.0 * s),
        Rect::new(w - 120.0 * s, 6.0 * s, 114.0 * s, 28.0 * s),
    ]
}

#[test]
fn modes_are_read_by_their_ids() {
    for mode in DecorationMode::ALL {
        assert_eq!(DecorationMode::parse(mode.id()), Some(mode));
    }
    assert_eq!(DecorationMode::parse("sometimes"), None);
    assert_eq!(DecorationMode::default(), DecorationMode::On);
}

#[test]
fn an_empty_window_lays_out_nothing() {
    let l = layout(&geometry(0.0, 0.0, 2.0, &[]));
    assert!(l.garland.swags.is_empty() && l.garland.lights.is_empty() && l.flakes.is_empty());
    assert!(l.corners.is_empty() && l.garland.sides.is_empty());
}

/// At every width, height and scale: the garland stays on the edges (the top's strip and
/// the sides' strips), the swags cover the top from edge to edge, the ornaments and corners
/// never cross what the top bar shows, every light and flake is in the window.
#[test]
fn the_layout_keeps_to_the_edges_and_clear_of_the_content() {
    for scale in [1.0, 1.5, 2.0] {
        for w in (300..=2600).step_by(77) {
            for h in [240.0, 700.0, 1400.0] {
                let w = w as f32 * scale;
                let h = h * scale;
                let clear = top_bar_content(w, scale);
                let all = layout(&geometry(w, h, scale, &clear));
                let l = &all.garland;
                let shape = l.swag.unwrap();
                let top_strip = SWAG_HEIGHT * scale + 1.0;
                let side_strip = SIDE_WIDTH * scale + 1.0;
                // The swags join from the left edge to the right one.
                let first = l.swags.first().unwrap();
                let last = l.swags.last().unwrap();
                assert!(first.x <= 0.5, "{w}x{h}@{scale}");
                assert!((last.x + shape.width - w).abs() <= 1.5, "{w}x{h}@{scale}");
                for p in &l.swags {
                    assert!(p.y == 0.0 && shape.height <= top_strip);
                }
                for t in &l.sides {
                    let on_edge =
                        if t.right { t.origin.x >= w - side_strip } else { t.origin.x <= 0.0 };
                    assert!(on_edge, "{w}x{h}@{scale}: {t:?}");
                    assert!(t.origin.y + SIDE_TILE * scale <= h * 0.8 + 1.0);
                }
                for light in &l.lights {
                    let c = light.center;
                    let on_top = c.y <= top_strip;
                    let on_side = c.x <= side_strip || c.x >= w - side_strip;
                    assert!(on_top || on_side, "{w}x{h}@{scale}: light at {c:?}");
                    assert!(c.x >= 0.0 && c.x <= w && c.y >= 0.0 && c.y <= h);
                }
                for o in &l.ornaments {
                    let r = ornament_rect(o.hook, scale);
                    let on_top = o.hook.y <= top_strip;
                    if on_top {
                        assert!(
                            clear.iter().all(|k| r.intersect(k).is_none()),
                            "{w}x{h}@{scale}: an ornament over the top bar's content"
                        );
                    }
                    assert!(r.max_y() <= h && r.min_x() >= -1.0 && r.max_x() <= w + 1.0);
                }
                for (p, _) in &all.corners {
                    let r = Rect::new(p.x, p.y, CORNER * scale, CORNER * scale);
                    assert!(clear.iter().all(|k| r.intersect(k).is_none()));
                }
                assert!(all.flakes.len() <= MAX_FLAKES);
                for f in &all.flakes {
                    for t in [0.0, 3.3, 100.0, 9000.0] {
                        let p = flake_at(f, all.window, scale, t, 0.5);
                        assert!(p.x >= 0.0 && p.x <= w, "{p:?}");
                        assert!(p.y >= -8.0 * scale && p.y <= h + 8.0 * scale, "{p:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_top_left_inset_is_left_free() {
    let mut g = geometry(2400.0, 1400.0, 2.0, &[]);
    g.top_inset = 150.0;
    let all = layout(&g);
    let l = &all.garland;
    assert!(l.swags.iter().all(|p| p.x >= 150.0));
    assert!(l.lights.iter().all(|p| p.center.x >= 150.0 || p.center.y >= g.top_bar));
    // The left run hangs from below the top bar there.
    let left = l.sides.iter().filter(|t| !t.right).map(|t| t.origin.y).fold(f32::MAX, f32::min);
    assert_eq!(left, g.top_bar);
    assert!(all.corners.iter().all(|(p, right)| *right || p.x >= 150.0));
}

#[test]
fn the_layout_follows_the_decorations() {
    let g = geometry(2400.0, 1400.0, 2.0, &[]);
    let only_top = GarlandSpec {
        edges: theming::decorations::GarlandEdges::Top,
        ornaments: Vec::new(),
        lights: 0.0,
        ..GarlandSpec::default()
    };
    let l = garland_layout(&only_top, &g);
    assert!(!l.swags.is_empty());
    assert!(l.sides.is_empty() && l.ornaments.is_empty() && l.lights.is_empty());
    // A string with nothing on it: no items either.
    let bare = garland_layout(&GarlandSpec { items: Items::Nothing, ..GarlandSpec::default() }, &g);
    assert!(bare.lights.is_empty() && !bare.swags.is_empty());
    // Denser snow: more flakes; the same layout each time.
    let snow = |density| particles_layout(&ParticleSpec { density, ..ParticleSpec::default() }, &g);
    assert!(snow(1.0).len() > snow(0.2).len() * 3);
    assert_eq!(snow(0.5), snow(0.5));
    // Even items a swag: none at the middle, where its ornament hangs.
    let shape = garland_layout(&GarlandSpec::default(), &g).swag.unwrap();
    assert_eq!(shape.lights % 2, 0);
    let m = shape.middle().x;
    assert!((0..shape.lights).all(|k| (shape.light(k).x - m).abs() > 4.0));
    // Beads sit on the string, bulbs hang below it.
    let beads = garland_layout(&GarlandSpec { items: Items::Beads, ..GarlandSpec::default() }, &g);
    let (b, s) = (beads.swag.unwrap(), shape);
    assert!(b.light(0).y - b.strand_y(b.light(0).x) < s.light(0).y - s.strand_y(s.light(0).x));
    // The colors in turn.
    let colors = ParticleSpec {
        colors: vec![kernel::Rgba::WHITE; 3],
        density: 1.0,
        ..ParticleSpec::default()
    };
    let p = particles_layout(&colors, &g);
    assert!((0..3).all(|c| p.iter().any(|f| f.color == c)));
}

/// Every shape, at every time: in the window (falling ones within a margin above and below,
/// wrapping), the still ones exactly where they started; tumbling ones show each of their
/// orientations in turn; brightness bounded.
#[test]
fn every_particle_stays_in_the_window() {
    let g = geometry(2400.0, 1400.0, 2.0, &[]);
    for shape in ParticleShape::ALL {
        let spec = ParticleSpec { shape: *shape, density: 1.0, ..ParticleSpec::default() };
        let particles = particles_layout(&spec, &g);
        assert!(!particles.is_empty() && particles.len() <= MAX_FLAKES, "{shape:?}");
        let mut frames = [false; 4];
        for p in &particles {
            for t in [0.0, 1.7, 33.3, 900.0] {
                let at = particle_at(p, *shape, g.window, 2.0, t, 0.6);
                assert!(at.x >= 0.0 && at.x <= 2400.0, "{shape:?} {at:?}");
                assert!(at.y >= -32.0 && at.y <= 1432.0, "{shape:?} {at:?}");
                if !shape.falls() {
                    assert_eq!(at, particle_at(p, *shape, g.window, 2.0, 0.0, 0.6), "still");
                }
                let level = particle_level(p, *shape, t, 0.6);
                assert!((0.0..=1.0).contains(&level), "{shape:?}: {level}");
                frames[tumble(p, t)] = true;
            }
        }
        if shape.tumbles() {
            assert!(frames.iter().all(|f| *f), "{shape:?}: every orientation shown");
        }
    }
    // Rain falls much faster than snow.
    let p = particles_layout(&ParticleSpec { density: 1.0, ..ParticleSpec::default() }, &g)[0];
    let fall = |shape| {
        particle_at(&p, shape, g.window, 2.0, 0.1, 0.5).y
            - particle_at(&p, shape, g.window, 2.0, 0.0, 0.5).y
    };
    assert!(fall(ParticleShape::Rain).abs() > 5.0 * fall(ParticleShape::Snow).abs());
}

#[test]
fn a_glow_reaches_in_by_its_width() {
    assert!(glow_reach(0.0, 2.0) < glow_reach(1.0, 2.0));
    assert_eq!(glow_reach(0.5, 2.0), 2.0 * glow_reach(0.5, 1.0));
    assert!(glow_reach(9.0, 1.0) <= 64.0, "bounded");
}

#[test]
fn twinkles_are_brief_bounded_and_rare() {
    let l = garland_layout(&GarlandSpec::default(), &geometry(2400.0, 1400.0, 2.0, &[]));
    let mut lit = 0usize;
    let mut total = 0usize;
    for step in 0..200 {
        let t = step as f32 * 0.1;
        for light in &l.lights {
            let v = twinkle(light, t, 0.4);
            assert!((0.0..=1.0).contains(&v));
            lit += usize::from(v > 0.0);
            total += 1;
        }
    }
    // A few lights at a time, never all of them.
    let share = lit as f32 / total as f32;
    assert!(share > 0.03 && share < 0.3, "{share}");
    assert!(l.lights.iter().all(|light| twinkle(light, 3.0, 0.0) == 0.0), "steady when 0");
}

#[test]
fn hidden_means_wholly_under_a_card() {
    let card = [Rect::new(100.0, 100.0, 500.0, 500.0)];
    assert!(hidden(&Rect::new(200.0, 200.0, 4.0, 4.0), &card));
    assert!(!hidden(&Rect::new(98.0, 200.0, 4.0, 4.0), &card), "showing at the edge");
    assert!(!hidden(&Rect::new(20.0, 20.0, 4.0, 4.0), &card));
}

fn moving() -> Moving {
    Moving::new(true, true, true, true)
}

#[test]
fn the_clock_runs_at_a_low_rate_only_while_in_use() {
    let start = Instant::now();
    let mut clock = DecorationClock::new(start);
    let m = moving();
    let next = clock.next_frame(start, m).unwrap();
    assert!(next - start >= Duration::from_secs(1) / FPS);
    const { assert!(FPS <= 20) };
    // The user's rate: 30 a second at most, whatever is asked.
    let fast = clock.next_frame(start, Moving { fps: 60, ..m }).unwrap();
    assert_eq!(fast - start, Duration::from_secs(1) / MAX_FPS);
    let slow = clock.next_frame(start, Moving { fps: 10, ..m }).unwrap();
    assert_eq!(slow - start, Duration::from_millis(100));
    // Hidden, unfocused, nothing moving, static: no frame asked.
    for off in [
        Moving { visible: false, ..m },
        Moving { focused: false, ..m },
        Moving { moves: false, ..m },
        Moving { allowed: false, ..m },
    ] {
        assert_eq!(clock.next_frame(start, off), None, "{off:?}");
    }
    // Time passes while it runs.
    clock.advance(start, m);
    let t = clock.advance(start + Duration::from_millis(200), m);
    assert!((t - 0.2).abs() < 1e-3);
    // A long pause doesn't make it jump.
    let t2 = clock.advance(start + Duration::from_secs(5), m);
    assert!(t2 - t <= 0.25 + 1e-3);
}

#[test]
fn idle_stops_it_frozen_and_input_resumes_it() {
    let start = Instant::now();
    let mut clock = DecorationClock::new(start);
    let m = moving();
    clock.advance(start, m);
    let idle = start + IDLE_STOP + Duration::from_millis(10);
    // Never idle when asked so.
    assert!(clock.running(idle, Moving { idle: None, ..moving() }));
    assert_eq!(clock.next_frame(idle, m), None, "idle: no more frames");
    let frozen = clock.advance(idle, m);
    assert_eq!(clock.advance(idle + Duration::from_secs(60), m), frozen, "a still frame");
    // Input: it moves on from where it stopped, without a jump.
    let back = idle + Duration::from_secs(60);
    clock.input(back);
    assert!(clock.next_frame(back, m).is_some());
    assert_eq!(clock.advance(back, m), frozen);
    let later = clock.advance(back + Duration::from_millis(100), m);
    assert!((later - frozen - 0.1).abs() < 1e-3);
}

#[test]
fn static_shows_the_same_moment_every_time() {
    let start = Instant::now();
    let mut clock = DecorationClock::new(start);
    let still = Moving { allowed: false, ..moving() };
    assert_eq!(clock.advance(start, still), STATIC_TIME);
    assert_eq!(clock.advance(start + Duration::from_secs(3), still), STATIC_TIME);
    assert_eq!(clock.next_frame(start, still), None);
}

/// The cards over effects: as translucent as asked when the text keeps its contrast, more
/// opaque when it wouldn't (the themes' rule: 4.5:1 for the text, 2.8:1 muted — or what it
/// had, if less); never more than opaque.
#[test]
fn a_card_over_effects_keeps_its_text_readable() {
    use crate::severity::contrast;
    let (base, card) = (Rgba::rgb(0x151618), Rgba::rgb(0x1e1f22));
    let texts = [(Rgba::rgb(0xdfe1e5), TEXT_CONTRAST), (Rgba::rgb(0x8a8e95), MUTED_CONTRAST)];
    let faint = [(Rgba::rgb(0x3cff7a), 0.3)];
    assert_eq!(glass_alpha(card, &texts, base, &faint, 0.91), 0.91, "readable as asked");
    // An effect the color of the muted text itself, drawn fully, under a card asked to be
    // far more translucent than any effect asks (30 %): it's made more opaque.
    let alike = [(Rgba::rgb(0x8a8e95), 1.0)];
    let alpha = glass_alpha(card, &texts, base, &alike, 0.3);
    assert!(alpha > 0.3 && alpha <= 1.0, "{alpha}");
    for (t, least) in texts {
        let seen = base.mix(Rgba::rgb(0x8a8e95), 1.0).mix(card, alpha).with_alpha(1.0);
        assert!(contrast(t, seen) >= contrast(t, card).min(least) - 0.01 || alpha == 1.0);
    }
    assert_eq!(glass_alpha(card, &texts, base, &[], 1.0), 1.0);
}

/// Each pause offered: moving until it's up without input (then still, asking nothing);
/// "never" moves on for good — but not hidden.
#[test]
#[allow(clippy::disallowed_methods)]
fn each_idle_pause_offered_holds_until_it_is_up() {
    let start = Instant::now();
    for secs in IDLE_CHOICES {
        let mut clock = DecorationClock::new(start);
        let idle = (secs > 0).then(|| Duration::from_secs(secs as u64));
        let m = Moving { idle, ..moving() };
        let before = start + Duration::from_secs((secs.max(1) - 1) as u64);
        assert!(clock.next_frame(before, m).is_some(), "{secs}: moving before");
        let after = start + Duration::from_secs(secs as u64 + 1);
        let hour = start + Duration::from_secs(3600);
        if secs == 0 {
            assert!(clock.next_frame(hour, m).is_some(), "never: still moving an hour on");
            assert_eq!(clock.next_frame(hour, Moving { visible: false, ..m }), None, "hidden");
        } else {
            assert_eq!(clock.next_frame(after, m), None, "{secs}: still after");
            clock.input(after);
            assert!(clock.next_frame(after, m).is_some(), "{secs}: input wakes them");
        }
    }
    assert_eq!(IDLE_CHOICES, [20, 60, 300, 0]);
    assert_eq!(IDLE_STOP, Duration::from_secs(20), "the default as before");
    for secs in IDLE_CHOICES {
        assert!(!idle_label(secs).is_empty());
    }
    assert_ne!(idle_label(300), idle_label(60));
}
