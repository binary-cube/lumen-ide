//! Rows made into what a chart draws: the categories and a value per series on each (bars,
//! lines, areas), points (a scatter, bubbles), slices (a pie) or ranges counted (a
//! histogram). Read once per version of the rows (on a worker for many), never while
//! drawing: the result holds only what's drawn, never the rows.

use std::collections::HashMap;

use crate::data::{ColumnKind, Tabular};
use crate::spec::{Aggregate, ChartKind, ChartSpec, SeriesSpec};

/// The most categories a chart keeps (more can't be told apart); the rest are counted in
/// [`Prepared::left_out`].
pub const MAX_CATEGORIES: usize = 2_000;
/// The most series a group column splits into (one color each).
pub const MAX_GROUPS: usize = 24;
/// The most slices a pie shows; the smaller rest becomes one slice.
pub const MAX_SLICES: usize = 12;
/// A histogram's ranges: at least, at most.
pub const BINS: (usize, usize) = (1, 100);

/// A series of values, one per category (`None`: nothing fell there).
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub name: String,
    pub values: Vec<Option<f64>>,
}

/// Points of a series: x, y and a size (a bubble's; 1 otherwise).
#[derive(Debug, Clone, PartialEq)]
pub struct Points {
    pub name: String,
    pub points: Vec<[f64; 3]>,
}

/// A slice of a pie.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub name: String,
    pub value: f64,
}

/// A histogram's range and how many values fell in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bin {
    pub from: f64,
    pub to: f64,
    pub count: f64,
}

/// What the chart draws, by its kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Categories along X, a value per series on each.
    Categories {
        labels: Vec<String>,
        /// Their places when X is measured (a time, a number: a line over time), sorted;
        /// `None`: evenly apart, in the order met.
        xs: Option<Vec<f64>>,
        series: Vec<Values>,
    },
    Points(Vec<Points>),
    Slices {
        slices: Vec<Slice>,
        /// The smaller slices made one: how many, and their sum (`None`: none left out).
        rest: Option<(usize, f64)>,
    },
    Bins {
        name: String,
        bins: Vec<Bin>,
    },
}

/// A chart's data, ready to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub kind: ChartKind,
    pub shape: Shape,
    /// X is a moment (its ticks and labels are dates and times).
    pub x_time: bool,
    /// Y is a moment.
    pub y_time: bool,
    /// Categories or groups beyond the limits, left out.
    pub left_out: usize,
    /// Rows read.
    pub rows: usize,
}

impl Prepared {
    /// Series measured over time (a machine's history): `times` (seconds since the epoch)
    /// along X, a label each (how a tooltip names the moment: "12 s ago"), each series its
    /// name and a value per time (fewer: the rest missing).
    pub fn over_time(
        kind: ChartKind,
        labels: Vec<String>,
        times: Vec<f64>,
        series: Vec<(String, Vec<f64>)>,
    ) -> Self {
        let n = times.len();
        let series = series
            .into_iter()
            .map(|(name, values)| Values {
                name,
                values: (0..n).map(|i| values.get(i).copied()).collect(),
            })
            .collect();
        Self {
            kind,
            shape: Shape::Categories { labels, xs: Some(times), series },
            x_time: true,
            y_time: false,
            left_out: 0,
            rows: n,
        }
    }

    /// A pie of values already counted (a folder's entries by size): `slices` named, in
    /// order; the smaller ones summed as `rest` (how many, their sum; `None`: none).
    pub fn slices(slices: Vec<(String, f64)>, rest: Option<(usize, f64)>) -> Self {
        let rows = slices.len() + rest.map_or(0, |(n, _)| n);
        let slices = slices.into_iter().map(|(name, value)| Slice { name, value }).collect();
        Self {
            kind: ChartKind::Pie,
            shape: Shape::Slices { slices, rest },
            x_time: false,
            y_time: false,
            left_out: 0,
            rows,
        }
    }

    /// How many series it draws (a pie, a histogram: one).
    pub fn series_count(&self) -> usize {
        match &self.shape {
            Shape::Categories { series, .. } => series.len(),
            Shape::Points(points) => points.len(),
            Shape::Slices { .. } | Shape::Bins { .. } => 1,
        }
    }

    /// Series `i`'s name.
    pub fn series_name(&self, i: usize) -> &str {
        match &self.shape {
            Shape::Categories { series, .. } => series.get(i).map_or("", |s| s.name.as_str()),
            Shape::Points(points) => points.get(i).map_or("", |s| s.name.as_str()),
            Shape::Bins { name, .. } => name,
            Shape::Slices { .. } => "",
        }
    }

    /// Nothing to draw.
    pub fn is_empty(&self) -> bool {
        match &self.shape {
            Shape::Categories { labels, series, .. } => labels.is_empty() || series.is_empty(),
            Shape::Points(points) => points.iter().all(|s| s.points.is_empty()),
            Shape::Slices { slices, .. } => slices.is_empty(),
            Shape::Bins { bins, .. } => bins.is_empty(),
        }
    }
}

/// Why a spec can't be drawn from these rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unready {
    /// No series says what to draw yet.
    NotConfigured,
    /// A column it names isn't among the rows' (renamed, another query).
    MissingColumn(String),
    /// A column that must be a number isn't.
    NotMeasurable(String),
}

/// Values falling on one place, made one by an [`Aggregate`].
#[derive(Debug, Clone, Copy, Default)]
struct Acc {
    sum: f64,
    count: u32,
    min: f64,
    max: f64,
}

impl Acc {
    fn add(&mut self, v: f64) {
        if self.count == 0 {
            (self.min, self.max) = (v, v);
        } else {
            self.min = self.min.min(v);
            self.max = self.max.max(v);
        }
        self.sum += v;
        self.count = self.count.saturating_add(1);
    }

    fn value(&self, how: Aggregate) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        Some(match how {
            Aggregate::Sum => self.sum,
            Aggregate::Count => f64::from(self.count),
            Aggregate::Average => self.sum / f64::from(self.count),
            Aggregate::Min => self.min,
            Aggregate::Max => self.max,
        })
    }
}

/// The columns a series reads, found among the rows'.
struct Cols {
    x: Option<usize>,
    group: Option<usize>,
    y: Vec<usize>,
}

fn find(data: &dyn Tabular, s: &SeriesSpec) -> Result<Cols, Unready> {
    let col = |name: &String| data.column(name).ok_or_else(|| Unready::MissingColumn(name.clone()));
    let x = s.x.as_ref().map(col).transpose()?;
    let group = s.group.as_ref().map(col).transpose()?;
    let y = s.y.iter().map(col).collect::<Result<Vec<_>, _>>()?;
    for (&c, name) in y.iter().zip(&s.y) {
        if s.aggregate != Aggregate::Count && !data.columns()[c].kind.measurable() {
            return Err(Unready::NotMeasurable(name.clone()));
        }
    }
    Ok(Cols { x, group, y })
}

/// `spec` made from `data`.
pub fn prepare(spec: &ChartSpec, data: &dyn Tabular) -> Result<Prepared, Unready> {
    if !spec.is_configured() {
        return Err(Unready::NotConfigured);
    }
    let series: Vec<(&SeriesSpec, Cols)> = spec
        .series
        .iter()
        .filter(|s| !s.y.is_empty() || s.aggregate == Aggregate::Count)
        .map(|s| find(data, s).map(|c| (s, c)))
        .collect::<Result<_, _>>()?;
    let kind_of = |c: Option<usize>| c.map(|c| data.columns()[c].kind);
    let first = &series[0];
    let x_time = kind_of(first.1.x) == Some(ColumnKind::Time);
    let y_time = kind_of(first.1.y.first().copied()) == Some(ColumnKind::Time);
    let mut prepared = Prepared {
        kind: spec.kind,
        shape: Shape::Bins { name: String::new(), bins: Vec::new() },
        x_time,
        y_time,
        left_out: 0,
        rows: data.rows(),
    };
    prepared.shape = match spec.kind {
        ChartKind::Pie => slices(data, first, &mut prepared.left_out),
        ChartKind::Histogram => bins(data, first)?,
        ChartKind::Scatter | ChartKind::Bubble => {
            points(data, &series, spec.kind, &mut prepared.left_out)?
        }
        _ => categories(data, &series, spec.kind, &mut prepared.left_out),
    };
    Ok(prepared)
}

/// The label of row `row`'s X (the row's number without an X column).
fn label(data: &dyn Tabular, col: Option<usize>, row: usize) -> String {
    match col {
        Some(c) => data.text(row, c).map(|t| t.into_owned()).unwrap_or_default(),
        None => (row + 1).to_string(),
    }
}

/// A Y value of `row` for an aggregate (a count reads nothing: 1 for any row).
fn y_of(data: &dyn Tabular, how: Aggregate, col: Option<usize>, row: usize) -> Option<f64> {
    match (how, col) {
        (Aggregate::Count, _) => Some(1.0),
        (_, Some(c)) => data.number(row, c),
        (_, None) => None,
    }
}

/// A series' name: its column's, followed by its group's value when it has one.
fn series_name(
    data: &dyn Tabular,
    y: Option<usize>,
    how: Aggregate,
    group: Option<&str>,
) -> String {
    let column = match (y, how) {
        (_, Aggregate::Count) | (None, _) => i18n::t("charts.series_count"),
        (Some(c), _) => data.columns()[c].name.clone(),
    };
    match group {
        Some(g) => format!("{g} · {column}"),
        None => column,
    }
}

fn categories(
    data: &dyn Tabular,
    series: &[(&SeriesSpec, Cols)],
    kind: ChartKind,
    left_out: &mut usize,
) -> Shape {
    // The categories: every series' X values (measured X sorted by place), in the order
    // first met; each series' rows placed by its own X.
    let measured = kind.lined()
        && series.iter().all(|(_, c)| c.x.is_some_and(|x| data.columns()[x].kind.measurable()));
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut labels: Vec<String> = Vec::new();
    let mut places: Vec<f64> = Vec::new();
    let mut rows_of: Vec<Vec<Option<usize>>> = Vec::with_capacity(series.len());
    for (_, cols) in series {
        let x = cols.x;
        let mut of_row: Vec<Option<usize>> = Vec::with_capacity(data.rows());
        for row in 0..data.rows() {
            let name = label(data, x, row);
            let at = match index.get(&name) {
                Some(&i) => Some(i),
                None if labels.len() < MAX_CATEGORIES => {
                    index.insert(name.clone(), labels.len());
                    places.push(x.and_then(|c| data.number(row, c)).unwrap_or(row as f64));
                    labels.push(name);
                    Some(labels.len() - 1)
                }
                None => {
                    *left_out += 1;
                    None
                }
            };
            of_row.push(at);
        }
        rows_of.push(of_row);
    }
    // Each series (split by its group's values), its values summed per category.
    let mut out: Vec<Values> = Vec::new();
    for ((spec, cols), of_row) in series.iter().zip(&rows_of) {
        let ys: Vec<Option<usize>> =
            if cols.y.is_empty() { vec![None] } else { cols.y.iter().copied().map(Some).collect() };
        for y in ys {
            let mut groups: Vec<(Option<String>, Vec<Acc>)> = Vec::new();
            let mut group_index: HashMap<Option<String>, usize> = HashMap::new();
            for (row, &cat) in of_row.iter().enumerate() {
                let Some(cat) = cat else { continue };
                let Some(v) = y_of(data, spec.aggregate, y, row) else { continue };
                let key = cols.group.map(|g| label(data, Some(g), row));
                let g = match group_index.get(&key) {
                    Some(&g) => g,
                    None if groups.len() < MAX_GROUPS => {
                        group_index.insert(key.clone(), groups.len());
                        groups.push((key, vec![Acc::default(); labels.len()]));
                        groups.len() - 1
                    }
                    None => {
                        *left_out += 1;
                        continue;
                    }
                };
                groups[g].1[cat].add(v);
            }
            for (group, accs) in groups {
                out.push(Values {
                    name: series_name(data, y, spec.aggregate, group.as_deref()),
                    values: accs.iter().map(|a| a.value(spec.aggregate)).collect(),
                });
            }
        }
    }
    if !measured {
        return Shape::Categories { labels, xs: None, series: out };
    }
    // Along a measured X: sorted by place.
    let mut order: Vec<usize> = (0..labels.len()).collect();
    order.sort_by(|&a, &b| places[a].total_cmp(&places[b]));
    let pick = |v: &[Option<f64>]| order.iter().map(|&i| v[i]).collect::<Vec<_>>();
    Shape::Categories {
        labels: order.iter().map(|&i| labels[i].clone()).collect(),
        xs: Some(order.iter().map(|&i| places[i]).collect()),
        series: out.into_iter().map(|s| Values { values: pick(&s.values), name: s.name }).collect(),
    }
}

fn points(
    data: &dyn Tabular,
    series: &[(&SeriesSpec, Cols)],
    kind: ChartKind,
    left_out: &mut usize,
) -> Result<Shape, Unready> {
    let mut out: Vec<Points> = Vec::new();
    for (spec, cols) in series {
        let Some(x) = cols.x else { continue };
        if !data.columns()[x].kind.measurable() {
            return Err(Unready::NotMeasurable(data.columns()[x].name.clone()));
        }
        let Some(&y) = cols.y.first() else { continue };
        let size = (kind == ChartKind::Bubble).then(|| cols.y.get(1).copied()).flatten();
        let mut groups: Vec<Points> = Vec::new();
        let mut group_index: HashMap<Option<String>, usize> = HashMap::new();
        for row in 0..data.rows() {
            let (Some(px), Some(py)) = (data.number(row, x), data.number(row, y)) else {
                continue;
            };
            let s = size.and_then(|c| data.number(row, c)).unwrap_or(1.0);
            let key = cols.group.map(|g| label(data, Some(g), row));
            let g = match group_index.get(&key) {
                Some(&g) => g,
                None if groups.len() < MAX_GROUPS => {
                    group_index.insert(key.clone(), groups.len());
                    let name = series_name(data, Some(y), spec.aggregate, key.as_deref());
                    groups.push(Points { name, points: Vec::new() });
                    groups.len() - 1
                }
                None => {
                    *left_out += 1;
                    continue;
                }
            };
            groups[g].points.push([px, py, s]);
        }
        out.extend(groups);
    }
    Ok(Shape::Points(out))
}

fn slices(data: &dyn Tabular, (spec, cols): &(&SeriesSpec, Cols), left_out: &mut usize) -> Shape {
    let y = cols.y.first().copied();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut named: Vec<(String, Acc)> = Vec::new();
    for row in 0..data.rows() {
        let Some(v) = y_of(data, spec.aggregate, y, row) else { continue };
        let name = label(data, cols.x, row);
        let i = *index.entry(name.clone()).or_insert_with(|| {
            named.push((name, Acc::default()));
            named.len() - 1
        });
        named[i].1.add(v);
    }
    let mut slices: Vec<Slice> = named
        .into_iter()
        .filter_map(|(name, acc)| acc.value(spec.aggregate).map(|value| Slice { name, value }))
        .filter(|s| s.value > 0.0)
        .collect();
    slices.sort_by(|a, b| b.value.total_cmp(&a.value));
    let rest = (slices.len() > MAX_SLICES).then(|| {
        let cut = slices.split_off(MAX_SLICES - 1);
        *left_out += cut.len();
        (cut.len(), cut.iter().map(|s| s.value).sum())
    });
    Shape::Slices { slices, rest }
}

/// The values of the first series' first Y in ranges, as many as the Freedman–Diaconis
/// rule says (within [`BINS`]).
fn bins(data: &dyn Tabular, (_, cols): &(&SeriesSpec, Cols)) -> Result<Shape, Unready> {
    let Some(&y) = cols.y.first() else { return Err(Unready::NotConfigured) };
    let mut values: Vec<f64> = (0..data.rows()).filter_map(|r| data.number(r, y)).collect();
    let name = data.columns()[y].name.clone();
    if values.is_empty() {
        return Ok(Shape::Bins { name, bins: Vec::new() });
    }
    values.sort_by(f64::total_cmp);
    let (lo, hi) = (values[0], values[values.len() - 1]);
    let n = values.len() as f64;
    let quartile = |q: f64| values[((n - 1.0) * q).round() as usize];
    let iqr = quartile(0.75) - quartile(0.25);
    let count = if hi <= lo {
        BINS.0
    } else if iqr > 0.0 {
        let width = 2.0 * iqr / n.cbrt();
        (((hi - lo) / width).ceil() as usize).clamp(BINS.0, BINS.1)
    } else {
        // Most values the same: Sturges' rule.
        ((n.log2().ceil() as usize) + 1).clamp(BINS.0, BINS.1)
    };
    let width = if hi > lo { (hi - lo) / count as f64 } else { 1.0 };
    let mut bins: Vec<Bin> = (0..count)
        .map(|i| Bin { from: lo + i as f64 * width, to: lo + (i + 1) as f64 * width, count: 0.0 })
        .collect();
    for v in values {
        let i = (((v - lo) / width) as usize).min(count - 1);
        bins[i].count += 1.0;
    }
    Ok(Shape::Bins { name, bins })
}

#[cfg(test)]
#[path = "../tests/unit/prepare.rs"]
mod tests;
