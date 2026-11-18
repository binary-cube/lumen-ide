//! `lossy-bitmap` — a bitmap scan's bitmap didn't fit in `work_mem` and kept whole pages
//! instead of rows (`Lossy Heap Blocks`): each such page's rows are checked again.
//!
//! - **Evidence:** a `BitmapHeapScan` with lossy blocks above zero.
//! - **Severity:** `Info`.
//! - **Suggestion:** a larger `work_mem` for the session: twice the one in use (its planner
//!   settings say it, else PostgreSQL's 4 MB), a power of two.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity, suggest};
use crate::profile::derive::ProfileReport;
use crate::profile::extra::Unit;
use crate::profile::node::OpKind;

pub(super) const ID: &str = "lossy-bitmap";

pub struct LossyBitmap;

/// A memory setting's text (`64MB`, `4096kB`, `1GB`, a number of kB) in bytes.
pub(super) fn memory_bytes(text: &str) -> Option<u64> {
    let t = text.trim().trim_matches('\'');
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (n, unit) = t.split_at(split);
    let n: u64 = n.parse().ok()?;
    let k = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "kb" => 1 << 10,
        "mb" => 1 << 20,
        "gb" => 1 << 30,
        "tb" => 1 << 40,
        "b" => 1,
        _ => return None,
    };
    n.checked_mul(k)
}

impl Rule for LossyBitmap {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        report.profile.nodes.iter().any(|n| n.actuals.lossy_blocks.is_some_and(|l| l > 0.0))
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let current = cx.setting("work_mem").and_then(memory_bytes).unwrap_or(4 << 20);
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            if node.op != OpKind::BitmapHeapScan {
                continue;
            }
            let Some(lossy) = node.actuals.lossy_blocks.filter(|l| *l > 0.0) else { continue };
            let value = suggest::memory_value(current.saturating_mul(2));
            let mut f = Finding::new(ID, Severity::Info)
                .node(id)
                .impact(cx.inclusive_share(id))
                .text(E::Step, cx.step(id))
                .with(E::Lossy, lossy, Unit::Blocks)
                .text(E::Value, value.clone())
                .at(cx.node_range(id));
            if cx.is_postgres() {
                f = f.suggest(suggest::setting("work_mem", &value));
            }
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/lossy_bitmap.rs"]
mod tests;
