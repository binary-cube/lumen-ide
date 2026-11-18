//! Two profiles compared (plan 39h6): before and after an index, a rewrite, a setting
//! ([`diff`]). Their steps are matched by their place in the tree — their kind and their
//! object, the children of two matched steps aligned in order (a weighted longest common
//! subsequence) — so a `Seq Scan on orders` that became an `Index Scan on orders` is the same
//! step *changed*, not one removed and another added; children swapped (a hash join's sides)
//! still meet, and a step that only wraps the others (a `Gather` added, a `Hash` gone) lets
//! what's under it meet across it. What's left is added or removed, with its subtree.
//!
//! Each step matched gets its deltas (its time in all its loops, its own, its share, its rows,
//! its buffers, its cost), the statement its own (execution, planning, the runner's clock,
//! rows, buffers, temporary files, cost), and the findings are told apart: resolved (before
//! only), new (after only), kept. Whether a delta is better or worse is the reader's
//! ([`Delta::trend`]): fewer milliseconds are better, fewer rows are neither.

use crate::plan::Measure;
use crate::profile::derive::ProfileReport;
use crate::profile::node::{NodeId, ObjectKind, OpKind, ProfileNode};
use crate::profile::rules::{EvidenceKey, Finding};

/// A change under this share (5 %) is noise: neither better nor worse.
pub const NOISE: f64 = 0.05;

/// What became of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Change {
    /// The same step (its kind, object, index and join).
    Same,
    /// The same place, done another way (`Seq Scan` → `Index Scan` on the same table).
    Changed,
    /// Only after.
    Added,
    /// Only before.
    Removed,
}

impl Change {
    pub fn id(self) -> &'static str {
        match self {
            Change::Same => "same",
            Change::Changed => "changed",
            Change::Added => "added",
            Change::Removed => "removed",
        }
    }
}

/// Which way a figure went, for the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trend {
    Better,
    Worse,
    /// Within [`NOISE`], or a figure that's neither (rows).
    Even,
}

/// A figure before and after.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Delta {
    pub before: Option<f64>,
    pub after: Option<f64>,
}

impl Delta {
    pub fn new(before: Option<f64>, after: Option<f64>) -> Self {
        Self { before, after }
    }

    /// After − before (both known).
    pub fn change(&self) -> Option<f64> {
        Some(self.after? - self.before?)
    }

    /// The change over before (`-0.986`: 98.6 % less); `None` when before was zero or unknown.
    pub fn ratio(&self) -> Option<f64> {
        let before = self.before?;
        let after = self.after?;
        (before.abs() > f64::EPSILON).then(|| (after - before) / before)
    }

    pub fn is_known(&self) -> bool {
        self.before.is_some() || self.after.is_some()
    }

    /// Better when it went down (`lower_is_better`) past [`NOISE`], worse when it went up;
    /// from nothing to something is worse, the other way better.
    pub fn trend(&self, lower_is_better: bool) -> Trend {
        let (Some(before), Some(after)) = (self.before, self.after) else {
            return Trend::Even;
        };
        let up = match self.ratio() {
            Some(r) if r.abs() < NOISE => return Trend::Even,
            Some(r) => r > 0.0,
            None if after.abs() <= f64::EPSILON => return Trend::Even,
            None => after > before,
        };
        match (up, lower_is_better) {
            (true, true) | (false, false) => Trend::Worse,
            _ => Trend::Better,
        }
    }
}

/// A row of the comparison: a step before, after, or both, in a tree merged of the two.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub before: Option<NodeId>,
    pub after: Option<NodeId>,
    pub change: Change,
    /// An earlier row (rows come parents first).
    pub parent: Option<usize>,
    pub depth: u32,
    /// Its time in all its loops (ms).
    pub time: Delta,
    /// Its own time (ms).
    pub self_time: Delta,
    /// Its own share of the statement (0–1: of the time, else of the cost).
    pub share: Delta,
    /// Its rows in all its loops (real, else estimated).
    pub rows: Delta,
    /// The blocks it found in memory and read.
    pub buffers: Delta,
    /// The blocks it read from disk.
    pub read: Delta,
    pub cost: Delta,
}

/// The statement before and after.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StatementDiff {
    /// The statement's time (the engine's execution, else its roots').
    pub time: Delta,
    pub planning: Delta,
    /// The runner's clock (the whole profile, the network included).
    pub wall: Delta,
    pub rows: Delta,
    pub buffers_hit: Delta,
    pub buffers_read: Delta,
    /// Temporary blocks read and written.
    pub temp: Delta,
    pub cost: Delta,
}

/// The findings before and after (problems only: the hot spot is in both).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindingsDiff {
    /// Before's, gone after (indices into before's findings).
    pub resolved: Vec<usize>,
    /// After's, not there before (indices into after's).
    pub new: Vec<usize>,
    /// In both: (before's, after's).
    pub kept: Vec<(usize, usize)>,
}

/// Two profiles compared.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileDiff {
    pub rows: Vec<DiffRow>,
    pub statement: StatementDiff,
    /// A step changed, was added or removed.
    pub shape_changed: bool,
    pub findings: FindingsDiff,
    /// What the shares weigh: the time when both ran, else the cost.
    pub measure: Option<Measure>,
}

impl ProfileDiff {
    /// How many rows of each change: (same, changed, added, removed).
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let n = |c: Change| self.rows.iter().filter(|r| r.change == c).count();
        (n(Change::Same), n(Change::Changed), n(Change::Added), n(Change::Removed))
    }
}

/// `before` and `after` compared (see the module's doc).
pub fn diff(before: &ProfileReport, after: &ProfileReport) -> ProfileDiff {
    let cx = Sides { a: before, b: after };
    let pairs = cx.align(&before.profile.roots, &after.profile.roots);
    let mut rows = Vec::new();
    cx.emit(&pairs, None, 0, &mut rows);
    let shape_changed = rows.iter().any(|r| r.change != Change::Same);
    let both_ran =
        before.totals.measure == Some(Measure::Time) && after.totals.measure == Some(Measure::Time);
    let measure = if both_ran {
        Some(Measure::Time)
    } else if before.totals.cost.is_some() && after.totals.cost.is_some() {
        Some(Measure::Cost)
    } else {
        None
    };
    ProfileDiff {
        rows,
        statement: statement_diff(before, after),
        shape_changed,
        findings: findings_diff(before, after),
        measure,
    }
}

/// How two steps' places are paired.
#[derive(Debug, Clone)]
enum Pair {
    Both(NodeId, NodeId),
    Before(NodeId),
    After(NodeId),
    /// A step before only, the steps under it meeting some after.
    BeforeWrap(NodeId, Vec<Pair>),
    /// A step after only, wrapping some before.
    AfterWrap(NodeId, Vec<Pair>),
}

/// A step's family: what may become what (a scan another scan, a join another join).
fn family(op: OpKind) -> u8 {
    use OpKind as O;
    match op {
        o if o.is_scan() => 1,
        O::NestedLoop | O::HashJoin | O::MergeJoin => 2,
        O::Aggregate(_) | O::WindowAgg | O::Unique => 3,
        O::Sort | O::IncrementalSort | O::TempBTree => 4,
        O::Gather | O::GatherMerge => 5,
        O::Append | O::MergeAppend | O::Compound => 6,
        O::Materialize | O::Memoize => 7,
        _ => 0,
    }
}

/// The table (or other object) a step works on, as compared.
fn object(node: &ProfileNode) -> Option<String> {
    node.object
        .as_ref()
        .filter(|o| o.kind != ObjectKind::Index)
        .map(|o| o.name.to_ascii_lowercase())
}

/// How alike two steps are (0: not to be paired). The same object weighs most; the same
/// kind next; the same family least.
fn similarity(a: &ProfileNode, b: &ProfileNode) -> u32 {
    let same_op = a.op == b.op;
    let same_family = family(a.op) != 0 && family(a.op) == family(b.op);
    match (object(a), object(b)) {
        (Some(x), Some(y)) if x == y => {
            if same_op {
                8
            } else if same_family {
                6
            } else {
                4
            }
        }
        (Some(_), Some(_)) => u32::from(same_op),
        (None, None) => {
            if same_op {
                5
            } else if same_family {
                3
            } else {
                0
            }
        }
        _ => u32::from(same_family),
    }
}

/// The least similarity for steps found across a gap (swapped, or met through a wrapper).
const STRONG: u32 = 4;

/// Past this many cells, two lists of steps are paired greedily (see [`Sides::greedy`]).
const MAX_CELLS: usize = 40_000;

/// Past this many steps left unpaired in a list, no wrapper is looked through.
const MAX_LEFT: usize = 48;

/// How many wrappers deep steps are looked for (a `Gather` over a `Sort` over an aggregate…).
const WRAP_DEPTH: u32 = 4;

/// Past this many pairs of steps both left, two replaced aren't looked through.
const MAX_REPLACED: usize = 9;

/// What looking through a wrapper costs (a direct meeting is preferred).
const WRAP_COST: f64 = 0.05;

/// The similarity of two steps of the same family (a join and another join).
const FAMILY: u32 = 3;

/// Both profiles.
struct Sides<'a> {
    a: &'a ProfileReport,
    b: &'a ProfileReport,
}

impl Sides<'_> {
    fn na(&self, id: NodeId) -> &ProfileNode {
        self.a.profile.node(id)
    }

    fn nb(&self, id: NodeId) -> &ProfileNode {
        self.b.profile.node(id)
    }

    fn sim(&self, x: NodeId, y: NodeId) -> u32 {
        similarity(self.na(x), self.nb(y))
    }

    /// `xs` (before) and `ys` (after) aligned: the most alike in order, then those swapped,
    /// then those meeting through a step that only wraps them.
    fn align(&self, xs: &[NodeId], ys: &[NodeId]) -> Vec<Pair> {
        self.align_in(xs, ys, WRAP_DEPTH)
    }

    /// [`Self::align`], looking through at most `budget` wrappers.
    fn align_in(&self, xs: &[NodeId], ys: &[NodeId], budget: u32) -> Vec<Pair> {
        let xs: Vec<NodeId> =
            xs.iter().copied().filter(|x| x.index() < self.a.profile.nodes.len()).collect();
        let ys: Vec<NodeId> =
            ys.iter().copied().filter(|y| y.index() < self.b.profile.nodes.len()).collect();
        let (n, m) = (xs.len(), ys.len());
        if n * m > MAX_CELLS {
            return self.greedy(&xs, &ys);
        }
        // The weighted longest common subsequence.
        let mut dp = vec![vec![0u32; m + 1]; n + 1];
        for i in 1..=n {
            for j in 1..=m {
                let s = self.sim(xs[i - 1], ys[j - 1]);
                let mut best = dp[i - 1][j].max(dp[i][j - 1]);
                if s > 0 {
                    best = best.max(dp[i - 1][j - 1] + s);
                }
                dp[i][j] = best;
            }
        }
        let mut rev = Vec::new();
        let (mut i, mut j) = (n, m);
        while i > 0 || j > 0 {
            if i > 0 && j > 0 {
                let s = self.sim(xs[i - 1], ys[j - 1]);
                if s > 0 && dp[i][j] == dp[i - 1][j - 1] + s {
                    rev.push(Pair::Both(xs[i - 1], ys[j - 1]));
                    i -= 1;
                    j -= 1;
                    continue;
                }
            }
            if i > 0 && (j == 0 || dp[i][j] == dp[i - 1][j]) {
                rev.push(Pair::Before(xs[i - 1]));
                i -= 1;
            } else {
                rev.push(Pair::After(ys[j - 1]));
                j -= 1;
            }
        }
        rev.reverse();
        let mut pairs = order_gaps(rev);
        // Swapped: an unmatched step before and one after, strongly alike, wherever they are.
        loop {
            let mut found = None;
            'outer: for (pi, p) in pairs.iter().enumerate() {
                let Pair::Before(x) = p else { continue };
                for (qi, q) in pairs.iter().enumerate() {
                    if let Pair::After(y) = q
                        && self.sim(*x, *y) >= STRONG
                    {
                        found = Some((pi, qi, *x, *y));
                        break 'outer;
                    }
                }
            }
            let Some((pi, qi, x, y)) = found else { break };
            pairs[qi] = Pair::Both(x, y);
            pairs.remove(pi);
        }
        // Through a wrapper: a step left on one side whose children meet what's left on the
        // other, or two left whose children meet (a `Gather` added, a `Hash` become a
        // `Sort`) — the candidate meeting the most taken first, while any meets; only between
        // a few (a plan of thousands of partitions isn't unwrapped), so deep.
        let left = |pairs: &[Pair]| {
            pairs.iter().filter(|p| matches!(p, Pair::Before(_) | Pair::After(_))).count()
        };
        if budget == 0 || left(&pairs) > MAX_LEFT {
            return pairs;
        }
        loop {
            let before: Vec<(usize, NodeId)> = pairs
                .iter()
                .enumerate()
                .filter_map(|(i, p)| if let Pair::Before(x) = p { Some((i, *x)) } else { None })
                .collect();
            let after: Vec<(usize, NodeId)> = pairs
                .iter()
                .enumerate()
                .filter_map(|(i, p)| if let Pair::After(y) = p { Some((i, *y)) } else { None })
                .collect();
            if before.is_empty() || after.is_empty() {
                break;
            }
            let ys: Vec<NodeId> = after.iter().map(|(_, y)| *y).collect();
            let xs: Vec<NodeId> = before.iter().map(|(_, x)| *x).collect();
            // (worth, the edit).
            let mut best: Option<(f64, Wrap)> = None;
            let mut consider = |worth: f64, wrap: Wrap| {
                if worth > 0.0 && best.as_ref().is_none_or(|(w, _)| worth > *w + 1e-9) {
                    best = Some((worth, wrap));
                }
            };
            for &(i, x) in &before {
                if self.na(x).children.is_empty() {
                    continue;
                }
                let inner = self.align_in(&self.na(x).children, &ys, budget - 1);
                if meets(&inner, self) {
                    consider(self.worth(&inner, budget - 1) - WRAP_COST, Wrap::Before(i, x, inner));
                }
            }
            for &(j, y) in &after {
                if self.nb(y).children.is_empty() {
                    continue;
                }
                let inner = self.align_in(&xs, &self.nb(y).children, budget - 1);
                if meets(&inner, self) {
                    consider(self.worth(&inner, budget - 1) - WRAP_COST, Wrap::After(j, y, inner));
                }
            }
            if before.len() * after.len() <= MAX_REPLACED {
                for &(i, x) in &before {
                    for &(j, y) in &after {
                        let (cx, cy) = (&self.na(x).children, &self.nb(y).children);
                        if cx.is_empty() || cy.is_empty() {
                            continue;
                        }
                        let inner = self.align_in(cx, cy, budget - 1);
                        if meets(&inner, self) {
                            let worth = self.worth(&inner, budget - 1) - 1.5 * WRAP_COST;
                            consider(worth, Wrap::Both(i, x, j, y, inner));
                        }
                    }
                }
            }
            let Some((_, wrap)) = best else { break };
            match wrap {
                Wrap::Before(i, x, inner) => {
                    let met = matched_after(&inner);
                    let inner: Vec<Pair> =
                        inner.into_iter().filter(|p| !matches!(p, Pair::After(_))).collect();
                    pairs[i] = Pair::BeforeWrap(x, inner);
                    pairs.retain(|p| !matches!(p, Pair::After(y) if met.contains(y)));
                }
                Wrap::After(j, y, inner) => {
                    let met = matched_before(&inner);
                    let inner: Vec<Pair> =
                        inner.into_iter().filter(|p| !matches!(p, Pair::Before(_))).collect();
                    pairs[j] = Pair::AfterWrap(y, inner);
                    pairs.retain(|p| !matches!(p, Pair::Before(x) if met.contains(x)));
                }
                Wrap::Both(i, x, j, y, inner) => {
                    let (left, met): (Vec<Pair>, Vec<Pair>) =
                        inner.into_iter().partition(|p| matches!(p, Pair::Before(_)));
                    pairs[i] = Pair::BeforeWrap(x, left);
                    pairs[j] = Pair::AfterWrap(y, met);
                }
            }
        }
        pairs
    }

    /// How much `pairs` meet: each pair's similarity with what its children meet (looked
    /// at `budget` levels down), a wrapper a little less.
    fn worth(&self, pairs: &[Pair], budget: u32) -> f64 {
        pairs
            .iter()
            .map(|p| match p {
                Pair::Both(x, y) => {
                    let own = f64::from(self.sim(*x, *y));
                    if budget == 0 {
                        own
                    } else {
                        let inner =
                            self.align_in(&self.na(*x).children, &self.nb(*y).children, budget - 1);
                        own + self.worth(&inner, budget - 1)
                    }
                }
                Pair::BeforeWrap(_, inner) | Pair::AfterWrap(_, inner) => {
                    self.worth(inner, budget) - WRAP_COST
                }
                _ => 0.0,
            })
            .sum()
    }

    /// `xs` and `ys` too many to align cell by cell (thousands of partitions): each step
    /// before paired with the first one after of the same kind and object, in order, then
    /// with one of the same object (a partition read another way).
    fn greedy(&self, xs: &[NodeId], ys: &[NodeId]) -> Vec<Pair> {
        let key = |n: &ProfileNode| (n.op, object(n));
        let mut free: std::collections::HashMap<(OpKind, Option<String>), Vec<usize>> =
            std::collections::HashMap::new();
        for (j, y) in ys.iter().enumerate().rev() {
            free.entry(key(self.nb(*y))).or_default().push(j);
        }
        let mut met: Vec<Option<NodeId>> = vec![None; ys.len()];
        let mut left = Vec::new();
        for x in xs {
            match free.get_mut(&key(self.na(*x))).and_then(Vec::pop) {
                Some(j) => met[j] = Some(*x),
                None => left.push(*x),
            }
        }
        // Then by object alone (a partition read another way).
        let mut by_object: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (j, y) in ys.iter().enumerate().rev() {
            if met[j].is_none()
                && let Some(o) = object(self.nb(*y))
            {
                by_object.entry(o).or_default().push(j);
            }
        }
        let mut out = Vec::with_capacity(xs.len() + ys.len());
        for x in left {
            let j = object(self.na(x)).and_then(|o| by_object.get_mut(&o).and_then(Vec::pop));
            match j {
                Some(j) => met[j] = Some(x),
                None => out.push(Pair::Before(x)),
            }
        }
        for (j, y) in ys.iter().enumerate() {
            out.push(match met[j] {
                Some(x) => Pair::Both(x, *y),
                None => Pair::After(*y),
            });
        }
        out
    }

    /// The rows of `pairs` under row `parent`.
    fn emit(&self, pairs: &[Pair], parent: Option<usize>, depth: u32, rows: &mut Vec<DiffRow>) {
        for pair in pairs {
            match pair {
                Pair::Both(x, y) => {
                    let (a, b) = (self.na(*x), self.nb(*y));
                    let same = a.op == b.op
                        && object(a) == object(b)
                        && a.index.as_deref().map(str::to_ascii_lowercase)
                            == b.index.as_deref().map(str::to_ascii_lowercase)
                        && a.join == b.join;
                    let change = if same { Change::Same } else { Change::Changed };
                    let at = self.push(rows, Some(*x), Some(*y), change, parent, depth);
                    let inner = self.align(&a.children, &b.children);
                    self.emit(&inner, Some(at), depth + 1, rows);
                }
                Pair::Before(x) => self.subtree(rows, *x, true, parent, depth),
                Pair::After(y) => self.subtree(rows, *y, false, parent, depth),
                Pair::BeforeWrap(x, inner) => {
                    let at = self.push(rows, Some(*x), None, Change::Removed, parent, depth);
                    self.emit(inner, Some(at), depth + 1, rows);
                }
                Pair::AfterWrap(y, inner) => {
                    let at = self.push(rows, None, Some(*y), Change::Added, parent, depth);
                    self.emit(inner, Some(at), depth + 1, rows);
                }
            }
        }
    }

    /// A step on one side only, with all its subtree.
    fn subtree(
        &self,
        rows: &mut Vec<DiffRow>,
        id: NodeId,
        before: bool,
        parent: Option<usize>,
        depth: u32,
    ) {
        let (at, children) = if before {
            (self.push(rows, Some(id), None, Change::Removed, parent, depth), &self.na(id).children)
        } else {
            (self.push(rows, None, Some(id), Change::Added, parent, depth), &self.nb(id).children)
        };
        let side = if before { &self.a.profile } else { &self.b.profile };
        for c in children {
            if c.index() < side.nodes.len() && depth < 4096 {
                self.subtree(rows, *c, before, Some(at), depth + 1);
            }
        }
    }

    fn push(
        &self,
        rows: &mut Vec<DiffRow>,
        x: Option<NodeId>,
        y: Option<NodeId>,
        change: Change,
        parent: Option<usize>,
        depth: u32,
    ) -> usize {
        let fig = |r: &ProfileReport, id: Option<NodeId>| -> Figures {
            id.map_or_else(Figures::default, |id| Figures::of(r, id))
        };
        let (a, b) = (fig(self.a, x), fig(self.b, y));
        let d = |f: fn(&Figures) -> Option<f64>| Delta::new(f(&a), f(&b));
        rows.push(DiffRow {
            before: x,
            after: y,
            change,
            parent,
            depth,
            time: d(|f| f.time),
            self_time: d(|f| f.self_time),
            share: d(|f| f.share),
            rows: d(|f| f.rows),
            buffers: d(|f| f.buffers),
            read: d(|f| f.read),
            cost: d(|f| f.cost),
        });
        rows.len() - 1
    }
}

/// The steps after of `pairs` that met one before (not those left alone).
fn matched_after(pairs: &[Pair]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for p in pairs {
        match p {
            Pair::Both(_, y) => out.push(*y),
            Pair::BeforeWrap(_, inner) => out.extend(matched_after(inner)),
            Pair::AfterWrap(y, inner) => {
                out.push(*y);
                out.extend(matched_after(inner));
            }
            _ => {}
        }
    }
    out
}

fn matched_before(pairs: &[Pair]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for p in pairs {
        match p {
            Pair::Both(x, _) => out.push(*x),
            Pair::AfterWrap(_, inner) => out.extend(matched_before(inner)),
            Pair::BeforeWrap(x, inner) => {
                out.push(*x);
                out.extend(matched_before(inner));
            }
            _ => {}
        }
    }
    out
}

/// Some pair of `pairs` met, at any depth, as well as two steps of a family do (through a
/// wrapper only those are worth it).
fn meets(pairs: &[Pair], sides: &Sides<'_>) -> bool {
    pairs.iter().any(|p| match p {
        Pair::Both(x, y) => sides.sim(*x, *y) >= FAMILY,
        Pair::BeforeWrap(_, inner) | Pair::AfterWrap(_, inner) => meets(inner, sides),
        _ => false,
    })
}

/// A change of a wrapper's: the pair's place, the step, what's under it met.
enum Wrap {
    Before(usize, NodeId, Vec<Pair>),
    After(usize, NodeId, Vec<Pair>),
    Both(usize, NodeId, usize, NodeId, Vec<Pair>),
}

/// In each gap between steps met, those removed before those added.
fn order_gaps(pairs: Vec<Pair>) -> Vec<Pair> {
    let mut out = Vec::with_capacity(pairs.len());
    let mut added = Vec::new();
    for p in pairs {
        match p {
            Pair::After(_) => added.push(p),
            Pair::Before(_) => out.push(p),
            other => {
                out.append(&mut added);
                out.push(other);
            }
        }
    }
    out.append(&mut added);
    out
}

/// A step's figures, as compared.
#[derive(Debug, Clone, Copy, Default)]
struct Figures {
    time: Option<f64>,
    self_time: Option<f64>,
    share: Option<f64>,
    rows: Option<f64>,
    buffers: Option<f64>,
    read: Option<f64>,
    cost: Option<f64>,
}

impl Figures {
    fn of(report: &ProfileReport, id: NodeId) -> Self {
        let node = report.profile.node(id);
        let d = report.derived(id);
        let rows = d.total_rows.or_else(|| {
            node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0))
        });
        let io = node.io.as_ref();
        let hit = io.and_then(|i| i.shared_hit);
        let read = io.and_then(|i| i.shared_read);
        let buffers = match (hit, read) {
            (None, None) => None,
            (h, r) => Some((h.unwrap_or(0) + r.unwrap_or(0)) as f64),
        };
        Self {
            time: d.inclusive_ms,
            self_time: d.self_ms,
            share: d.self_pct.or(d.cost_pct),
            rows,
            buffers,
            read: read.map(|r| r as f64),
            cost: node.estimates.total_cost,
        }
    }
}

fn statement_diff(a: &ProfileReport, b: &ProfileReport) -> StatementDiff {
    let rows = |r: &ProfileReport| {
        r.profile.statement.rows_returned.map(|v| v as f64).or_else(|| {
            let roots: Vec<f64> =
                r.profile.roots.iter().filter_map(|id| r.derived(*id).total_rows).collect();
            (!roots.is_empty()).then(|| roots.iter().sum())
        })
    };
    let io = |r: &ProfileReport| r.totals.io.or(r.profile.statement.io_total);
    let blocks = |v: Option<u64>| v.map(|b| b as f64);
    let temp = |r: &ProfileReport| {
        io(r).and_then(|i| match (i.temp_read, i.temp_written) {
            (None, None) => None,
            (x, y) => Some((x.unwrap_or(0) + y.unwrap_or(0)) as f64),
        })
    };
    StatementDiff {
        time: Delta::new(a.totals.time_ms, b.totals.time_ms),
        planning: Delta::new(a.profile.statement.planning_ms, b.profile.statement.planning_ms),
        wall: Delta::new(a.profile.statement.wall_ms, b.profile.statement.wall_ms),
        rows: Delta::new(rows(a), rows(b)),
        buffers_hit: Delta::new(
            blocks(io(a).and_then(|i| i.shared_hit)),
            blocks(io(b).and_then(|i| i.shared_hit)),
        ),
        buffers_read: Delta::new(
            blocks(io(a).and_then(|i| i.shared_read)),
            blocks(io(b).and_then(|i| i.shared_read)),
        ),
        temp: Delta::new(temp(a), temp(b)),
        cost: Delta::new(a.totals.cost, b.totals.cost),
    }
}

/// What a finding is about, to know it again in the other profile: its rule and its table (its
/// evidence's, else its first step's object).
fn finding_key(report: &ProfileReport, f: &Finding) -> (&'static str, Option<String>) {
    let table = f
        .text_of(EvidenceKey::Table)
        .map(str::to_ascii_lowercase)
        .or_else(|| f.nodes.first().and_then(|n| report.profile.get(*n)).and_then(object));
    let table = table.map(|t| t.rsplit('.').next().unwrap_or(&t).to_owned());
    (f.rule, table)
}

fn findings_diff(a: &ProfileReport, b: &ProfileReport) -> FindingsDiff {
    let problems = |r: &ProfileReport| -> Vec<usize> {
        (0..r.findings.len()).filter(|&i| r.findings[i].is_problem()).collect()
    };
    let (pa, pb) = (problems(a), problems(b));
    let mut taken = vec![false; pb.len()];
    let mut out = FindingsDiff::default();
    for &i in &pa {
        let key = finding_key(a, &a.findings[i]);
        let found = pb
            .iter()
            .enumerate()
            .find(|(k, j)| !taken[*k] && finding_key(b, &b.findings[**j]) == key)
            .map(|(k, j)| (k, *j));
        match found {
            Some((k, j)) => {
                taken[k] = true;
                out.kept.push((i, j));
            }
            None => out.resolved.push(i),
        }
    }
    out.new = pb.iter().enumerate().filter(|(k, _)| !taken[*k]).map(|(_, j)| *j).collect();
    out
}

#[cfg(test)]
#[path = "../../tests/unit/profile/compare.rs"]
mod tests;
