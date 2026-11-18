//! `disabled-node` — the planner was told not to use a kind of step (`enable_seqscan = off`
//! and the like) and used it anyway, or chose a worse plan around it.
//!
//! - **Evidence:** a node marked `Disabled` (PostgreSQL 18), or of a cost of at least
//!   [`Thresholds::disabled_cost`](super::Thresholds) (`disable_cost`, before 18); else an
//!   `enable_*` setting off in the profile's planner settings (variant `setting`).
//! - **Severity:** `Warning`.
//! - **Suggestion:** the setting that disabled it back on, for the session.

use super::{EvidenceKey as E, Finding, Rule, RuleContext, Severity, suggest};
use crate::profile::derive::ProfileReport;
use crate::profile::node::{AggStrategy, OpKind};

pub(super) const ID: &str = "disabled-node";

pub struct DisabledNode;

/// The `enable_*` setting that turns off steps of `op`.
fn setting_of(op: OpKind) -> Option<&'static str> {
    use OpKind as O;
    Some(match op {
        O::SeqScan => "enable_seqscan",
        O::IndexScan => "enable_indexscan",
        O::IndexOnlyScan => "enable_indexonlyscan",
        O::BitmapHeapScan | O::BitmapIndexScan => "enable_bitmapscan",
        O::TidScan => "enable_tidscan",
        O::NestedLoop => "enable_nestloop",
        O::HashJoin => "enable_hashjoin",
        O::MergeJoin => "enable_mergejoin",
        O::Sort => "enable_sort",
        O::IncrementalSort => "enable_incremental_sort",
        O::Aggregate(AggStrategy::Hashed | AggStrategy::Mixed) => "enable_hashagg",
        O::Materialize => "enable_material",
        O::Memoize => "enable_memoize",
        O::GatherMerge => "enable_gathermerge",
        _ => return None,
    })
}

fn is_off(value: &str) -> bool {
    matches!(value.trim().trim_matches('\'').to_ascii_lowercase().as_str(), "off" | "false" | "0")
}

impl Rule for DisabledNode {
    fn id(&self) -> &'static str {
        ID
    }

    fn applies(&self, report: &ProfileReport) -> bool {
        let p = &report.profile;
        p.nodes
            .iter()
            .any(|n| n.flags.disabled || n.estimates.total_cost.is_some_and(|c| c >= 1e10))
            || p.statement
                .settings
                .iter()
                .any(|s| s.name.starts_with("enable_") && is_off(&s.value))
    }

    fn variants(&self) -> &'static [&'static str] {
        &["setting"]
    }

    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding> {
        let t = cx.thresholds;
        let off: Vec<&str> = cx
            .profile()
            .statement
            .settings
            .iter()
            .filter(|s| s.name.starts_with("enable_") && is_off(&s.value))
            .map(|s| s.name.as_str())
            .collect();
        let mut out = Vec::new();
        for (id, node) in cx.nodes() {
            let disabled = node.flags.disabled
                || node.estimates.total_cost.is_some_and(|c| c >= t.disabled_cost);
            if !disabled {
                continue;
            }
            // Its own setting; under a disabled step's cost its parents carry it too: the
            // deepest says it.
            if node.children.iter().any(|c| {
                let n = cx.node(*c);
                n.flags.disabled || n.estimates.total_cost.is_some_and(|x| x >= t.disabled_cost)
            }) && !node.flags.disabled
            {
                continue;
            }
            let setting = setting_of(node.op)
                .filter(|s| off.is_empty() || off.contains(s))
                .or_else(|| off.first().copied());
            let mut f = Finding::new(ID, Severity::Warning)
                .node(id)
                .impact(cx.inclusive_share(id).max(0.1))
                .text(E::Step, cx.step(id))
                .at(cx.node_range(id));
            if let Some(s) = setting {
                f = f.text(E::Setting, s).suggest(suggest::setting(s, "on"));
            }
            out.push(f);
        }
        if out.is_empty()
            && let Some(s) = off.first()
        {
            out.push(
                Finding::new(ID, Severity::Warning)
                    .variant("setting")
                    .impact(0.1)
                    .text(E::Setting, *s)
                    .suggest(suggest::setting(s, "on")),
            );
        }
        out
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/disabled_node.rs"]
mod tests;
