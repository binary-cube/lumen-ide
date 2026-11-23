//! What a chart shows, as the user set it: its kind, its series (which columns go on X, in
//! groups, on Y, and how values falling on one place are made one) and its options. Kept as
//! it is (serialized) so a result opened again finds its chart.

use serde::{Deserialize, Serialize};

/// A chart's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChartKind {
    /// Bars side by side, a group of them per category.
    #[default]
    Bar,
    /// Bars of a category stacked on one another.
    StackedBar,
    /// A line through the values.
    Line,
    /// The line's area filled down to zero.
    Area,
    /// Areas stacked on one another.
    StackedArea,
    /// Points at (x, y).
    Scatter,
    /// Points at (x, y) as large as a third value.
    Bubble,
    /// Slices of a whole.
    Pie,
    /// How many values fall in each range of one column.
    Histogram,
}

impl ChartKind {
    pub const ALL: [ChartKind; 9] = [
        ChartKind::Bar,
        ChartKind::Pie,
        ChartKind::Area,
        ChartKind::Line,
        ChartKind::Scatter,
        ChartKind::Bubble,
        ChartKind::StackedBar,
        ChartKind::StackedArea,
        ChartKind::Histogram,
    ];

    /// Its X is a place on a number line (a scatter's), not one of a list of categories.
    pub fn continuous_x(self) -> bool {
        matches!(self, ChartKind::Scatter | ChartKind::Bubble)
    }

    /// Its series are piled on one another.
    pub fn stacked(self) -> bool {
        matches!(self, ChartKind::StackedBar | ChartKind::StackedArea)
    }

    /// It draws a line (a step, points on it mean something).
    pub fn lined(self) -> bool {
        matches!(self, ChartKind::Line | ChartKind::Area | ChartKind::StackedArea)
    }

    /// It has axes (a pie has none).
    pub fn has_axes(self) -> bool {
        self != ChartKind::Pie
    }
}

/// How the values falling on one place (one category, one group) become one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Aggregate {
    #[default]
    Sum,
    /// How many rows (the Y column isn't read: any row counts).
    Count,
    Average,
    Min,
    Max,
}

/// A series: the columns it's read from, by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct SeriesSpec {
    /// The categories (or a scatter's X). `None`: the rows in order (1, 2, 3…).
    pub x: Option<String>,
    /// A column whose values split the series in one per value (a line per country).
    pub group: Option<String>,
    /// The values: a series each (a bubble's: its Y then its size).
    pub y: Vec<String>,
    pub aggregate: Aggregate,
}

/// What a chart's options change (one missing from what was saved: its default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Options {
    /// A line in steps (each value held until the next).
    pub stepped: bool,
    /// A dot at each value of a line.
    pub points: bool,
    /// Stacked to 100% (each category's share).
    pub percent: bool,
    /// The values' axis on a logarithmic scale (only what's above zero shows).
    pub log: bool,
    /// A pie's hole, a share of its radius in percent (0: a full pie; a ring otherwise, at
    /// most [`MOST_HOLE`]).
    pub hole: u8,
}

/// The widest a ring's hole is (percent of its radius): a ring still to be seen.
pub const MOST_HOLE: u8 = 90;

/// A chart as the user set it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub series: Vec<SeriesSpec>,
    pub options: Options,
}

impl ChartSpec {
    /// It says what to draw: a series with at least a value column (or a count).
    pub fn is_configured(&self) -> bool {
        self.series.iter().any(|s| !s.y.is_empty() || s.aggregate == Aggregate::Count)
    }
}
