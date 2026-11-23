//! Charts as data, for every chart in the app (a query's result drawn, the Task Manager's
//! history, the relax mode's widgets, a profile, a Markdown diagram): what a chart shows
//! ([`spec`]), the rows it's made from ([`data`]), made into what's drawn ([`prepare`]), its
//! axes' scales and ticks ([`scale`]), and each mark's place for a size with what's under a
//! point ([`geometry`]). Pure: no drawing, no theme, no files; the component in
//! `ui-components` draws a [`geometry::Geometry`] in the theme's colors.

pub mod data;
pub mod geometry;
pub mod prepare;
pub mod scale;
pub mod spec;
pub mod svg;

pub use data::{Column, ColumnKind, Rows, Tabular, moment};
pub use geometry::{Geometry, Hit, Metrics, View, lay_out};
pub use prepare::{Prepared, Shape, Unready, prepare};
pub use spec::{Aggregate, ChartKind, ChartSpec, Options, SeriesSpec};
