//! The figures a profile's table shows ([`Metric`]): each one's value on a node, and the
//! columns a profile has ([`present`]: those at least one node has, in a fixed order). An
//! estimated SQLite plan shows only its operations and objects; PostgreSQL's `ANALYZE,
//! BUFFERS` every column — the same view.

use crate::profile::derive::{Derived, ProfileReport};
use crate::profile::extra::{Quantity, Unit};
use crate::profile::node::ProfileNode;

/// A column of a profile's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    SelfTime,
    TotalTime,
    StartupTime,
    EstRows,
    ActualRows,
    Loops,
    /// Real rows over estimated ones.
    RowsRatio,
    Cost,
    Width,
    SharedHit,
    SharedRead,
    /// Blocks dirtied or written.
    Written,
    /// Temporary blocks read and written.
    Temp,
    /// Bytes spilled to disk.
    Spill,
    RemovedByFilter,
    Workers,
    Memory,
}

impl Metric {
    /// Every metric, in the table's order.
    pub const ALL: [Metric; 17] = [
        Metric::SelfTime,
        Metric::TotalTime,
        Metric::StartupTime,
        Metric::EstRows,
        Metric::ActualRows,
        Metric::Loops,
        Metric::RowsRatio,
        Metric::Cost,
        Metric::Width,
        Metric::SharedHit,
        Metric::SharedRead,
        Metric::Written,
        Metric::Temp,
        Metric::Spill,
        Metric::RemovedByFilter,
        Metric::Workers,
        Metric::Memory,
    ];

    /// Stable (the settings' saved columns, the locales' keys): `self_time`…
    pub fn id(self) -> &'static str {
        match self {
            Metric::SelfTime => "self_time",
            Metric::TotalTime => "total_time",
            Metric::StartupTime => "startup_time",
            Metric::EstRows => "est_rows",
            Metric::ActualRows => "actual_rows",
            Metric::Loops => "loops",
            Metric::RowsRatio => "rows_ratio",
            Metric::Cost => "cost",
            Metric::Width => "width",
            Metric::SharedHit => "shared_hit",
            Metric::SharedRead => "shared_read",
            Metric::Written => "written",
            Metric::Temp => "temp",
            Metric::Spill => "spill",
            Metric::RemovedByFilter => "removed_by_filter",
            Metric::Workers => "workers",
            Metric::Memory => "memory",
        }
    }

    /// Its value on `node` (`derived`: what's worked out of it).
    pub fn of(self, node: &ProfileNode, derived: &Derived) -> Option<Quantity> {
        let q = |value: Option<f64>, unit: Unit| value.map(|v| Quantity::new(v, unit));
        let io = node.io.as_ref();
        let blocks = |v: Option<u64>| v.map(|b| b as f64);
        match self {
            Metric::SelfTime => q(derived.self_ms, Unit::Millis),
            Metric::TotalTime => q(derived.inclusive_ms, Unit::Millis),
            Metric::StartupTime => q(node.actuals.startup_ms, Unit::Millis),
            Metric::EstRows => q(node.estimates.rows, Unit::Rows),
            Metric::ActualRows => q(derived.total_rows, Unit::Rows),
            Metric::Loops => q(node.actuals.loops, Unit::Count),
            Metric::RowsRatio => q(derived.misestimate, Unit::Ratio),
            Metric::Cost => q(node.estimates.total_cost, Unit::Cost),
            Metric::Width => q(node.estimates.width, Unit::Bytes),
            Metric::SharedHit => q(blocks(io.and_then(|i| i.shared_hit)), Unit::Blocks),
            Metric::SharedRead => q(blocks(io.and_then(|i| i.shared_read)), Unit::Blocks),
            Metric::Written => {
                let sum = io.and_then(|i| match (i.shared_dirtied, i.shared_written) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                });
                q(blocks(sum), Unit::Blocks)
            }
            Metric::Temp => {
                let sum = io.and_then(|i| match (i.temp_read, i.temp_written) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
                });
                q(blocks(sum), Unit::Blocks)
            }
            Metric::Spill => {
                q(node.spill.and_then(|s| s.disk_bytes).map(|b| b as f64), Unit::Bytes)
            }
            Metric::RemovedByFilter => {
                let a = &node.actuals;
                let sum = match (a.rows_removed_by_filter, a.rows_removed_by_join_filter) {
                    (None, None) => None,
                    (x, y) => Some(x.unwrap_or(0.0) + y.unwrap_or(0.0)),
                };
                q(sum, Unit::Rows)
            }
            Metric::Workers => q(
                node.parallel.as_ref().and_then(|p| p.launched.or(p.planned)).map(f64::from),
                Unit::Count,
            ),
            Metric::Memory => q(
                node.memory
                    .as_ref()
                    .and_then(|m| m.peak_bytes.or(m.sort_space.map(|(bytes, _)| bytes)))
                    .map(|b| b as f64),
                Unit::Bytes,
            ),
        }
    }
}

/// The metrics at least one node of `report` has, in [`Metric::ALL`]'s order.
pub fn present(report: &ProfileReport) -> Vec<Metric> {
    Metric::ALL
        .into_iter()
        .filter(|m| {
            report
                .profile
                .nodes
                .iter()
                .zip(&report.derived)
                .any(|(node, derived)| m.of(node, derived).is_some())
        })
        .collect()
}
