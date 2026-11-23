use super::*;
use crate::data::{Column, ColumnKind, Rows};
use crate::spec::{Options, SeriesSpec};

fn sales() -> Rows {
    let mut rows = Rows::new(vec![
        Column::new("country", ColumnKind::Text),
        Column::new("year", ColumnKind::Number),
        Column::new("amount", ColumnKind::Number),
    ]);
    for (c, y, a) in
        [("RO", 2024, 10), ("DE", 2024, 30), ("RO", 2025, 15), ("RO", 2025, 5), ("DE", 2025, 40)]
    {
        rows.push([Some(c.to_string()), Some(y.to_string()), Some(a.to_string())]);
    }
    rows
}

fn spec(
    kind: ChartKind,
    x: &str,
    group: Option<&str>,
    y: &[&str],
    aggregate: Aggregate,
) -> ChartSpec {
    ChartSpec {
        kind,
        series: vec![SeriesSpec {
            x: Some(x.into()),
            group: group.map(Into::into),
            y: y.iter().map(|s| s.to_string()).collect(),
            aggregate,
        }],
        options: Options::default(),
    }
}

/// Bars: the categories in the order met, values falling on one summed; a group column
/// splits the series in one per value.
#[test]
fn categories_are_summed_and_split_by_group() {
    let p = prepare(&spec(ChartKind::Bar, "country", None, &["amount"], Aggregate::Sum), &sales())
        .expect("prepared");
    let Shape::Categories { labels, series, xs } = &p.shape else { panic!("categories") };
    assert_eq!(labels, &["RO", "DE"]);
    assert!(xs.is_none());
    assert_eq!(series[0].values, [Some(30.0), Some(70.0)]);

    let p = prepare(
        &spec(ChartKind::Bar, "year", Some("country"), &["amount"], Aggregate::Sum),
        &sales(),
    )
    .expect("prepared");
    let Shape::Categories { series, .. } = &p.shape else { panic!("categories") };
    let names: Vec<_> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["RO · amount", "DE · amount"]);
    assert_eq!(series[0].values, [Some(10.0), Some(20.0)]);
}

/// The aggregates: count reads no value, average, min, max.
#[test]
fn values_on_one_place_are_made_one_by_the_aggregate() {
    let of = |how| {
        let p =
            prepare(&spec(ChartKind::Bar, "country", None, &["amount"], how), &sales()).expect("p");
        let Shape::Categories { series, .. } = p.shape else { panic!() };
        series[0].values.clone()
    };
    assert_eq!(of(Aggregate::Count), [Some(3.0), Some(2.0)]);
    assert_eq!(of(Aggregate::Average), [Some(10.0), Some(35.0)]);
    assert_eq!(of(Aggregate::Min), [Some(5.0), Some(30.0)]);
    assert_eq!(of(Aggregate::Max), [Some(15.0), Some(40.0)]);
}

/// A line over a measured X is sorted by it, its places kept.
#[test]
fn a_line_over_a_number_is_sorted_by_it() {
    let mut rows =
        Rows::new(vec![Column::new("t", ColumnKind::Number), Column::new("v", ColumnKind::Number)]);
    for (t, v) in [(3, 30), (1, 10), (2, 20)] {
        rows.push([Some(t), Some(v)]);
    }
    let p = prepare(&spec(ChartKind::Line, "t", None, &["v"], Aggregate::Sum), &rows).expect("p");
    let Shape::Categories { xs, series, .. } = &p.shape else { panic!() };
    assert_eq!(xs.as_deref(), Some(&[1.0, 2.0, 3.0][..]));
    assert_eq!(series[0].values, [Some(10.0), Some(20.0), Some(30.0)]);
}

/// A pie's slices, largest first; past the limit the smaller ones are one.
#[test]
fn a_pie_keeps_its_largest_slices() {
    let mut rows =
        Rows::new(vec![Column::new("k", ColumnKind::Text), Column::new("v", ColumnKind::Number)]);
    for i in 0..20 {
        rows.push([Some(format!("k{i}")), Some((i + 1).to_string())]);
    }
    let p = prepare(&spec(ChartKind::Pie, "k", None, &["v"], Aggregate::Sum), &rows).expect("p");
    let Shape::Slices { slices, rest } = &p.shape else { panic!() };
    assert_eq!(slices.len(), MAX_SLICES - 1);
    assert_eq!(slices[0].name, "k19");
    let (n, sum) = rest.expect("the rest");
    assert_eq!(n, 20 - (MAX_SLICES - 1));
    assert_eq!(sum, (1..=9).sum::<i32>() as f64);
}

/// A histogram counts every value once, in ranges covering them all.
#[test]
fn a_histogram_counts_every_value() {
    let mut rows = Rows::new(vec![Column::new("v", ColumnKind::Number)]);
    for i in 0..1000 {
        rows.push([Some((i % 97) as f64)]);
    }
    let s = ChartSpec {
        kind: ChartKind::Histogram,
        series: vec![SeriesSpec { y: vec!["v".into()], ..SeriesSpec::default() }],
        options: Options::default(),
    };
    let p = prepare(&s, &rows).expect("p");
    let Shape::Bins { bins, .. } = &p.shape else { panic!() };
    assert!(bins.len() > 1 && bins.len() <= BINS.1);
    assert_eq!(bins.iter().map(|b| b.count).sum::<f64>(), 1000.0);
    assert_eq!(bins[0].from, 0.0);
    assert_eq!(bins[bins.len() - 1].to, 96.0);
}

/// What can't be drawn says why: nothing chosen, a column gone, a text as a value.
#[test]
fn a_spec_that_cant_be_drawn_says_why() {
    let rows = sales();
    assert_eq!(prepare(&ChartSpec::default(), &rows), Err(Unready::NotConfigured));
    assert_eq!(
        prepare(&spec(ChartKind::Bar, "gone", None, &["amount"], Aggregate::Sum), &rows),
        Err(Unready::MissingColumn("gone".into()))
    );
    assert_eq!(
        prepare(&spec(ChartKind::Bar, "year", None, &["country"], Aggregate::Sum), &rows),
        Err(Unready::NotMeasurable("country".into()))
    );
}

/// Scatter points by group; a bubble's size from its second value.
#[test]
fn points_are_split_by_group_with_their_size() {
    let mut rows = Rows::new(vec![
        Column::new("g", ColumnKind::Text),
        Column::new("x", ColumnKind::Number),
        Column::new("y", ColumnKind::Number),
        Column::new("s", ColumnKind::Number),
    ]);
    rows.push([Some("a"), Some("1"), Some("2"), Some("9")]);
    rows.push([Some("b"), Some("3"), Some("4"), Some("1")]);
    let p = prepare(&spec(ChartKind::Bubble, "x", Some("g"), &["y", "s"], Aggregate::Sum), &rows)
        .expect("p");
    let Shape::Points(points) = &p.shape else { panic!() };
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].points, [[1.0, 2.0, 9.0]]);
}

/// Two series with their own X: the categories are both's, each series' values on its own.
#[test]
fn series_with_their_own_x_share_the_categories() {
    let mut rows = Rows::new(vec![
        Column::new("a", ColumnKind::Text),
        Column::new("b", ColumnKind::Text),
        Column::new("v", ColumnKind::Number),
    ]);
    rows.push([Some("x"), Some("y"), Some("1")]);
    rows.push([Some("x"), Some("z"), Some("2")]);
    let s = ChartSpec {
        kind: ChartKind::Bar,
        series: vec![
            SeriesSpec { x: Some("a".into()), y: vec!["v".into()], ..SeriesSpec::default() },
            SeriesSpec { x: Some("b".into()), y: vec!["v".into()], ..SeriesSpec::default() },
        ],
        options: Options::default(),
    };
    let p = prepare(&s, &rows).expect("p");
    let Shape::Categories { labels, series, .. } = &p.shape else { panic!() };
    assert_eq!(labels, &["x", "y", "z"]);
    assert_eq!(series[0].values, [Some(3.0), None, None]);
    assert_eq!(series[1].values, [None, Some(1.0), Some(2.0)]);
}

/// A pie of values already counted: its slices in order, the rest as one.
#[test]
fn a_pie_of_counted_values() {
    let p = Prepared::slices(vec![("a".into(), 3.0), ("b".into(), 1.0)], Some((4, 2.0)));
    assert_eq!(p.kind, ChartKind::Pie);
    let Shape::Slices { slices, rest } = &p.shape else { panic!() };
    assert_eq!((slices.len(), slices[0].name.as_str(), *rest), (2, "a", Some((4, 2.0))));
    assert_eq!(p.rows, 6);
}
