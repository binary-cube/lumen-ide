//! `spill-to-disk` — a step's work didn't fit in its memory and went to disk: a sort as an
//! external merge, a hash in several batches, a hash aggregate on disk (PostgreSQL 13+), a
//! material, window or CTE stored on disk (18).
//!
//! - **Evidence:** the spill the driver worked out ([`crate::profile::Spill`]), or what the
//!   engine says of the memory (`external` sort method, `Batches` > 1, storage on disk).
//! - **Severity:** `Warning`.
//! - **Suggestion:** on PostgreSQL, `work_mem` for the session at the power of two above
//!   about twice what went to disk (each such step, and each parallel worker, may take as
//!   much: the text says so). Other engines: none.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity, suggest};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::{ProfileNode, SpillKind, Storage};

pub(super) const ID: &str = "spill-to-disk";

pub struct SpillToDisk;

/// What spilled of `node`: its kind, the bytes on disk and the batches (as far as known).
fn spilled(node: &ProfileNode) -> Option<(SpillKind, Option<u64>, Option<u64>)> {
    if let Some(s) = node.spill {
        return Some((s.kind, s.disk_bytes, s.batches));
    }
    let m = node.memory.as_ref()?;
    if let Some(method) = &m.sort_method
        && method.to_ascii_lowercase().contains("external")
    {
        let disk = m.sort_space.filter(|(_, p)| *p == Storage::Disk).map(|(b, _)| b);
        return Some((SpillKind::Sort, disk, None));
    }
    if let Some(h) = m.hash
        && h.batches.is_some_and(|b| b > 1)
    {
        return Some((SpillKind::Hash, None, h.batches));
    }
    if m.storage == Some(Storage::Disk) {
        return Some((SpillKind::Material, None, None));
    }
    None
}

impl Rule for SpillToDisk {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| spilled(n).is_some())
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            let Some((kind, disk, batches)) = spilled(node) else { continue };
            let share = cx.inclusive_share(id);
            let mut f = Finding::new(ID, Severity::Warning)
                .node(id)
                .impact(share)
                .text(E::Step, cx.step(id))
                .text(E::Spill, kind.id())
                .with(E::Share, share * 100.0, Unit::Percent)
                .at(cx.node_range(id));
            if let Some(d) = disk {
                f = f.with(E::Disk, d as f64, Unit::Bytes);
            }
            if let Some(b) = batches {
                f = f.with(E::Batches, b as f64, Unit::Count);
            }
            // What it would have needed: what went to disk, else its memory × its batches.
            let peak = node.memory.as_ref().and_then(|m| m.peak_bytes);
            let needed = disk.or_else(|| Some(peak? * batches.unwrap_or(1).max(1)));
            if cx.is_postgres()
                && let Some(needed) = needed
            {
                let value = suggest::memory_value(needed.saturating_mul(2));
                f = f.text(E::Value, value.clone()).suggest(suggest::setting("work_mem", &value));
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/spill_to_disk.rs"]
mod tests;
