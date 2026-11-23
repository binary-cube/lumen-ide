use super::*;
use crate::data::{Column, ColumnKind, Rows};
use crate::prepare::prepare;
use crate::spec::{Aggregate, ChartSpec, SeriesSpec};

fn rows() -> Rows {
    let mut rows = Rows::new(vec![
        Column::new("k", ColumnKind::Text),
        Column::new("a", ColumnKind::Number),
        Column::new("b", ColumnKind::Number),
    ]);
    for (k, a, b) in [("x", 10, 5), ("y", 20, 15), ("z", 40, -5)] {
        rows.push([Some(k.to_string()), Some(a.to_string()), Some(b.to_string())]);
    }
    rows
}

fn laid(kind: ChartKind, options: Options, hidden: &[bool]) -> Geometry {
    let spec = ChartSpec {
        kind,
        series: vec![SeriesSpec {
            x: Some("k".into()),
            y: vec!["a".into(), "b".into()],
            ..SeriesSpec::default()
        }],
        options,
    };
    let p = prepare(&spec, &rows()).expect("prepared");
    let view = View {
        area: Rect::new(0.0, 0.0, 600.0, 400.0),
        options,
        metrics: Metrics::at(1.0),
        hidden,
        zoom: None,
        axes: true,
        legend: true,
        y_range: None,
        lanes: false,
        y_label: None,
    };
    lay_out(&p, &view, &mut |s: &str| s.len() as f32 * 7.0)
}

/// Bars stand on zero side by side, the taller value higher; a negative one hangs below.
#[test]
fn bars_stand_on_zero_side_by_side() {
    let g = laid(ChartKind::Bar, Options::default(), &[]);
    assert_eq!(g.marks.len(), 6);
    let zero = g.zero.expect("zero in view");
    let bar = |s: usize, i: usize| match g
        .marks
        .iter()
        .find(|m| m.series == s && m.index == i)
        .map(|m| m.shape)
    {
        Some(MarkShape::Bar(r)) => r,
        other => panic!("{other:?}"),
    };
    assert!((bar(0, 0).max_y() - zero).abs() < 0.5);
    assert!(bar(0, 2).size.height > bar(0, 1).size.height);
    assert!(bar(1, 2).min_y() >= zero - 0.5, "below zero");
    assert!(bar(1, 0).min_x() >= bar(0, 0).max_x() - 0.5, "side by side");
    assert_eq!(g.legend.len(), 2, "two series: a legend");
    assert!(g.plot.min_y() >= g.legend[0].area.max_y(), "under the legend");
}

/// Stacked bars pile up; a series hidden from the legend isn't drawn.
#[test]
fn stacked_bars_pile_and_hidden_series_go() {
    let g = laid(ChartKind::StackedBar, Options::default(), &[]);
    let of = |s: usize| g.marks.iter().find(|m| m.series == s && m.index == 0).map(|m| m.shape);
    let (Some(MarkShape::Bar(a)), Some(MarkShape::Bar(b))) = (of(0), of(1)) else { panic!() };
    assert!((b.max_y() - a.min_y()).abs() < 0.5, "the second on the first");
    let g = laid(ChartKind::Bar, Options::default(), &[false, true]);
    assert!(g.marks.iter().all(|m| m.series == 0));
    assert!(g.legend[1].hidden);
}

/// What's under the mouse: a bar, a category's column for a line, a legend entry.
#[test]
fn the_mouse_finds_a_bar_a_column_or_the_legend() {
    let g = laid(ChartKind::Bar, Options::default(), &[]);
    let Some(MarkShape::Bar(r)) = g.marks.first().map(|m| m.shape) else { panic!() };
    assert_eq!(g.hit(r.center()), Some(Hit::Mark { series: 0, index: 0 }));
    assert_eq!(g.hit(g.legend[1].area.center()), Some(Hit::Legend(1)));
    let g = laid(ChartKind::Line, Options::default(), &[]);
    assert_eq!(g.lines.len(), 2);
    let near_y = Point::new(g.slots[1] + 3.0, g.plot.center().y);
    assert_eq!(g.hit(near_y), Some(Hit::Slot(1)));
}

/// A pie's slices go round clockwise from the top, the whole circle once.
#[test]
fn a_pie_goes_round_once() {
    let spec = ChartSpec {
        kind: ChartKind::Pie,
        series: vec![SeriesSpec {
            x: Some("k".into()),
            y: vec!["a".into()],
            aggregate: Aggregate::Sum,
            ..SeriesSpec::default()
        }],
        options: Options::default(),
    };
    let p = prepare(&spec, &rows()).expect("p");
    let view = View {
        area: Rect::new(0.0, 0.0, 400.0, 400.0),
        options: Options::default(),
        metrics: Metrics::at(1.0),
        hidden: &[],
        zoom: None,
        axes: true,
        legend: true,
        y_range: None,
        lanes: false,
        y_label: None,
    };
    let g = lay_out(&p, &view, &mut |s: &str| s.len() as f32 * 7.0);
    let MarkShape::Slice { end, center, radius, .. } = g.marks[g.marks.len() - 1].shape else {
        panic!()
    };
    assert!((end - std::f32::consts::TAU).abs() < 1e-3);
    // Right of the top: the largest slice (z, 40 of 70) starts there.
    let p = Point::new(center.x + radius * 0.5, center.y - radius * 0.5);
    assert_eq!(g.hit(p), Some(Hit::Mark { series: 0, index: 0 }));
    // A ring: its hole is nothing's, its band the slice's.
    let ring = View { options: Options { hole: 60, ..Options::default() }, ..view };
    let g = lay_out(&p_of(&spec), &ring, &mut |s: &str| s.len() as f32 * 7.0);
    let MarkShape::Slice { inner, radius, center, .. } = g.marks[0].shape else { panic!() };
    assert!((inner - radius * 0.6).abs() < 1e-3);
    assert_eq!(g.hit(center), None, "the hole");
    let on_band = Point::new(center.x + radius * 0.56, center.y - radius * 0.56);
    assert_eq!(g.hit(on_band), Some(Hit::Mark { series: 0, index: 0 }));
}

fn p_of(spec: &ChartSpec) -> crate::prepare::Prepared {
    prepare(spec, &rows()).expect("p")
}

/// A long line keeps at most two points a pixel column, its highest and lowest.
#[test]
fn a_long_line_is_thinned_keeping_its_spikes() {
    let run: Vec<Point> = (0..100_000)
        .map(|i| Point::new(i as f32 / 1000.0, if i == 5_000 { -50.0 } else { 0.0 }))
        .collect();
    let thinned = thin(run, 100.0);
    assert!(thinned.len() <= 400);
    assert!(thinned.iter().any(|p| p.y == -50.0), "the spike stays");
}

/// A run in steps holds each value until the next's place.
#[test]
fn a_run_in_steps_holds_each_value() {
    let run = [Point::new(0.0, 10.0), Point::new(5.0, 20.0)];
    assert_eq!(
        stepped(&run, true),
        [Point::new(0.0, 10.0), Point::new(5.0, 10.0), Point::new(5.0, 20.0)]
    );
    assert_eq!(stepped(&run, false), run);
}

/// In lanes each series has a band of its own, one under the other, on its own scale: the
/// highest value of each reaches as high in its band; a column under the mouse is every
/// series' moment.
#[test]
fn lanes_put_each_series_in_its_band() {
    let p = crate::prepare::Prepared::over_time(
        ChartKind::Area,
        vec!["0".into(), "1".into(), "2".into()],
        vec![0.0, 1.0, 2.0],
        vec![
            ("cpu".into(), vec![10.0, 50.0, 20.0]),
            ("frames".into(), vec![1000.0, 3000.0, 2000.0]),
        ],
    );
    let view = View {
        area: Rect::new(0.0, 0.0, 600.0, 300.0),
        options: Options::default(),
        metrics: Metrics::at(1.0),
        hidden: &[],
        zoom: None,
        axes: true,
        legend: false,
        y_range: None,
        lanes: true,
        y_label: None,
    };
    let g = lay_out(&p, &view, &mut |s: &str| s.len() as f32 * 7.0);
    assert_eq!(g.bands.len(), 2);
    let (a, b) = (g.bands[0].1, g.bands[1].1);
    assert!(b.min_y() > a.max_y(), "one under the other");
    let top = |i: usize| g.levels[i].ys.iter().flatten().copied().fold(f32::INFINITY, f32::min);
    let (ta, tb) = (top(0) - a.min_y(), top(1) - b.min_y());
    assert!(ta >= 0.0 && tb >= 0.0, "each inside its band");
    assert!(g.y_ticks.is_empty());
    assert_eq!(g.hit(Point::new(g.slots[1], b.center().y)), Some(Hit::Slot(1)));
}
