//! A statement's execution plan as a tree ([`PlanNode`]), whatever engine gave it: SQLite's
//! `EXPLAIN QUERY PLAN` rows read here ([`from_query_plan`]), PostgreSQL's
//! `EXPLAIN (FORMAT JSON)` by its driver (JSON is the protocol's and the infra's, not the
//! domain's).

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanNode {
    /// What it does: `Seq Scan`, `Hash Join`, `SCAN users`.
    pub operation: String,
    /// The table, index or CTE it works on.
    pub object: Option<String>,
    /// Its conditions, as the engine writes them (`(id = 1)`, `USING INDEX users_name`).
    pub detail: Option<String>,
    /// The planner's cost before its first row, and in all.
    pub startup_cost: Option<f64>,
    pub total_cost: Option<f64>,
    /// The rows the planner expects.
    pub plan_rows: Option<f64>,
    /// The rows it gave, when it ran (`ANALYZE`; per loop).
    pub actual_rows: Option<f64>,
    /// Its time when it ran, in milliseconds (per loop).
    pub actual_time_ms: Option<f64>,
    pub loops: Option<f64>,
    pub children: Vec<PlanNode>,
}

impl PlanNode {
    pub fn new(operation: impl Into<String>) -> Self {
        Self { operation: operation.into(), ..Self::default() }
    }

    /// The node and every one under it, depth first.
    pub fn walk(&self) -> Vec<&PlanNode> {
        let mut out = vec![self];
        for child in &self.children {
            out.extend(child.walk());
        }
        out
    }
}

impl PlanNode {
    /// Its time in all its loops (milliseconds), when it ran: the time per loop × the loops.
    pub fn inclusive_ms(&self) -> Option<f64> {
        self.actual_time_ms.map(|t| t * self.loops.unwrap_or(1.0).max(1.0))
    }

    /// Its rows in all its loops, when it ran.
    pub fn total_rows(&self) -> Option<f64> {
        self.actual_rows.map(|r| r * self.loops.unwrap_or(1.0).max(1.0))
    }

    /// Its detail, unless its operation already says it (SQLite's rows are one text).
    pub fn own_detail(&self) -> Option<&str> {
        self.detail.as_deref().filter(|d| !d.is_empty() && !self.operation.contains(d))
    }
}

/// What a plan's weight is measured by (its costliest branch, its shares): the time when it
/// ran, else the planner's cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    Time,
    Cost,
}

/// A plan's node in depth-first order: how deep, its parent (an index in the same order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatNode<'a> {
    pub node: &'a PlanNode,
    pub depth: usize,
    pub parent: Option<usize>,
}

/// Every node of `roots`, depth first (a parent before its children, in their order).
pub fn flatten(roots: &[PlanNode]) -> Vec<FlatNode<'_>> {
    fn walk<'a>(
        node: &'a PlanNode,
        depth: usize,
        parent: Option<usize>,
        out: &mut Vec<FlatNode<'a>>,
    ) {
        let at = out.len();
        out.push(FlatNode { node, depth, parent });
        for child in &node.children {
            walk(child, depth + 1, Some(at), out);
        }
    }
    let mut out = Vec::new();
    for root in roots {
        walk(root, 0, None, &mut out);
    }
    out
}

/// What the columns of a plan's table show: only what at least one node has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanColumns {
    /// A detail of its own (conditions, the index used) on some node.
    pub detail: bool,
    pub cost: bool,
    pub rows: bool,
    /// It ran (`ANALYZE`): the real rows, the time, the loops.
    pub actual_rows: bool,
    pub time: bool,
    /// Some node ran more than once.
    pub loops: bool,
}

impl PlanColumns {
    pub fn of(roots: &[PlanNode]) -> Self {
        let mut c = Self::default();
        for f in flatten(roots) {
            let n = f.node;
            c.detail |= n.own_detail().is_some();
            c.cost |= n.total_cost.is_some();
            c.rows |= n.plan_rows.is_some();
            c.actual_rows |= n.actual_rows.is_some();
            c.time |= n.actual_time_ms.is_some();
            c.loops |= n.loops.is_some_and(|l| l > 1.0);
        }
        c
    }

    /// What the weight of a node is measured by (`None`: nothing to measure, as SQLite's
    /// plan).
    pub fn measure(&self) -> Option<Measure> {
        if self.time {
            Some(Measure::Time)
        } else if self.cost {
            Some(Measure::Cost)
        } else {
            None
        }
    }
}

/// A node's weight by `measure` (its time in all its loops, or its total cost).
pub fn weight(node: &PlanNode, measure: Measure) -> Option<f64> {
    match measure {
        Measure::Time => node.inclusive_ms(),
        Measure::Cost => node.total_cost,
    }
}

/// The costliest branch, as indices of [`flatten`]'s order: from the heaviest root, each
/// time the heaviest child (ties: the first), down to a leaf. Empty when nothing is
/// measured (a plan without costs or times).
pub fn hot_path(roots: &[PlanNode]) -> Vec<usize> {
    let Some(measure) = PlanColumns::of(roots).measure() else { return Vec::new() };
    let flat = flatten(roots);
    let heaviest = |candidates: &mut dyn Iterator<Item = usize>| -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for i in candidates {
            let w = weight(flat[i].node, measure).unwrap_or(0.0);
            if best.is_none_or(|(_, b)| w > b) {
                best = Some((i, w));
            }
        }
        best.map(|(i, _)| i)
    };
    let mut path = Vec::new();
    let mut at = heaviest(&mut (0..flat.len()).filter(|&i| flat[i].parent.is_none()));
    while let Some(i) = at {
        path.push(i);
        at = heaviest(&mut (i + 1..flat.len()).filter(|&c| flat[c].parent == Some(i)));
    }
    path
}

/// The plan as text, indented two spaces a level: each node's operation, object, detail,
/// cost, rows and time (what's known), one a line — what "Copy" puts on the clipboard.
pub fn to_text(roots: &[PlanNode]) -> String {
    let mut out = String::new();
    for f in flatten(roots) {
        out.push_str(&"  ".repeat(f.depth));
        out.push_str(&line(f.node));
        out.push('\n');
    }
    out
}

/// A node on one line, as [`to_text`] writes it.
pub fn line(n: &PlanNode) -> String {
    let mut text = n.operation.clone();
    if let Some(object) = &n.object
        && !n.operation.contains(object.as_str())
    {
        text.push_str(" on ");
        text.push_str(object);
    }
    if let Some(detail) = n.own_detail() {
        text.push_str(&format!(" ({detail})"));
    }
    let mut numbers = Vec::new();
    match (n.startup_cost, n.total_cost) {
        (Some(a), Some(b)) => numbers.push(format!("cost={a:.2}..{b:.2}")),
        (None, Some(b)) => numbers.push(format!("cost={b:.2}")),
        _ => {}
    }
    if let Some(rows) = n.plan_rows {
        numbers.push(format!("rows={rows}"));
    }
    if let Some(time) = n.actual_time_ms {
        numbers.push(format!("actual time={time:.3} ms"));
    }
    if let Some(rows) = n.actual_rows {
        numbers.push(format!("actual rows={rows}"));
    }
    if let Some(loops) = n.loops {
        numbers.push(format!("loops={loops}"));
    }
    if !numbers.is_empty() {
        text.push_str(&format!("  ({})", numbers.join(" ")));
    }
    text
}

/// SQLite's `EXPLAIN QUERY PLAN` rows (`id`, `parent`, `detail`, in the order given) as a
/// tree: a row under its parent's id (`0`: the top). A row whose parent isn't there goes to
/// the top; none is lost, none read twice.
pub fn from_query_plan(rows: &[(i64, i64, String)]) -> Vec<PlanNode> {
    let mut placed = vec![false; rows.len()];
    let known: std::collections::HashSet<i64> = rows.iter().map(|r| r.0).collect();
    let tops: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            let (id, parent, _) = &rows[i];
            *parent == 0 || !known.contains(parent) || parent == id
        })
        .collect();
    let mut out = Vec::new();
    for top in tops {
        if !placed[top] {
            out.push(build(rows, top, &mut placed));
        }
    }
    // A cycle (no row reaching the top): each such row at the top, as it came.
    for i in 0..rows.len() {
        if !placed[i] {
            out.push(build(rows, i, &mut placed));
        }
    }
    out
}

fn build(rows: &[(i64, i64, String)], at: usize, placed: &mut [bool]) -> PlanNode {
    placed[at] = true;
    let (id, _, detail) = &rows[at];
    let mut node = sqlite_node(detail);
    for child in 0..rows.len() {
        if !placed[child] && rows[child].1 == *id && rows[child].0 != *id {
            node.children.push(build(rows, child, placed));
        }
    }
    node
}

/// A SQLite plan row's text as a node: `SCAN users`, `SEARCH users USING INDEX x (a=?)`
/// name their table.
fn sqlite_node(detail: &str) -> PlanNode {
    let mut node = PlanNode::new(detail.trim());
    let mut words = detail.split_whitespace().peekable();
    let verb = words.next();
    // Before 3.36: `SCAN TABLE users`.
    if words.peek() == Some(&"TABLE") {
        words.next();
    }
    if let (Some(verb), Some(object)) = (verb, words.next())
        && matches!(verb, "SCAN" | "SEARCH")
    {
        node.object = Some(object.to_owned());
        let rest: Vec<&str> = words.collect();
        if !rest.is_empty() {
            node.detail = Some(rest.join(" "));
        }
    }
    node
}

#[cfg(test)]
#[path = "../tests/unit/plan.rs"]
mod tests;
