//! `cold-cache` — most of the blocks the statement touched were read from disk, not found in
//! memory: a first run (the cache cold), or a working set larger than the cache.
//!
//! - **Evidence:** the statement's shared blocks (summed over its nodes): read at least
//!   [`Thresholds::cold_read_share`](super::Thresholds) (90 %) of those touched, and at least
//!   `cold_blocks` (1 000) touched.
//! - **Severity:** `Info` (variant `no_io_timing` when `track_io_timing` is off: the time
//!   those reads took is unknown).
//! - **Suggestion:** none to write: run it again to compare with a warm cache.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity};
use crate::profile::ProfileNote;
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;

pub(super) const ID: &str = "cold-cache";

pub struct ColdCache;

impl Rule for ColdCache {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.statement.io_total.is_some()
            || report.profile.nodes.iter().any(|n| n.io.is_some())
    }

    fn variants(&self) -> &'static [&'static str] {
        &["no_io_timing"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let p = cx.profile();
        // The roots' blocks (PostgreSQL's include their children's: the nodes' sum would
        // count them twice), else the sum.
        let io = p
            .roots
            .iter()
            .filter_map(|r| p.get(*r)?.io)
            .reduce(|a, b| a.add(&b))
            .or(p.statement.io_total);
        let Some(io) = io else { return Vec::new() };
        let hit = io.shared_hit.unwrap_or(0);
        let read = io.shared_read.unwrap_or(0);
        let touched = hit + read;
        if touched < t.cold_blocks || (read as f64) < touched as f64 * t.cold_read_share {
            return Vec::new();
        }
        let mut f = Finding::new(ID, Severity::Info)
            .impact(read as f64 / touched as f64)
            .with(E::BlocksRead, read as f64, Unit::Blocks)
            .with(E::BlocksTouched, touched as f64, Unit::Blocks)
            .with(E::ReadShare, read as f64 / touched as f64 * 100.0, Unit::Percent);
        if p.has_note(ProfileNote::IoTimingOff) {
            f = f.variant("no_io_timing");
        }
        // The node reading the most of its own.
        let reader = cx
            .nodes()
            .filter(|(_, n)| n.op.is_scan())
            .filter_map(|(id, n)| Some((id, n.io?.shared_read?)))
            .max_by_key(|(_, r)| *r);
        if let Some((id, _)) = reader {
            f = f.node(id).text(E::Step, cx.step(id)).at(cx.node_range(id));
        }
        vec![f]
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/cold_cache.rs"]
mod tests;
