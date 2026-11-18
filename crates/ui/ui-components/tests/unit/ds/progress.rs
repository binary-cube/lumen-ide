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
