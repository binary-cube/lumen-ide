//! The metric card: everything inside its rectangle at every width and form, a threshold
//! with its sign (not only its color), a long value cut inside, two series with a legend,
//! the focus ring, the pill; nothing allocated once its texts were shaped.

use super::*;
use crate::charts::spark::{Reduce, Samples};
use render_api::Scene;

fn plot() -> Plot {
    let values: Vec<f32> = (0..60).map(|i| (i % 9) as f32).collect();
    Plot::build(Samples::new(&values), 60, 120, Reduce::Max)
}

fn card<'a>(series: &'a [CardSeries<'a>], value: &'a str, warning: bool) -> MetricCard<'a> {
    MetricCard {
        label: "CPU",
        value,
        sub: Some("of 12 cores · max 18% in the last minute"),
        warning,
        series,
        max: 10.0,
        hovered: false,
        active: false,
        focused: false,
    }
}

fn draw(card: &MetricCard<'_>, rect: Rect, form: CardForm) -> (Scene, CardAreas) {
    draw_with(&mut TextSystem::new(), &Icons::builtin(), card, rect, form)
}

/// [`draw`] with a text system and icons made once (many cards drawn).
fn draw_with(
    text: &mut TextSystem,
    icons: &Icons,
    card: &MetricCard<'_>,
    rect: Rect,
    form: CardForm,
) -> (Scene, CardAreas) {
    let palette = crate::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut scene = Scene::default();
    let layer = scene.push_layer(None);
    let areas = metric_card(layer, text, icons, &ds, rect, card, form);
    (scene, areas)
}

fn inside(scene: &Scene, rect: Rect) -> bool {
    let quads = scene.layers().iter().flat_map(|l| &l.quads);
    let glyphs = scene.layers().iter().flat_map(|l| &l.glyphs);
    quads.into_iter().all(|q| {
        q.bounds.min_x() >= rect.min_x() - 0.5
            && q.bounds.max_x() <= rect.max_x() + 0.5
            && q.bounds.min_y() >= rect.min_y() - 0.5
            && q.bounds.max_y() <= rect.max_y() + 0.5
    }) && glyphs.into_iter().all(|g| g.origin.x >= rect.min_x() && g.origin.x < rect.max_x())
}

#[test]
fn everything_stays_inside_at_every_width_and_form() {
    let plot = plot();
    let palette = crate::Palette::dark();
    let series = [CardSeries { plot: &plot, color: palette.accent, label: "CPU" }];
    let card = card(&series, "3.4%", false);
    let (mut text, icons) = (TextSystem::new(), Icons::builtin());
    for (form, height) in [
        (CardForm::Card, 96.0),
        (CardForm::Card, 80.0),
        (CardForm::Card, 68.0),
        (CardForm::Pill, 36.0),
    ] {
        for width in (60..=400).step_by(7) {
            let rect = Rect::new(20.0, 10.0, width as f32, height);
            let (scene, areas) = draw_with(&mut text, &icons, &card, rect, form);
            assert!(inside(&scene, rect), "{form:?} {width}×{height}");
            if areas.chart.size.width > 0.0 {
                assert!(areas.chart.min_x() >= rect.min_x() && areas.chart.max_x() <= rect.max_x());
            }
        }
    }
}

#[test]
fn a_threshold_has_its_sign_not_only_its_color() {
    let plot = plot();
    let palette = crate::Palette::dark();
    let series = [CardSeries { plot: &plot, color: palette.accent, label: "CPU" }];
    // Wide enough that nothing is cut in any font: only the sign differs.
    let rect = Rect::new(0.0, 0.0, 600.0, 96.0);
    let (calm, _) = draw(&card(&series, "3.4%", false), rect, CardForm::Card);
    let (hot, _) = draw(&card(&series, "3.4%", true), rect, CardForm::Card);
    let glyphs = |s: &Scene| s.glyph_count();
    assert_eq!(glyphs(&hot), glyphs(&calm) + 1, "the sign");
    let warned = hot.layers()[0].glyphs.iter().filter(|g| g.color == palette.warning).count();
    assert!(warned > 1, "the line in the warning color");
}

#[test]
fn two_series_have_a_legend_and_a_long_value_is_cut() {
    let (a, b) = (plot(), plot());
    let palette = crate::Palette::dark();
    let series = [
        CardSeries { plot: &a, color: palette.accent, label: "Network" },
        CardSeries { plot: &b, color: palette.warning, label: "Disk" },
    ];
    let rect = Rect::new(0.0, 0.0, 120.0, 96.0);
    let long = "↓↑ 1 234 567 890 KB/s and more";
    let (scene, areas) = draw(&card(&series, long, false), rect, CardForm::Card);
    assert!(inside(&scene, rect));
    assert!(areas.value.max_x() <= rect.max_x());
    let dots = scene.layers()[0].quads.iter().filter(|q| q.bounds.size.width == 6.0).count();
    assert_eq!(dots, 2, "a dot a series in the legend");
}

#[test]
fn the_focus_ring_and_the_active_edge() {
    let plot = plot();
    let palette = crate::Palette::dark();
    let series = [CardSeries { plot: &plot, color: palette.accent, label: "CPU" }];
    let rect = Rect::new(10.0, 10.0, 200.0, 80.0);
    let mut c = card(&series, "1%", false);
    c.focused = true;
    c.active = true;
    let (scene, _) = draw(&c, rect, CardForm::Card);
    let quads = &scene.layers()[0].quads;
    assert!(
        quads.iter().any(|q| q.bounds.min_x() < rect.min_x() && q.border_width > 1.0),
        "the ring outside"
    );
    assert!(quads.iter().any(|q| q.bounds == rect && q.border_color.r == palette.accent.r));
}
