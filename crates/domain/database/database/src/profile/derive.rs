//! What every view of a profile shows, worked out once and the same way for every engine
//! ([`derive`]): each node's time in all its loops and its own (its children's taken out),
//! its share of the statement, its rows in all loops, how far the estimate was from
//! reality, and the costliest branch. PostgreSQL's special cases are handled here, marked
//! approximate where they are: the workers of a `Gather` (their times are averages),
//! a materialized CTE (paid by its first reader, not twice), rounding past the parent.

use std::sync::Arc;

use crate::plan::Measure;
use crate::profile::Profile;
use crate::profile::node::{ChildRole, Io, NodeId, ObjectKind, OpKind};

/// What's worked out of a node.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Derived {
    /// Its time in all its loops (ms).
    pub inclusive_ms: Option<f64>,
    /// Its own: its children's taken out, never below zero.
    pub self_ms: Option<f64>,
    /// Its own time's share of the statement's (0–1).
    pub self_pct: Option<f64>,
    /// The same, on the planner's cost (an estimated plan: "cost, not time").
    pub self_cost: Option<f64>,
    pub cost_pct: Option<f64>,
    /// Its rows in all its loops.
    pub total_rows: Option<f64>,
    /// Real rows over estimated ones (each at least 1): `120` is ×120 more than expected,
    /// `0.01` a hundred times fewer.
    pub misestimate: Option<f64>,
    pub depth: u32,
    /// Its weight (time, else cost) over its parent's.
    pub share_of_parent: Option<f64>,
    /// A figure here is approximate (parallel workers' averages, rounding past the parent,
    /// a CTE's share).
    pub approx: bool,
}

/// The statement's totals.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Totals {
    /// The statement's time: the engine's execution, else its roots' time.
    pub time_ms: Option<f64>,
    pub cost: Option<f64>,
    pub nodes: usize,
    /// What its weights are measured by (`None`: nothing measured, SQLite's plan).
    pub measure: Option<Measure>,
    pub io: Option<Io>,
    /// The most a node's estimate missed by (either way, as a factor ≥ 1).
    pub worst_misestimate: Option<f64>,
}

/// A profile and what's worked out of it: what the views and the rules read (immutable,
/// shared).
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileReport {
    pub profile: Arc<Profile>,
    /// One per node, by [`NodeId`].
    pub derived: Vec<Derived>,
    /// From the heaviest root, each time the heaviest child, down to a leaf.
    pub hot_path: Vec<NodeId>,
    /// Depth first.
    pub order: Vec<NodeId>,
    pub totals: Totals,
    /// What the analysis found ([`crate::profile::rules::analyze`]; empty until it ran).
    pub findings: Vec<crate::profile::rules::Finding>,
}

impl ProfileReport {
    pub fn derived(&self, id: NodeId) -> &Derived {
        &self.derived[id.index()]
    }

    /// `id` is on the costliest branch.
    pub fn on_hot_path(&self, id: NodeId) -> bool {
        self.hot_path.contains(&id)
    }

    /// About how many bytes it holds.
    pub fn memory(&self) -> usize {
        self.profile.memory()
            + self.derived.len() * std::mem::size_of::<Derived>()
            + (self.hot_path.len() + self.order.len()) * 4
            + self.findings.len() * 512
    }
}

/// Works out what `profile`'s views show.
pub fn derive(profile: impl Into<Arc<Profile>>) -> ProfileReport {
    let profile: Arc<Profile> = profile.into();
    let p = &*profile;
    let n = p.nodes.len();
    let order: Vec<NodeId> = p.walk().into_iter().map(|(id, _)| id).collect();
    let mut derived = vec![Derived::default(); n];
    for (id, depth) in p.walk() {
        derived[id.index()].depth = depth as u32;
    }

    // Under a `Gather`: per-loop times are averages over its workers and leader.
    let mut gather_divisor = vec![None::<f64>; n];
    for &id in &order {
        let node = p.node(id);
        let inherited = node.parent.and_then(|parent| {
            let pn = p.node(parent);
            if pn.op.gathers() {
                let launched = pn.parallel.as_ref().and_then(|par| par.launched).unwrap_or(0);
                (launched > 0).then(|| f64::from(launched) + 1.0)
            } else {
                gather_divisor[parent.index()]
            }
        });
        gather_divisor[id.index()] = inherited;
    }

    let measured = p.depth.runs() && p.nodes.iter().any(|x| x.actuals.total_ms.is_some());
    for &id in &order {
        let node = p.node(id);
        let d = &mut derived[id.index()];
        d.total_rows = node.total_rows();
        if node.actuals.never_executed || node.actuals.loops == Some(0.0) {
            d.inclusive_ms = node.actuals.total_ms.map(|_| 0.0);
            if d.total_rows.is_some() {
                d.total_rows = Some(0.0);
            }
        } else if let Some(t) = node.actuals.total_ms {
            let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
            let mut inclusive = t * loops;
            if let Some(divisor) = gather_divisor[id.index()]
                && loops > 1.0
            {
                inclusive /= divisor.min(loops);
                d.approx = true;
            }
            d.inclusive_ms = Some(inclusive);
        }
        if let (Some(real), Some(est)) = (node.actuals.rows, node.estimates.rows)
            && !node.actuals.never_executed
        {
            d.misestimate = Some(real.max(1.0) / est.max(1.0));
        }
    }

    // The CTEs materialized once (their plan an InitPlan named `CTE x`), and their first
    // reader in depth-first order: it pays for them, their parent doesn't twice.
    let mut cte_plans: Vec<(String, NodeId)> = Vec::new();
    for &id in &order {
        let node = p.node(id);
        if matches!(node.role, Some(ChildRole::InitPlan | ChildRole::Cte))
            && let Some(name) = node.subplan_name.as_deref().and_then(|s| s.strip_prefix("CTE "))
        {
            cte_plans.push((name.trim().to_owned(), id));
        }
    }
    let mut first_reader: Vec<(NodeId, NodeId)> = Vec::new();
    for (name, plan) in &cte_plans {
        let reader = order.iter().copied().find(|&id| {
            let node = p.node(id);
            node.op == OpKind::CteScan
                && node
                    .object
                    .as_ref()
                    .is_some_and(|o| o.kind == ObjectKind::Cte && o.name == *name)
        });
        if let Some(reader) = reader {
            first_reader.push((reader, *plan));
        }
    }
    let cte_plan_ids: Vec<NodeId> = first_reader.iter().map(|(_, plan)| *plan).collect();

    for &id in &order {
        let node = p.node(id);
        // Time.
        if let Some(inclusive) = derived[id.index()].inclusive_ms {
            let mut children = 0.0;
            for child in &node.children {
                if cte_plan_ids.contains(child) {
                    continue;
                }
                children += derived[child.index()].inclusive_ms.unwrap_or(0.0);
            }
            for (reader, plan) in &first_reader {
                if *reader == id {
                    children += derived[plan.index()].inclusive_ms.unwrap_or(0.0);
                }
            }
            let own = inclusive - children;
            let d = &mut derived[id.index()];
            if own < -1e-9 {
                d.approx = true;
            }
            d.self_ms = Some(own.max(0.0));
        }
        // Cost.
        if let Some(total) = node.estimates.total_cost {
            let children: f64 =
                node.children.iter().map(|c| p.node(*c).estimates.total_cost.unwrap_or(0.0)).sum();
            derived[id.index()].self_cost = Some((total - children).max(0.0));
        }
    }

    let roots_time: Option<f64> = {
        let times: Vec<f64> =
            p.roots.iter().filter_map(|r| derived[r.index()].inclusive_ms).collect();
        (!times.is_empty()).then(|| times.iter().sum())
    };
    let time_ms = p.statement.execution_ms.or(roots_time);
    let roots_cost: Option<f64> = {
        let costs: Vec<f64> =
            p.roots.iter().filter_map(|r| p.node(*r).estimates.total_cost).collect();
        (!costs.is_empty()).then(|| costs.iter().sum())
    };
    let measure = if measured && time_ms.is_some() {
        Some(Measure::Time)
    } else if roots_cost.is_some() {
        Some(Measure::Cost)
    } else {
        None
    };

    for &id in &order {
        let node = p.node(id);
        let weight_of = |at: NodeId, d: &[Derived]| match measure {
            Some(Measure::Time) => d[at.index()].inclusive_ms,
            Some(Measure::Cost) => p.node(at).estimates.total_cost,
            None => None,
        };
        let share = node.parent.and_then(|parent| {
            let (w, pw) = (weight_of(id, &derived)?, weight_of(parent, &derived)?);
            (pw > 0.0).then(|| (w / pw).min(1.0))
        });
        let d = &mut derived[id.index()];
        d.share_of_parent = share;
        if let (Some(own), Some(total)) = (d.self_ms, time_ms)
            && total > 0.0
        {
            d.self_pct = Some((own / total).min(1.0));
        }
        if let (Some(own), Some(total)) = (d.self_cost, roots_cost)
            && total > 0.0
        {
            d.cost_pct = Some((own / total).min(1.0));
        }
    }

    // The costliest branch.
    let mut hot_path = Vec::new();
    if let Some(measure) = measure {
        let weight = |id: NodeId| -> f64 {
            match measure {
                Measure::Time => derived[id.index()].inclusive_ms,
                Measure::Cost => p.node(id).estimates.total_cost,
            }
            .unwrap_or(0.0)
        };
        let heaviest = |candidates: &[NodeId]| -> Option<NodeId> {
            let mut best: Option<(NodeId, f64)> = None;
            for &c in candidates {
                if c.index() >= n {
                    continue;
                }
                let w = weight(c);
                if best.is_none_or(|(_, b)| w > b) {
                    best = Some((c, w));
                }
            }
            best.map(|(c, _)| c)
        };
        let mut at = heaviest(&p.roots);
        while let Some(id) = at {
            if hot_path.contains(&id) {
                break;
            }
            hot_path.push(id);
            at = heaviest(&p.node(id).children);
        }
    }

    let worst_misestimate = derived
        .iter()
        .filter_map(|d| d.misestimate)
        .map(|f| if f < 1.0 { 1.0 / f } else { f })
        .fold(None, |best: Option<f64>, f| Some(best.map_or(f, |b| b.max(f))));
    let totals = Totals {
        time_ms,
        cost: roots_cost,
        nodes: n,
        measure,
        io: p.statement.io_total,
        worst_misestimate,
    };
    ProfileReport { profile, derived, hot_path, order, totals, findings: Vec::new() }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/derive.rs"]
mod tests;
