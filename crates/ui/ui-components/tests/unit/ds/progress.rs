use super::*;
use crate::palette::Palette;

fn ctx(p: &Palette) -> Ctx<'_> {
    Ctx::new(p, 1.0)
}

/// Built by its builder: known or not, clamped, counted, its value as it reads.
#[test]
fn it_is_configured_by_its_builder() {
    assert_eq!(ProgressBar::of(1.7).fraction, Some(1.0), "clamped");
    assert_eq!(ProgressBar::of(-1.0).fraction, Some(0.0));
    assert!(ProgressBar::unknown().animating() && !ProgressBar::of(0.5).animating());
    assert_eq!(ProgressBar::counted(25, 100).fraction, Some(0.25));
    assert_eq!(ProgressBar::counted(25, 0).fraction, None, "no total: not known");
    assert_eq!(ProgressBar::of(0.404).percent().value_text().as_deref(), Some("40%"));
    assert_eq!(ProgressBar::unknown().percent().value_text(), None, "no percent unknown");
    assert_eq!(
        ProgressBar::of(0.1).value("1 MB of 9 MB").value_text().as_deref(),
        Some("1 MB of 9 MB")
    );
    let bar = ProgressBar::of(0.5).tone(Tone::Success).thickness(2.0).label("Copying");
    assert_eq!((bar.tone, bar.thickness, bar.label), (Tone::Success, 2.0, Some("Copying")));
}

/// Its height: the bar alone, or the text row over it.
#[test]
fn its_height_has_the_row_only_with_a_text() {
    let p = Palette::dark();
    let c = ctx(&p);
    assert_eq!(ProgressBar::of(0.5).height(&c), 4.0);
    assert_eq!(ProgressBar::of(0.5).thickness(2.0).height(&c), 2.0);
    assert_eq!(ProgressBar::of(0.5).percent().height(&c), 28.0);
}

/// Drawn: the track, the fill as far as it went in the tone's color; the row's texts.
#[test]
fn it_is_drawn_as_far_as_it_went() {
    let p = Palette::dark();
    let c = ctx(&p);
    let mut scene = render_api::Scene::default();
    let track = Rect::new(10.0, 0.0, 200.0, 4.0);
    ProgressBar::of(0.25).tone(Tone::Warning).paint_bar(scene.push_layer(None), &c, track);
    let quads = &scene.layers()[0].quads;
    assert_eq!(quads.len(), 2, "the track and the fill");
    assert_eq!(quads[1].bounds.size.width, 50.0);
    assert_eq!(quads[1].color, p.warning);
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let rect = Rect::new(0.0, 0.0, 300.0, 28.0);
    ProgressBar::of(0.5).label("Downloading").percent().paint(
        scene.push_layer(None),
        &mut text,
        &c,
        rect,
    );
    assert!(scene.glyph_count() > "Downloading50%".len() - 2, "the label and the percent");
    let bar = scene.layers()[0].quads[0].bounds;
    assert_eq!((bar.min_y(), bar.size.height), (24.0, 4.0), "the bar at the bottom");
}

/// Not known: a piece sweeps across, always inside the track.
#[test]
fn not_known_a_piece_sweeps_inside_the_track() {
    let track = Rect::new(10.0, 0.0, 100.0, 4.0);
    for ms in (0..SWEEP_MS).step_by(50) {
        let (x, w) = sweep(track, ms);
        assert!(x >= track.min_x() && x + w <= track.max_x() + 0.001, "at {ms} ms: {x} {w}");
    }
    assert!(sweep(track, SWEEP_MS / 2).1 > 0.0, "seen midway");
}

fn drawn(bar: ProgressBar<'_>, track: Rect, scale: f32) -> Vec<render_api::Quad> {
    let p = Palette::dark();
    let c = Ctx::new(&p, scale);
    let mut layer = Layer::default();
    bar.paint_bar(&mut layer, &c, track);
    layer.quads
}

/// Its new options by their builder; a position given, it needs no frames.
#[test]
fn its_looks_are_configured_by_its_builder() {
    let bar = ProgressBar::of(0.5)
        .color(Rgba::WHITE)
        .fill_alpha(0.5)
        .track_alpha(0.07)
        .min_fill(2.0)
        .radius(1.0)
        .snapped();
    assert_eq!(bar.fill, Some(Rgba::WHITE));
    assert_eq!((bar.fill_alpha, bar.track), (0.5, ProgressTrack::Faint(0.07)));
    assert_eq!((bar.min_fill, bar.radius, bar.snapped), (Some(2.0), Some(1.0), true));
    assert_eq!(ProgressBar::of(0.5).fill_only().track, ProgressTrack::None);
    let dark = ProgressBar::unknown().on_dark().track;
    assert_eq!(dark, ProgressTrack::Color(Rgba::WHITE.with_alpha(DARK_TRACK_ALPHA)));
    assert!(!ProgressBar::unknown().sweep_at(0.3).animating(), "the caller's clock");
    let glide = ProgressBar::unknown().glide(0.22).sweep;
    assert_eq!((glide.share, glide.motion), (0.22, SweepMotion::Glide));
}

/// The defaults are the usual bar: a faint track, a pill, the fill at least round.
#[test]
fn its_defaults_are_the_usual_bar() {
    let p = Palette::dark();
    let track = Rect::new(0.0, 0.0, 200.0, 4.0);
    let quads = drawn(ProgressBar::of(0.001), track, 1.0);
    assert_eq!(quads[0].color, p.text.with_alpha(TRACK_ALPHA));
    assert_eq!(quads[0].corner_radius, 2.0, "half the height");
    assert_eq!(quads[1].bounds.size.width, 4.0, "at least as wide as high");
    assert_eq!(quads[1].color, p.accent);
}

/// Its own fill, track, least width, corners and whole pixels.
#[test]
fn a_bar_takes_its_colors_least_width_and_corners() {
    let track = Rect::new(0.0, 0.0, 100.0, 6.0);
    let bar = ProgressBar::of(0.333)
        .color(Rgba::WHITE)
        .track(ProgressTrack::Color(Rgba::BLACK))
        .radius(1.5)
        .snapped();
    let quads = drawn(bar, track, 2.0);
    assert_eq!(quads[0].color, Rgba::BLACK);
    assert_eq!((quads[1].bounds.size.width, quads[1].color), (33.0, Rgba::WHITE));
    assert_eq!(quads[1].corner_radius, 3.0, "scaled");
    let tiny = drawn(ProgressBar::of(0.001).min_fill(2.0).snapped(), track, 1.0);
    assert_eq!(tiny[1].bounds.size.width, 2.0);
    assert_eq!(drawn(ProgressBar::of(0.0).min_fill(2.0), track, 1.0).len(), 1, "empty: no fill");
    let alone = drawn(ProgressBar::of(0.5).fill_only().fill_alpha(0.5), track, 1.0);
    assert_eq!(alone.len(), 1, "no track");
    assert_eq!(alone[0].color.a, Palette::dark().accent.a * 0.5);
}

/// Split in parts: one after another, never past the track's end.
#[test]
fn a_split_bar_draws_its_parts_one_after_another() {
    let parts = [ProgressSegment::new(0.25, Rgba::WHITE), ProgressSegment::new(0.9, Rgba::BLACK)];
    let quads = drawn(ProgressBar::split(&parts).snapped(), Rect::new(10.0, 0.0, 100.0, 6.0), 1.0);
    assert_eq!(quads.len(), 3, "the track and two parts");
    assert_eq!((quads[1].bounds.min_x(), quads[1].bounds.size.width), (10.0, 25.0));
    assert_eq!((quads[2].bounds.min_x(), quads[2].bounds.size.width), (35.0, 75.0));
    assert_eq!(quads[2].color, Rgba::BLACK);
}

/// A gliding piece at the caller's position: hidden at the start, whole midway.
#[test]
fn a_gliding_piece_follows_the_callers_clock() {
    let track = Rect::new(0.0, 0.0, 100.0, 3.0);
    let at =
        |t: f32| drawn(ProgressBar::unknown().min_fill(0.0).glide(0.22).sweep_at(t), track, 1.0);
    assert_eq!(at(0.0).len(), 1, "not in yet");
    let mid = at(0.5)[1].bounds;
    assert_eq!(mid.size.width, 22.0);
    assert_eq!(mid.min_x(), 39.0, "centered midway");
}

/// A meter: its width, each bar's track and fill standing on the line, the least shown.
#[test]
fn a_meter_stands_its_bars_on_a_line() {
    let p = Palette::dark();
    let c = Ctx::new(&p, 1.0);
    let meter = Meter::default();
    assert_eq!(meter.width(&c, 3), 3.0 * 5.0 + 2.0 * 3.0);
    let mut layer = Layer::default();
    meter.paint(&mut layer, &c, (10.0, 20.0), [(0.0, Rgba::WHITE), (1.0, Rgba::BLACK)]);
    let q = &layer.quads;
    assert_eq!(q.len(), 4, "a track and a fill each");
    assert_eq!(q[0].color, Rgba::WHITE.with_alpha(0.16));
    assert_eq!((q[1].bounds.max_y(), q[1].bounds.size.height), (20.0, 2.0), "empty: the least");
    assert_eq!(q[3].bounds, Rect::new(18.0, 7.0, 5.0, 13.0), "full");
}
