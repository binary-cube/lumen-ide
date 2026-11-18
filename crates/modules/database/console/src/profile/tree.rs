//! A profile as a tree on columns ([`ui_components::tree_table`]): a row per step, depth
//! first, the columns only those some step has ([`database::profile::metric::present`], in a
//! fixed order, [`Col`]) —
//!
//! - the operation: the icon of its kind, the engine's words, the object strong, the index
//!   muted (what the words don't say already), the guides and chevrons of the tree;
//! - its own time as a heat bar (its share of the statement: the length and the heat, the
//!   figure written: `412 ms · 48 %`; on a plan only estimated, its own cost — "cost, not
//!   time" in the header's tooltip), its time in all, the rows expected → real with the
//!   factor in a pill when it's ×10 off or more, the loops, the cost, the buffers (found in
//!   memory / read, a bar in two parts), the rows a filter removed, the width, a spill (the
//!   disk's icon and its size), the workers (launched / planned).
//!
//! The costliest branch is marked (an accent stripe, the words stronger); a step a finding
//! is about has its severity's sign in the gutter (the hot spot a flame), a light tint of
//! that color and the figure it's about outlined; the tooltips say the exact values. Past
//! [`BIG_PLAN`] steps, those deeper than [`DEEP`] levels taking under [`SMALL_SHARE`] start
//! folded, saying what they hide (`+1 240 nodes · 3 %`) — never the costliest branch nor a
//! step with a problem, nor what leads to them.

use database::profile::derive::ProfileReport;
use database::profile::export;
use database::profile::metric::{Metric, present};
use database::profile::node::{AggStrategy, NodeId, ObjectKind, OpKind, ProfileNode};
use database::profile::rules::Finding;
use ui_components::ds::Tone;
use ui_components::tree_table::CellValue;
use ui_components::{Ink, Level, Span, SpanStyle, TableCell, TableColumn, TableRow};

use crate::profile::analysis;
use crate::profile::figures::{self, level};

/// Past this many steps, the small deep ones start folded.
pub const BIG_PLAN: usize = 300;
/// How deep a step must be to start folded.
pub const DEEP: u32 = 12;
/// The share of the statement under which a deep step starts folded.
pub const SMALL_SHARE: f64 = 0.005;

/// A column of the profile's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Col {
    Operation,
    Condition,
    SelfTime,
    TotalTime,
    Rows,
    Loops,
    Cost,
    Buffers,
    Removed,
    Width,
    Spill,
    Workers,
}

impl Col {
    /// The columns in their order.
    pub const ORDER: [Col; 12] = [
        Col::Operation,
        Col::SelfTime,
        Col::TotalTime,
        Col::Rows,
        Col::Loops,
        Col::Buffers,
        Col::Removed,
        Col::Spill,
        Col::Workers,
        Col::Cost,
        Col::Width,
        Col::Condition,
    ];

    /// Which go first when the room is short (the first never goes).
    pub const PRIORITY: [Col; 12] = [
        Col::Operation,
        Col::SelfTime,
        Col::Rows,
        Col::TotalTime,
        Col::Spill,
        Col::Buffers,
        Col::Removed,
        Col::Loops,
        Col::Workers,
        Col::Cost,
        Col::Condition,
        Col::Width,
    ];

    /// Its width (logical pixels; the operation's is what's left).
    pub fn width(self) -> f32 {
        match self {
            Col::Operation => 0.0,
            Col::Condition => 200.0,
            Col::SelfTime => 136.0,
            Col::TotalTime => 76.0,
            Col::Rows => 164.0,
            Col::Loops => 54.0,
            Col::Cost => 76.0,
            Col::Buffers => 116.0,
            Col::Removed => 86.0,
            Col::Width => 58.0,
            Col::Spill => 86.0,
            Col::Workers => 66.0,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Col::Operation => "operation",
            Col::Condition => "condition",
            Col::SelfTime => "self_time",
            Col::TotalTime => "total_time",
            Col::Rows => "rows",
            Col::Loops => "loops",
            Col::Cost => "cost",
            Col::Buffers => "buffers",
            Col::Removed => "removed",
            Col::Width => "width",
            Col::Spill => "spill",
            Col::Workers => "workers",
        }
    }

    /// Its narrower width, when the room is short (its figures still whole: the rows,
    /// with their factor's pill, never narrower).
    pub fn compact_width(self) -> f32 {
        match self {
            Col::SelfTime => 116.0,
            Col::Buffers => 92.0,
            Col::Condition => 140.0,
            Col::Spill => 76.0,
            other => other.width(),
        }
    }
}

/// The columns `report` has, in [`Col::ORDER`].
pub fn columns_of(report: &ProfileReport) -> Vec<Col> {
    let metrics = present(report);
    let has = |m: Metric| metrics.contains(&m);
    let conditions = report.profile.nodes.iter().any(|n| condition(n).is_some());
    Col::ORDER
        .into_iter()
        .filter(|c| match c {
            Col::Operation => true,
            Col::Condition => conditions,
            Col::SelfTime => has(Metric::SelfTime) || has(Metric::Cost),
            Col::TotalTime => has(Metric::TotalTime),
            Col::Rows => has(Metric::EstRows) || has(Metric::ActualRows),
            Col::Loops => has(Metric::Loops),
            Col::Cost => has(Metric::Cost) && has(Metric::SelfTime),
            Col::Buffers => has(Metric::SharedHit) || has(Metric::SharedRead),
            Col::Removed => has(Metric::RemovedByFilter),
            Col::Width => has(Metric::Width),
            Col::Spill => has(Metric::Spill),
            Col::Workers => has(Metric::Workers),
        })
        .collect()
}

/// The least and the most the tree's column asks for (logical pixels), whatever its words.
pub const TREE_MIN: f32 = 230.0;
pub const TREE_MAX: f32 = 460.0;

/// The columns fitted to a room: which, and whether at their narrower widths.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub cols: Vec<Col>,
    pub compact: bool,
}

/// Of `columns`, those that fit in `width` logical pixels with the tree's column given
/// `tree` (what its words need, kept between [`TREE_MIN`] and [`TREE_MAX`]): the tree has
/// the priority — the figures narrowed first, then the least important left out
/// ([`Col::PRIORITY`]); the operation and its own time always stay.
pub fn fitting(columns: &[Col], width: f32, tree: f32) -> Fit {
    let need = tree.clamp(TREE_MIN, TREE_MAX);
    let sum = |cols: &[Col], compact: bool| -> f32 {
        cols.iter().map(|c| if compact { c.compact_width() } else { c.width() }).sum()
    };
    let mut kept: Vec<Col> = columns.to_vec();
    if sum(&kept, false) + need <= width {
        return Fit { cols: kept, compact: false };
    }
    for drop in Col::PRIORITY.iter().rev() {
        if sum(&kept, true) + need <= width || kept.len() <= 2 {
            break;
        }
        if *drop != Col::Operation {
            kept.retain(|c| c != drop);
        }
    }
    // What's kept at full width, if the room allows it now.
    let compact = sum(&kept, false) + need > width;
    Fit { cols: kept, compact }
}

/// The table made of a profile: its columns, a row per step (depth first), which step each
/// row is and which row each step, which rows start open.
#[derive(Debug, Clone, Default)]
pub struct ProfileTable {
    pub columns: Vec<TableColumn>,
    pub cols: Vec<Col>,
    pub rows: Vec<TableRow>,
    /// Row → step.
    pub nodes: Vec<NodeId>,
    /// Step → row.
    pub row_of: Vec<usize>,
    pub open: Vec<bool>,
}

/// A step's icon, by its kind.
pub fn op_icon(op: OpKind) -> &'static str {
    use OpKind as O;
    match op {
        O::IndexScan
        | O::IndexOnlyScan
        | O::BitmapHeapScan
        | O::BitmapIndexScan
        | O::RowidLookup
        | O::AutoIndex
        | O::MultiIndexOr
        | O::BloomFilter => "index",
        O::NestedLoop | O::HashJoin | O::MergeJoin | O::Append | O::MergeAppend | O::Compound => {
            "layers"
        }
        O::Hash => "hash",
        O::Sort | O::IncrementalSort | O::TempBTree | O::Unique | O::SetOp => "filter",
        O::Aggregate(_) | O::WindowAgg => "count",
        O::Gather | O::GatherMerge => "cpu",
        O::Materialize | O::Memoize => "memory",
        O::ModifyTable(_) | O::LockRows => "file-pen",
        O::Limit => "minus",
        O::SeqScan
        | O::TidScan
        | O::FunctionScan
        | O::ValuesScan
        | O::CteScan
        | O::SubqueryScan => "table",
        O::Coroutine | O::Result | O::Other => "plan",
    }
}

/// A step's kind, in words (`Hash aggregate`).
pub fn op_kind_text(op: OpKind) -> String {
    let key = match op {
        OpKind::Aggregate(AggStrategy::Plain) => "aggregate".to_owned(),
        other => other.id().to_owned(),
    };
    let label = i18n::t(&format!("profile.op.{key}"));
    if label.starts_with("profile.op.") { key.replace('_', " ") } else { label }
}

/// The condition a step's row shows: the first one its words don't say already.
pub fn condition(node: &ProfileNode) -> Option<&str> {
    node.conditions
        .iter()
        .map(|c| c.text.as_str())
        .find(|t| !t.is_empty() && !node.name.contains(*t))
}

/// The worst finding about each step (`None`: none), the hot spot apart.
fn worst(report: &ProfileReport, findings: &[&Finding]) -> Vec<Option<usize>> {
    let mut out = vec![None::<usize>; report.profile.nodes.len()];
    for (i, f) in findings.iter().enumerate() {
        if !f.is_problem() {
            continue;
        }
        for node in &f.nodes {
            let Some(slot) = out.get_mut(node.index()) else { continue };
            let better = match *slot {
                Some(j) => {
                    let other: &Finding = findings[j];
                    (f.severity, f.impact) > (other.severity, other.impact)
                }
                None => true,
            };
            if better {
                *slot = Some(i);
            }
        }
    }
    out
}

/// The column a rule's finding is about (its figure outlined).
pub fn culprit(rule: &str) -> Col {
    match rule {
        "seq-scan-selective"
        | "filter-removes-most"
        | "sqlite-scan"
        | "function-on-column"
        | "implicit-cast"
        | "or-defeats-index"
        | "leading-wildcard" => Col::Removed,
        "misestimate" | "stale-stats" => Col::Rows,
        "spill-to-disk" | "sqlite-temp-btree" => Col::Spill,
        "nested-loop-many" | "n-plus-one" | "fk-join-no-index" | "auto-index" => Col::Loops,
        "workers-not-launched" => Col::Workers,
        "cold-cache" | "heap-fetches" | "lossy-bitmap" => Col::Buffers,
        _ => Col::SelfTime,
    }
}

/// `report`'s table on `cols` (`findings`: those shown, the worst first).
pub fn table(report: &ProfileReport, cols: &[Col], findings: &[&Finding]) -> ProfileTable {
    let p = &report.profile;
    let measure_time = report.totals.measure == Some(database::plan::Measure::Time);
    let columns: Vec<TableColumn> = cols.iter().map(|c| column(*c, measure_time)).collect();
    let worst = worst(report, findings);
    let hot_spot = findings.iter().find(|f| !f.is_problem()).and_then(|f| f.nodes.first().copied());
    let n = p.nodes.len();
    let mut row_of = vec![usize::MAX; n];
    let mut nodes = Vec::with_capacity(n);
    let mut rows = Vec::with_capacity(n);
    for &id in &report.order {
        row_of[id.index()] = rows.len();
        nodes.push(id);
        let node = p.node(id);
        let d = report.derived(id);
        let parent = node.parent.map(|p| row_of[p.index()]).filter(|&r| r != usize::MAX);
        let finding = worst[id.index()].map(|i| findings[i]);
        let ink = finding.map(|f| Ink::Severity(level(f.severity)));
        let culprit_col = finding.map(|f| culprit(f.rule));
        let cells = cols
            .iter()
            .map(|c| {
                let cell = cell(report, node, id, *c, measure_time);
                if Some(*c) == culprit_col { cell.outlined(ink) } else { cell }
            })
            .collect();
        let mut row = TableRow::new(parent, cells)
            .with_icon(op_icon(node.op))
            .marked(report.on_hot_path(id))
            .tinted(ink);
        if let Some(f) = finding {
            let text = analysis::texts(f);
            let tip = format!("{} · {}\n{}", analysis::severity(f.severity), text.title, text.why);
            row = row.with_sign(
                level(f.severity).icon(),
                ink.unwrap_or(Ink::Tone(Tone::Neutral)),
                tip,
            );
        } else if hot_spot == Some(id) {
            let tip = findings
                .iter()
                .find(|f| !f.is_problem())
                .map(|f| {
                    let t = analysis::texts(f);
                    format!("{}\n{}", t.title, t.why)
                })
                .unwrap_or_default();
            row = row.with_sign("flame", Ink::heat(d.self_pct.or(d.cost_pct).unwrap_or(1.0)), tip);
        }
        rows.push(row);
    }
    // What each folded row would hide.
    let total = report.totals.time_ms.or(report.totals.cost).unwrap_or(0.0);
    let mut below = vec![0usize; n];
    for &id in report.order.iter().rev() {
        if let Some(parent) = p.node(id).parent {
            below[parent.index()] += below[id.index()] + 1;
        }
    }
    for &id in &report.order {
        let count = below[id.index()];
        if count == 0 {
            continue;
        }
        let d = report.derived(id);
        let weight = if measure_time { d.inclusive_ms } else { p.node(id).estimates.total_cost };
        let note = match weight.filter(|_| total > 0.0) {
            Some(w) => i18n::tr!(
                "profile.view.folded_share",
                count = count as i64,
                share = figures::share((w / total).min(1.0)).as_str()
            ),
            None => i18n::tr!("profile.view.folded", count = count as i64),
        };
        let r = row_of[id.index()];
        rows[r].folded_note = Some(note);
    }
    let open = start_open(report, &worst, measure_time);
    let open = report.order.iter().map(|id| open[id.index()]).collect();
    ProfileTable { columns, cols: cols.to_vec(), rows, nodes, row_of, open }
}

/// Which steps start open: all, but past [`BIG_PLAN`] steps those deeper than [`DEEP`]
/// taking under [`SMALL_SHARE`]; the costliest branch, the steps with problems and what
/// leads to them stay open.
pub fn start_open(
    report: &ProfileReport,
    worst: &[Option<usize>],
    measure_time: bool,
) -> Vec<bool> {
    let p = &report.profile;
    let n = p.nodes.len();
    if n <= BIG_PLAN {
        return vec![true; n];
    }
    let total = report.totals.time_ms.or(report.totals.cost).unwrap_or(0.0);
    let mut open: Vec<bool> = (0..n)
        .map(|i| {
            let id = NodeId(i as u32);
            let d = report.derived(id);
            let weight =
                if measure_time { d.inclusive_ms } else { p.node(id).estimates.total_cost };
            let share = match (weight, total > 0.0) {
                (Some(w), true) => w / total,
                _ => 0.0,
            };
            !(d.depth >= DEEP && share < SMALL_SHARE)
        })
        .collect();
    // What must stay seen: its parents opened.
    for i in 0..n {
        let id = NodeId(i as u32);
        if report.on_hot_path(id) || worst.get(i).copied().flatten().is_some() {
            let mut at = p.node(id).parent;
            while let Some(parent) = at {
                open[parent.index()] = true;
                at = p.node(parent).parent;
            }
        }
    }
    open
}

/// The worst finding of each step (the table's signs), for the view.
pub fn worst_of(report: &ProfileReport, findings: &[&Finding]) -> Vec<Option<usize>> {
    worst(report, findings)
}

fn column(c: Col, measure_time: bool) -> TableColumn {
    let key = c.key();
    let (title, tip) = if c == Col::SelfTime && !measure_time {
        (i18n::t("profile.view.col.self_cost"), i18n::t("profile.view.col.self_cost_tip"))
    } else {
        (
            i18n::t(&format!("profile.view.col.{key}")),
            i18n::t(&format!("profile.view.col.{key}_tip")),
        )
    };
    let column = TableColumn::new(title, c.width()).with_tip(tip);
    match c {
        Col::Operation => column.with_min(200.0),
        Col::Condition => column.with_min(80.0),
        _ => column.right().with_min(48.0),
    }
}

/// A step's words split as the tree shows them: its words, its object strong, its index
/// muted (each only when its words don't say it already).
pub fn operation_spans(node: &ProfileNode) -> Vec<Span> {
    let mut spans = Vec::new();
    if let Some(name) = &node.subplan_name {
        spans.push(Span::new(name.clone(), SpanStyle::Muted));
    }
    let mut words = node.name.clone();
    if let Some(join) = node.join
        && join != database::profile::node::JoinKind::Inner
        && !words.contains(join.words())
    {
        words = format!("{} {words}", join.words());
    }
    spans.push(Span::new(words.clone(), SpanStyle::Normal));
    if let Some(object) = node.object.as_ref().filter(|o| o.kind != ObjectKind::Index)
        && !export::has_word(&words, &object.name)
    {
        spans.push(Span::new(object.label(), SpanStyle::Strong));
    }
    if let Some(index) = &node.index
        && !export::has_word(&words, index)
    {
        spans.push(Span::new(index.clone(), SpanStyle::Muted));
    }
    if node.actuals.never_executed {
        spans.push(Span::new(
            i18n::t("profile.view.never_executed"),
            SpanStyle::Pill(Ink::Tone(Tone::Neutral)),
        ));
    }
    if node.flags.disabled {
        spans.push(Span::new(
            i18n::t("profile.view.disabled"),
            SpanStyle::Pill(Ink::Severity(Level::Warning)),
        ));
    }
    spans
}

fn cell(
    report: &ProfileReport,
    node: &ProfileNode,
    id: NodeId,
    c: Col,
    measure_time: bool,
) -> TableCell {
    let d = report.derived(id);
    let approx =
        if d.approx { format!("\n{}", i18n::t("profile.view.approx")) } else { String::new() };
    match c {
        Col::Operation => {
            let tip = format!("{}\n{}", export::head(node), op_kind_text(node.op));
            TableCell::spans(operation_spans(node)).with_tip(tip)
        }
        Col::Condition => match condition(node) {
            Some(text) => TableCell::spans(vec![Span::new(text, SpanStyle::Code)]).with_tip(text),
            None => TableCell::empty(),
        },
        Col::SelfTime => {
            if measure_time {
                let (Some(own), Some(share)) = (d.self_ms, d.self_pct) else {
                    return TableCell::empty();
                };
                let label = format!("{} · {}", figures::ms(own), figures::share(share));
                let tip = i18n::tr!(
                    "profile.view.self_tip",
                    own = figures::ms_exact(own).as_str(),
                    share = figures::share_exact(share).as_str(),
                    total = figures::ms_exact(report.totals.time_ms.unwrap_or(0.0)).as_str()
                );
                TableCell::bar(share as f32, Ink::heat(share), label)
                    .with_tip(format!("{tip}{approx}"))
            } else {
                let (Some(own), Some(share)) = (d.self_cost, d.cost_pct) else {
                    return TableCell::empty();
                };
                let label = format!("{} · {}", figures::number(own.round()), figures::share(share));
                let tip = i18n::tr!(
                    "profile.view.self_cost_tip",
                    own = figures::number(own).as_str(),
                    share = figures::share_exact(share).as_str()
                );
                TableCell::bar(share as f32, Ink::heat(share), label).with_tip(tip)
            }
        }
        Col::TotalTime => match d.inclusive_ms {
            Some(total) => {
                let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
                let mut tip = figures::ms_exact(total);
                if loops > 1.0 {
                    tip = i18n::tr!(
                        "profile.view.total_tip",
                        total = figures::ms_exact(total).as_str(),
                        per = figures::ms_exact(node.actuals.total_ms.unwrap_or(0.0)).as_str(),
                        loops = figures::count(loops).as_str()
                    );
                }
                TableCell::text(figures::ms(total)).with_tip(format!("{tip}{approx}"))
            }
            None => TableCell::empty(),
        },
        Col::Rows => {
            let est = node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0));
            match (est, d.total_rows) {
                (Some(est), Some(actual)) => {
                    let mut spans = vec![
                        Span::new(figures::count(est), SpanStyle::Muted),
                        Span::new("→", SpanStyle::Muted),
                        Span::new(figures::count(actual), SpanStyle::Normal),
                    ];
                    let ratio = d.misestimate.unwrap_or(1.0);
                    let off = if ratio >= 1.0 { ratio } else { 1.0 / ratio.max(1e-9) };
                    // Fewer rows than planned under a LIMIT: what the limit is for.
                    let limited = ratio < 1.0 && under_limit(report, id);
                    if off >= 10.0 && !limited {
                        let severity = if off >= 1000.0 { Level::Critical } else { Level::Warning };
                        spans.insert(
                            0,
                            Span::new(
                                figures::factor(ratio),
                                SpanStyle::Pill(Ink::Severity(severity)),
                            ),
                        );
                    }
                    let tip = i18n::tr!(
                        "profile.view.rows_tip",
                        expected = figures::count(est).as_str(),
                        actual = figures::count(actual).as_str(),
                        factor = figures::factor(ratio).as_str()
                    );
                    TableCell::spans(spans).with_tip(tip)
                }
                (Some(est), None) => TableCell::text(figures::count(est)).with_tip(i18n::tr!(
                    "profile.view.rows_est_tip",
                    expected = figures::count(est).as_str()
                )),
                (None, Some(actual)) => TableCell::text(figures::count(actual)),
                (None, None) => TableCell::empty(),
            }
        }
        Col::Loops => match node.actuals.loops {
            Some(l) => TableCell::text(figures::count(l)),
            None => TableCell::empty(),
        },
        Col::Cost => match node.estimates.total_cost {
            Some(total) => TableCell::text(figures::number(total.round())).with_tip(i18n::tr!(
                "profile.view.cost_tip",
                startup = figures::number(node.estimates.startup_cost.unwrap_or(0.0)).as_str(),
                total = figures::number(total).as_str()
            )),
            None => TableCell::empty(),
        },
        Col::Buffers => {
            let Some(io) = node.io.as_ref() else { return TableCell::empty() };
            let (hit, read) = (io.shared_hit.unwrap_or(0), io.shared_read.unwrap_or(0));
            if io.shared_hit.is_none() && io.shared_read.is_none() {
                return TableCell::empty();
            }
            let all = (hit + read).max(1) as f32;
            let label = figures::bytes(io.bytes(hit + read));
            let tip = i18n::tr!(
                "profile.view.buffers_tip",
                hit = figures::count(hit as f64).as_str(),
                hit_bytes = figures::bytes(io.bytes(hit)).as_str(),
                read = figures::count(read as f64).as_str(),
                read_bytes = figures::bytes(io.bytes(read)).as_str()
            );
            TableCell::of(CellValue::Split { a: hit as f32 / all, b: read as f32 / all, label })
                .with_tip(tip)
        }
        Col::Removed => {
            let a = &node.actuals;
            match (a.rows_removed_by_filter, a.rows_removed_by_join_filter) {
                (None, None) => TableCell::empty(),
                (x, y) => {
                    let loops = a.loops.unwrap_or(1.0).max(1.0);
                    let sum = (x.unwrap_or(0.0) + y.unwrap_or(0.0)) * loops;
                    TableCell::text(figures::count(sum)).with_tip(i18n::tr!(
                        "profile.view.removed_tip",
                        rows = figures::count(sum).as_str()
                    ))
                }
            }
        }
        Col::Width => match node.estimates.width {
            Some(w) => TableCell::text(format!("{} B", figures::count(w))),
            None => TableCell::empty(),
        },
        Col::Spill => match node.spill.and_then(|s| s.disk_bytes) {
            Some(bytes) => TableCell::text(figures::bytes(bytes))
                .with_lead("hard-drive", Ink::Severity(Level::Warning))
                .with_tip(i18n::tr!(
                    "profile.view.spill_tip",
                    bytes = figures::bytes(bytes).as_str(),
                    exact = figures::count(bytes as f64).as_str()
                )),
            None => TableCell::empty(),
        },
        Col::Workers => match node.parallel.as_ref() {
            Some(par) if par.launched.is_some() || par.planned.is_some() => {
                let launched = par.launched.unwrap_or(0);
                let text = match par.planned {
                    Some(planned) => format!("{launched}/{planned}"),
                    None => launched.to_string(),
                };
                let short = par.planned.is_some_and(|p| launched < p);
                let style = if short {
                    SpanStyle::Ink(Ink::Severity(Level::Warning))
                } else {
                    SpanStyle::Normal
                };
                TableCell::spans(vec![Span::new(text, style)]).with_tip(i18n::tr!(
                    "profile.view.workers_tip",
                    launched = i64::from(launched),
                    planned = i64::from(par.planned.unwrap_or(launched))
                ))
            }
            _ => TableCell::empty(),
        },
    }
}

/// A step under a `LIMIT` (it may stop before giving what it planned).
fn under_limit(report: &ProfileReport, id: NodeId) -> bool {
    let p = &report.profile;
    let mut at = p.node(id).parent;
    while let Some(parent) = at {
        if p.node(parent).op == OpKind::Limit {
            return true;
        }
        at = p.node(parent).parent;
    }
    false
}

#[cfg(test)]
#[path = "../../tests/unit/profile/tree.rs"]
mod tests;
