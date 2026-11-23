use super::*;
use charts_model::{Aggregate, ChartKind, ChartSpec, Column, ColumnKind, Rows, SeriesSpec};
use render_api::Layer;

use crate::palette::Palette;

fn data(kind: ChartKind, x_kind: ColumnKind) -> Arc<Prepared> {
    let mut rows = Rows::new(vec![
        Column::new("k", x_kind),
        Column::new("a", ColumnKind::Number),
        Column::new("b", ColumnKind::Number),
    ]);
    for (k, a, b) in [("1", 10, 5), ("2", 20, 15), ("3", 40, 25), ("4", 30, 20)] {
        rows.push(
            [Some(k), Some(&a.to_string()), Some(&b.to_string())].map(|c| c.map(str::to_string)),
        );
    }
    let spec = ChartSpec {
        kind,
        series: vec![SeriesSpec {
            x: Some("k".into()),
            y: vec!["a".into(), "b".into()],
            aggregate: Aggregate::Sum,
            ..SeriesSpec::default()
        }],
        ..ChartSpec::default()
    };
    Arc::new(charts_model::prepare(&spec, &rows).expect("prepared"))
}

fn painted(kind: ChartKind, x_kind: ColumnKind) -> (Chart, TextSystem) {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    chart.set_data(Some(data(kind, x_kind)), 1);
    let look = ChartLook::new(&ctx, palette.editor_background);
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, Rect::new(0.0, 0.0, 640.0, 360.0));
    (chart, text)
}

/// A bar under the mouse is lit and told: its category, its series and its value.
#[test]
fn a_bar_under_the_mouse_is_told() {
    let (mut chart, _) = painted(ChartKind::Bar, ColumnKind::Text);
    let bar = chart
        .geometry()
        .marks
        .iter()
        .find(|m| m.series == 0 && m.index == 2)
        .map(|m| m.shape)
        .expect("the bar");
    let MarkShape::Bar(r) = bar else { panic!("{bar:?}") };
    assert!(chart.hover(Some(r.center())), "lit");
    let tip = chart.tooltip(r.center()).expect("a tooltip");
    assert_eq!(tip.label, "3\na: 40");
}

/// A click on the legend hides its series: laid out again without it.
#[test]
fn the_legend_hides_a_series() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let (mut chart, mut text) = painted(ChartKind::Bar, ColumnKind::Text);
    let entry = chart.geometry().legend[1].area.center();
    assert_eq!(chart.press(entry, 1), Press::Took);
    assert_eq!(chart.cursor(entry), Some(ViewerCursor::Pointer));
    let look = ChartLook::new(&ctx, palette.editor_background);
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, Rect::new(0.0, 0.0, 640.0, 360.0));
    assert!(chart.geometry().marks.iter().all(|m| m.series == 0));
    assert!(chart.geometry().legend[1].hidden);
}

/// Over a line along a number: the wheel zooms around the mouse, a drag moves, a double
/// click shows all; the column under the mouse tells every series there.
#[test]
fn a_line_zooms_moves_and_tells_its_column() {
    let (mut chart, _) = painted(ChartKind::Line, ColumnKind::Number);
    let plot = chart.geometry().plot;
    let at = Point::new(plot.center().x, plot.center().y);
    chart.hover(Some(at));
    assert!(chart.scroll(0.0, -1.0), "zoomed in");
    let (lo, hi) = chart.zoom.expect("a zoom");
    assert!(hi - lo < 3.0);
    assert_eq!(chart.press(at, 1), Press::Held);
    assert!(chart.drag(Point::new(at.x - 50.0, at.y)));
    chart.release();
    let moved = chart.zoom.expect("still zoomed");
    assert!(moved.0 > lo, "moved right along X");
    assert_eq!(chart.press(at, 2), Press::Took);
    assert!(chart.zoom.is_none(), "all of X again");
    let slot = chart.geometry().slots[1];
    let tip = chart.tooltip(Point::new(slot, plot.center().y)).expect("a tooltip");
    assert_eq!(tip.label, "2\na: 20\nb: 15");
}

/// A line's masks are made on a worker; drawn once ready, made again only for a new layout.
#[test]
fn a_line_is_rasterized_once_per_layout() {
    let (mut chart, _) = painted(ChartKind::Area, ColumnKind::Text);
    let started = std::time::Instant::now();
    while !chart.poll() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "rasterized");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let pieces = chart.raster.pieces().expect("masks");
    assert!(!pieces.is_empty());
    assert!(!chart.busy());
}

/// To a screen reader: an image named by its title, its summary as its value.
#[test]
fn a_chart_tells_what_it_shows() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    chart.set_title("Sales");
    chart.set_data(Some(data(ChartKind::Bar, ColumnKind::Text)), 1);
    let look = ChartLook::new(&ctx, palette.editor_background);
    let recording = ui_kit::access::begin();
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, Rect::new(0.0, 0.0, 640.0, 360.0));
    let frame = recording.take();
    let image =
        frame.nodes().iter().find(|n| n.role == ui_kit::access::Role::Image).expect("the chart");
    assert_eq!(&*image.name, "Sales");
    assert_eq!(image.value.as_deref(), Some("bar, 4 × 2, max 40"));
}

/// A series over time with its values in their unit and its axis fixed: the tooltip says
/// the moment's label and each value as formatted; the axis ends where it was fixed.
#[test]
fn a_series_over_time_is_told_in_its_unit() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    let data = Prepared::over_time(
        ChartKind::Area,
        vec!["20 s ago".into(), "10 s ago".into(), "now".into()],
        vec![0.0, 10.0, 20.0],
        vec![("CPU".into(), vec![10.0, 40.0, 25.0])],
    );
    chart.set_data(Some(Arc::new(data)), 1);
    chart.set_y_range(Some((0.0, 100.0)));
    chart.set_format(Some(ValueFormat(Arc::new(|_, v| format!("{v:.1}%")))));
    let look = ChartLook::new(&ctx, palette.editor_background);
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, Rect::new(0.0, 0.0, 400.0, 160.0));
    let ticks = &chart.geometry().y_ticks;
    assert_eq!(ticks.last().map(|t| t.value), Some(100.0));
    let slot = chart.geometry().slots[1];
    let tip = chart.tooltip(Point::new(slot, chart.geometry().plot.center().y)).expect("a tooltip");
    assert_eq!(tip.label, "10 s ago\nCPU: 40.0%");
}

/// The legend says its series and what a press does; a label cut short is said whole.
#[test]
fn the_legend_and_cut_labels_have_tooltips() {
    let (chart, _) = painted(ChartKind::Bar, ColumnKind::Text);
    let entry = chart.geometry().legend[0].area.center();
    let tip = chart.tooltip(entry).expect("the legend's");
    assert!(tip.label.starts_with("a\n"), "{}", tip.label);
}

/// A ring (a pie with a hole): its slices rasterized on a worker and drawn; its hole is
/// nothing's, its band a slice's; a slice lit by its owner (a row beside it) moves out.
#[test]
fn a_ring_is_drawn_and_lit_from_outside() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    let data = Prepared::slices(vec![("a".into(), 3.0), ("b".into(), 1.0)], None);
    chart.set_data(Some(Arc::new(data)), 1);
    chart.set_options(charts_model::Options { hole: 58, ..Default::default() });
    // Still (no appearing): the picture as it settles.
    let look = ChartLook::new(&ctx, palette.editor_background)
        .parts(ChartParts::bare())
        .motion(crate::charts::ChartMotion::none());
    let area = Rect::new(0.0, 0.0, 200.0, 200.0);
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, area);
    let started = std::time::Instant::now();
    while !chart.poll() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "rasterized");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut layer = Layer::default();
    chart.paint(&mut layer, &mut text, &ctx, &look, area);
    assert!(!layer.images.is_empty(), "its slices drawn");
    assert_eq!(chart.slice_at(area.center()), None, "the hole");
    assert_eq!(chart.slice_at(Point::new(130.0, 30.0)), Some(0), "on the band, right of the top");
    assert!(chart.set_lit(Some(1)));
    assert!(!chart.set_lit(Some(1)), "the same: nothing new");
    // Its tooltip in the owner's unit, with its share.
    chart.set_format(Some(ValueFormat(Arc::new(|_, v| format!("{v} B")))));
    let on_band = Point::new(130.0, 30.0);
    crate::control::Control::hover(&mut chart, Some(on_band));
    let tip = crate::control::Control::tooltip(&chart, on_band).expect("its tooltip");
    assert!(tip.label.starts_with("a\n3 B · 75"), "{}", tip.label);
}

/// A single slice (the whole ring) lit stays where it is: moved out, the ring would only
/// look off its place.
#[test]
fn a_whole_ring_lit_stays_in_place() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    chart.set_data(Some(Arc::new(Prepared::slices(vec![("logs".into(), 17.0)], None))), 1);
    chart.set_options(charts_model::Options { hole: 58, ..Default::default() });
    let look = ChartLook::new(&ctx, palette.editor_background)
        .parts(ChartParts::bare())
        .motion(crate::charts::ChartMotion::none());
    let area = Rect::new(0.0, 0.0, 200.0, 200.0);
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, area);
    let started = std::time::Instant::now();
    while !chart.poll() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "rasterized");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let places = |chart: &mut Chart, text: &mut TextSystem| {
        let mut layer = Layer::default();
        chart.paint(&mut layer, text, &ctx, &look, area);
        layer.images.iter().map(|i| (i.bounds.min_x(), i.bounds.min_y())).collect::<Vec<_>>()
    };
    let rest = places(&mut chart, &mut text);
    assert!(chart.set_lit(Some(0)));
    assert_eq!(places(&mut chart, &mut text), rest, "lit, in the same place");
}

/// The values' labels as the owner writes them (`50.0%`) have their room: none drawn left
/// of the chart's area.
#[test]
fn written_labels_stay_inside_the_area() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    chart.set_data(Some(data(ChartKind::Area, ColumnKind::Text)), 1);
    chart.set_format(Some(ValueFormat(Arc::new(|_, v| format!("{v:.1}%")))));
    chart.set_y_range(Some((0.0, 50.0)));
    let look = ChartLook::new(&ctx, palette.editor_background);
    let area = Rect::new(100.0, 0.0, 600.0, 300.0);
    let mut layer = Layer::default();
    chart.paint(&mut layer, &mut text, &ctx, &look, area);
    let leftmost = layer.glyphs.iter().map(|g| g.origin.x).fold(f32::MAX, f32::min);
    assert!(leftmost >= area.min_x() - 0.5, "{leftmost} left of {}", area.min_x());
}

/// A chart not drawn (not yet, or forgotten: a detail's chart for a figure the part doesn't
/// have) moves for nobody: new data starts no transition, it says it's still — frames
/// asked for it would spin the processor for nothing.
#[test]
fn a_chart_not_drawn_never_animates() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut chart = Chart::new();
    for generation in 1..4 {
        chart.set_data(Some(data(ChartKind::Bar, ColumnKind::Text)), generation);
        assert!(!chart.animating(), "never drawn: still");
    }
    let look = ChartLook::new(&ctx, palette.editor_background);
    let area = Rect::new(0.0, 0.0, 400.0, 200.0);
    chart.paint(&mut Layer::default(), &mut text, &ctx, &look, area);
    chart.set_data(Some(data(ChartKind::Bar, ColumnKind::Text)), 10);
    assert!(chart.animating(), "drawn: its new data moves in");
    chart.forget();
    assert!(!chart.animating(), "forgotten: still");
    chart.set_data(Some(data(ChartKind::Bar, ColumnKind::Text)), 11);
    assert!(!chart.animating(), "and its new data starts nothing");
}
