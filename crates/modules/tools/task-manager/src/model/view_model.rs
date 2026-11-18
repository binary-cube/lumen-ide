//! What a Task Manager view lists, worked out once per look or change of its state (never
//! while drawing): the operations (running, then the last ended), then each group's header
//! with its totals and, unless folded, its parts — those the filter finds, sorted — each
//! with its numbers written. Built from the look's frame and the view's state; the view
//! keeps the last one and builds again only when either changed (their generations).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use i18n::t;
use kernel::ProjectId;
use workbench_model::Activity;

use super::format::{PartTexts, ProcessTexts, kind_label, percent, progress, status};
use super::groups::{GroupKey, Grouping, belongs, groups};
use crate::collector::{Frame, Line};

/// What the parts are sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Cpu,
    Memory,
    Gpu,
    Net,
    Disk,
    Threads,
}

/// The ended operations shown (the last ones; each project keeps more, for later).
pub const ENDED_SHOWN: usize = 10;

/// What a view chose: how it sorts (by what, the largest first), what it filters, what it
/// folded, how it groups.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub sort: (Column, bool),
    pub filter: String,
    pub folded: HashSet<GroupKey>,
    pub grouping: Grouping,
    /// The parts whose programs are listed under them (by key).
    pub expanded: HashSet<String>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            // The busiest first.
            sort: (Column::Cpu, true),
            filter: String::new(),
            // The modules start folded: there are many.
            folded: HashSet::from([GroupKey::Kind(workbench_model::resources::MODULES)]),
            grouping: Grouping::Kind,
            expanded: HashSet::new(),
        }
    }
}

/// An operation as listed: itself, the part it's of (its key), its project's name (in the
/// view of every project), its texts.
#[derive(Debug, Clone)]
pub struct OperationRow {
    pub activity: Activity,
    pub owner: String,
    pub project: Option<String>,
    /// What it is, in the user's language.
    pub kind: String,
    /// How much it did.
    pub progress: String,
    /// Where it is (how far, how fast, how it ended).
    pub status: String,
}

/// A row of the list: a group's header, a part, or an operation.
#[derive(Debug, Clone)]
pub enum Item {
    Group {
        key: GroupKey,
        name: String,
        /// Its name and size as its header says them (`Indexing · 3`).
        title: String,
        count: usize,
        cpu: f32,
        memory: u64,
        /// Its totals, written.
        cpu_text: String,
        memory_text: Option<String>,
        /// The operations' header: how many run (said instead of the numbers).
        running: Option<usize>,
        open: bool,
    },
    /// A part: its place in the frame's lines, its numbers written, the line its row shows
    /// under its name in a two-line list (its other numbers than the one sorted by).
    Part {
        index: usize,
        texts: PartTexts,
        others: String,
    },
    /// A program a part started (its row under the part, opened): the part's place in the
    /// frame's lines, the program's place in its processes, its numbers written.
    Process {
        part: usize,
        process: usize,
        texts: ProcessTexts,
    },
    Operation(OperationRow),
}

/// The selected part as it stands: its last row, and whether it's gone (no longer in the
/// looks: kept with its last values, said so).
#[derive(Debug, Clone, PartialEq)]
pub struct Selected {
    pub line: Line,
    pub gone: bool,
}

/// What a view lists, for one look and one state.
#[derive(Debug, Clone)]
pub struct ViewModel {
    pub frame: Arc<Frame>,
    pub items: Vec<Item>,
    /// The parts' keys in the order shown (folded groups' left out).
    pub keys: Vec<String>,
    /// The parts the filter finds (their places in the frame), sorted.
    pub shown: Vec<usize>,
    /// What it was built for: the look's and the state's generations.
    pub stamp: (u64, u64),
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            frame: Arc::new(Frame::default()),
            items: Vec::new(),
            keys: Vec::new(),
            shown: Vec::new(),
            stamp: (0, 0),
        }
    }
}

impl ViewModel {
    /// The list for `frame` (look `look`) as `state` (generation `generation`) wants it,
    /// the operations' times counted to `now`.
    pub fn build(frame: Arc<Frame>, state: &ViewState, stamp: (u64, u64), now: Instant) -> Self {
        let filter = ui_kit::ListFilter::new(&state.filter);
        let filtering = !state.filter.trim().is_empty();
        let shown = shown(&frame, state, &filter);
        let mut items = Vec::new();
        let operations = operations(&frame, state, &filter, now);
        if !operations.is_empty() {
            let open = filtering || !state.folded.contains(&GroupKey::Operations);
            let name = t("tasks.operations");
            let running = operations.iter().filter(|o| o.activity.running()).count();
            items.push(Item::Group {
                key: GroupKey::Operations,
                title: format!("{name}  ·  {}", operations.len()),
                name,
                count: operations.len(),
                cpu: 0.0,
                memory: 0,
                // How many run, said where the others have their CPU.
                cpu_text: i18n::tr!("tasks.running", count = running),
                memory_text: None,
                running: Some(running),
                open,
            });
            if open {
                items.extend(operations.into_iter().map(Item::Operation));
            }
        }
        let sorted = state.sort.0;
        let push_part = |items: &mut Vec<Item>, index: usize| {
            let line = &frame.lines[index];
            let texts = PartTexts::of(line);
            let others = texts.others(sorted);
            items.push(Item::Part { index, texts, others });
            if state.expanded.contains(&line.key) {
                for (process, row) in line.processes.iter().enumerate() {
                    items.push(Item::Process {
                        part: index,
                        process,
                        texts: ProcessTexts::of(row),
                    });
                }
            }
        };
        let grouping = state.grouping.on(&frame);
        if grouping == Grouping::List {
            for &i in &shown {
                push_part(&mut items, i);
            }
        }
        for (key, name) in groups(&frame, grouping) {
            let parts: Vec<usize> = shown
                .iter()
                .copied()
                .filter(|&i| belongs(&frame, grouping, &frame.lines[i], key))
                .collect();
            if parts.is_empty() {
                continue;
            }
            let open = filtering || !state.folded.contains(&key);
            let cpu: f32 = parts.iter().map(|&i| frame.lines[i].cpu).sum();
            let memory: u64 = parts.iter().filter_map(|&i| frame.lines[i].memory).sum();
            items.push(Item::Group {
                key,
                title: format!("{name}  ·  {}", parts.len()),
                name,
                count: parts.len(),
                cpu,
                memory,
                cpu_text: percent(cpu),
                memory_text: (memory > 0).then(|| i18n::bytes(memory)),
                running: None,
                open,
            });
            if open {
                for i in parts {
                    push_part(&mut items, i);
                }
            }
        }
        let keys = items
            .iter()
            .filter_map(|i| match i {
                Item::Part { index, .. } => Some(frame.lines[*index].key.clone()),
                Item::Group { .. } | Item::Process { .. } | Item::Operation(_) => None,
            })
            .collect();
        Self { frame, items, keys, shown, stamp }
    }

    /// The parts the filter finds, sorted.
    pub fn shown_lines(&self) -> impl Iterator<Item = &Line> + '_ {
        self.shown.iter().map(|&i| &self.frame.lines[i])
    }

    /// The operations listed (open or not).
    pub fn operations(&self) -> impl Iterator<Item = &OperationRow> + '_ {
        self.items.iter().filter_map(|i| match i {
            Item::Operation(o) => Some(o),
            Item::Group { .. } | Item::Part { .. } | Item::Process { .. } => None,
        })
    }
}

/// Whether the filter finds `line`: by its name, owner or detail, a program's id or name.
fn matches(filter: &ui_kit::ListFilter, line: &Line) -> bool {
    if filter.matches_any([line.name.as_str(), &line.owner, &line.detail]) {
        return true;
    }
    line.processes.iter().any(|p| {
        let pid = p.pid.to_string();
        filter.matches_any([pid.as_str(), p.name.as_deref().unwrap_or("")])
    })
}

/// The parts the filter finds, sorted (stable: equal ones keep the look's order).
fn shown(frame: &Frame, state: &ViewState, filter: &ui_kit::ListFilter) -> Vec<usize> {
    let mut shown: Vec<usize> =
        (0..frame.lines.len()).filter(|&i| matches(filter, &frame.lines[i])).collect();
    let (column, descending) = state.sort;
    shown.sort_by(|&a, &b| {
        let (a, b) = (&frame.lines[a], &frame.lines[b]);
        let order = match column {
            Column::Name => a
                .name
                .chars()
                .flat_map(char::to_lowercase)
                .cmp(b.name.chars().flat_map(char::to_lowercase)),
            Column::Cpu => a.cpu.total_cmp(&b.cpu),
            Column::Memory => a.memory.unwrap_or(0).cmp(&b.memory.unwrap_or(0)),
            Column::Gpu => a.gpu.unwrap_or(0).cmp(&b.gpu.unwrap_or(0)),
            Column::Net => a.net.unwrap_or(0.0).total_cmp(&b.net.unwrap_or(0.0)),
            Column::Disk => a.disk.unwrap_or(0.0).total_cmp(&b.disk.unwrap_or(0.0)),
            Column::Threads => a.threads.cmp(&b.threads),
        };
        if descending { order.reverse() } else { order }
    });
    shown
}

/// The operations shown: the running ones (the oldest first), then the last ended (the
/// newest first) — matching the filter; by name when the list is sorted so.
fn operations(
    frame: &Frame,
    state: &ViewState,
    filter: &ui_kit::ListFilter,
    now: Instant,
) -> Vec<OperationRow> {
    let project_name = |id: Option<ProjectId>| {
        let id = id?;
        frame.projects.iter().find(|p| p.id == id).map(|p| p.name.clone())
    };
    let rows = frame.lines.iter().flat_map(|line| {
        line.activities.iter().map(|a| OperationRow {
            activity: a.clone(),
            owner: line.key.clone(),
            project: frame.all.then(|| project_name(a.project)).flatten(),
            // What it is, and where (a query's source and console).
            kind: if a.detail.is_empty() {
                kind_label(a.kind)
            } else {
                format!("{} · {}", kind_label(a.kind), a.detail)
            },
            progress: String::new(),
            status: String::new(),
        })
    });
    let found = |row: &OperationRow| {
        let texts =
            [row.activity.subject.as_str(), &row.kind, row.project.as_deref().unwrap_or("")];
        filter.matches_any(texts)
    };
    let (mut running, mut ended): (Vec<OperationRow>, Vec<OperationRow>) =
        rows.filter(|r| found(r)).partition(|r| r.activity.running());
    running.sort_by_key(|r| r.activity.started);
    ended.sort_by_key(|r| std::cmp::Reverse(r.activity.ended.as_ref().map(|(at, _)| *at)));
    ended.truncate(ENDED_SHOWN);
    let mut all = running;
    all.extend(ended);
    if state.sort.0 == Column::Name {
        all.sort_by_cached_key(|r| r.activity.subject.to_lowercase());
        if !state.sort.1 {
            all.reverse();
        }
    }
    for row in &mut all {
        // Every project's view: whose it is, after what it is.
        if let Some(project) = &row.project {
            row.kind = format!("{} · {project}", row.kind);
        }
        row.progress = progress(&row.activity);
        row.status = status(&row.activity, now);
    }
    all
}

/// The part `key` as it stands in `frame`: its row now, or — gone — its `last` one, said
/// gone (`None`: never seen).
pub fn selection(frame: &Frame, key: &str, last: Option<&Line>) -> Option<Selected> {
    match frame.line(key) {
        Some(line) => Some(Selected { line: line.clone(), gone: false }),
        None => last.filter(|l| l.key == key).map(|l| Selected { line: l.clone(), gone: true }),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/model/view_model.rs"]
mod tests;
