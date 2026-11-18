//! A statement's profile, normalized for every engine (plan 39h2): what each step did and
//! what the statement cost, the same model whatever the driver (PostgreSQL's `EXPLAIN
//! (ANALYZE, BUFFERS, FORMAT JSON)`, SQLite's measured run, an external driver's answer),
//! what one engine says beyond it kept as [`Extra`] — nothing is lost.
//!
//! The driver profiles ([`crate::Session::profile`], by default from its plan:
//! [`Profile::from_plan`]); the domain works out what every view shows the same way
//! ([`derive`]: self time, shares, the costliest branch, estimates against reality) and
//! writes it out ([`export`]: as text, as our versioned JSON) and analyzes it ([`rules`]:
//! pure rules finding what could be faster, with their evidence and SQL to suggest; the
//! conditions read by [`cond`], the nodes taken back to the statement by [`sql_map`]),
//! compares two of them ([`compare`]: the steps matched, their deltas, the findings resolved
//! and new) and says what the statement did with each table ([`tables::usage`]). The tree is flat
//! ([`ProfileNode`]s in a vector, [`NodeId`] their place): a plan of thousands of partitions
//! is walked, virtualized and compared by index. A profile never changes once made.

pub mod compare;
pub mod cond;
pub mod derive;
pub mod export;
pub mod extra;
pub mod metric;
pub mod node;
pub mod rules;
pub mod sql_map;
pub mod statement;
pub mod tables;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::sync::Arc;
use std::time::Duration;

pub use derive::{Derived, ProfileReport, Totals, derive};
pub use extra::{Extra, ExtraValue, Quantity, Unit};
pub use metric::Metric;
pub use node::{
    Actuals, AggStrategy, CacheStats, ChildRole, CondKind, Condition, Estimates, HashStats, Io,
    JoinKind, Memory, NodeFlags, NodeId, ObjectKind, ObjectRef, OpKind, Parallel, ProfileNode,
    ScanDirection, Spill, SpillKind, Storage, Wal, WorkerStats, WriteKind,
};
pub use statement::{
    EngineCounters, Jit, PlanningMemory, Serialize, Setting, StatementProfile, StatementStats,
    TriggerTime, Wait,
};
pub use tables::{Access, IndexStats, TableScan, TableStats, TableUsage};

use crate::capabilities::ProfileCaps;
use crate::plan::PlanNode;
use crate::sql::{self, Flavor, TokenKind};
use crate::{Cancel, DbError, Session};

/// The model's format: written in every export and sent by the protocol. A field added is
/// optional (an older reader ignores it); a reader meeting a newer format keeps what it
/// understands, with [`ProfileNote::NewerFormat`].
pub const PROFILE_FORMAT: u16 = 1;

/// The most bytes of the engine's own answer kept for an export ([`Profile::raw`]).
pub const MAX_RAW_BYTES: usize = 8 << 20;

/// Past this many nodes, a profile is kept whole but its views start with the small
/// subtrees closed ([`ProfileNote::Truncated`]).
pub const LARGE_PLAN: usize = 50_000;

/// A profile's identity: a UUID v4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProfileId(u128);

impl ProfileId {
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }

    pub const fn from_u128(id: u128) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u128 {
        self.0
    }

    pub fn parse(text: &str) -> Option<Self> {
        kernel::id::parse_uuid(text).map(Self)
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&kernel::id::uuid_text(self.0))
    }
}

/// The engine that made it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Engine {
    /// The driver's kind (`postgres`, `sqlite`, an external one's id).
    pub driver: String,
    /// `PostgreSQL`, `SQLite`.
    pub name: String,
    /// As the server says it (`16.4`, `3.46.0`).
    pub version: Option<String>,
    /// As a number, where the engine has one (PostgreSQL's `server_version_num`).
    pub version_num: Option<u32>,
}

/// How deep it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileDepth {
    /// The plan, nothing run.
    Estimated,
    /// Run and measured; `timing: false`: real rows without per-node times (cheaper on
    /// queries of many rows, PostgreSQL's `TIMING off`).
    Actual { timing: bool },
}

impl ProfileDepth {
    pub fn runs(self) -> bool {
        matches!(self, ProfileDepth::Actual { .. })
    }
}

/// What a reader should know (keys of the locales, with their arguments).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileNote {
    /// The statement wrote: what it did was undone.
    RolledBack,
    /// `track_io_timing` is off: no I/O times.
    IoTimingOff,
    /// The times include what measuring them cost.
    InstrumentationOverhead,
    /// Planner settings differ from their defaults.
    PlannerSettingsChanged,
    /// Made by a newer driver: fields not understood were left out.
    NewerFormat { format: u16 },
    /// A large plan (its views start with small subtrees closed).
    Truncated { nodes: usize },
    /// The engine's answer was too large to keep for an export.
    RawTooLarge { bytes: usize },
    /// The server's history of the query was asked for, and the server doesn't keep one
    /// (PostgreSQL without `pg_stat_statements`).
    NoStatementStats,
}

impl ProfileNote {
    /// Stable (the exported JSON's): `rolled_back`, `newer_format`…
    pub fn id(self) -> &'static str {
        match self {
            ProfileNote::RolledBack => "rolled_back",
            ProfileNote::IoTimingOff => "io_timing_off",
            ProfileNote::InstrumentationOverhead => "instrumentation_overhead",
            ProfileNote::PlannerSettingsChanged => "planner_settings_changed",
            ProfileNote::NewerFormat { .. } => "newer_format",
            ProfileNote::Truncated { .. } => "truncated",
            ProfileNote::RawTooLarge { .. } => "raw_too_large",
            ProfileNote::NoStatementStats => "no_statement_stats",
        }
    }

    /// Its number argument, when it has one.
    pub fn argument(self) -> Option<u64> {
        match self {
            ProfileNote::NewerFormat { format } => Some(u64::from(format)),
            ProfileNote::Truncated { nodes } => Some(nodes as u64),
            ProfileNote::RawTooLarge { bytes } => Some(bytes as u64),
            _ => None,
        }
    }

    pub fn from_id(id: &str, argument: Option<u64>) -> Option<Self> {
        let n = argument.unwrap_or(0);
        Some(match id {
            "rolled_back" => ProfileNote::RolledBack,
            "io_timing_off" => ProfileNote::IoTimingOff,
            "instrumentation_overhead" => ProfileNote::InstrumentationOverhead,
            "planner_settings_changed" => ProfileNote::PlannerSettingsChanged,
            "newer_format" => {
                ProfileNote::NewerFormat { format: n.min(u64::from(u16::MAX)) as u16 }
            }
            "truncated" => ProfileNote::Truncated { nodes: n as usize },
            "raw_too_large" => ProfileNote::RawTooLarge { bytes: n as usize },
            "no_statement_stats" => ProfileNote::NoStatementStats,
            _ => return None,
        })
    }
}

/// The engine's own answer's form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawFormat {
    Json,
    Text,
    /// Rows (SQLite's `EXPLAIN QUERY PLAN`), as tab-separated text.
    Rows,
}

impl RawFormat {
    pub fn id(self) -> &'static str {
        match self {
            RawFormat::Json => "json",
            RawFormat::Text => "text",
            RawFormat::Rows => "rows",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Json, Self::Text, Self::Rows].into_iter().find(|f| f.id() == id)
    }
}

/// The engine's answer, kept for an export ("Copy the engine's JSON").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPlan {
    pub format: RawFormat,
    pub text: Arc<str>,
}

/// A statement's profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub id: ProfileId,
    pub format: u16,
    pub engine: Engine,
    pub depth: ProfileDepth,
    pub statement: StatementProfile,
    pub nodes: Vec<ProfileNode>,
    /// Several when the engine planned several statements (PostgreSQL's rewrite rules).
    pub roots: Vec<NodeId>,
    /// What this profiling asked for and got (not what the driver could).
    pub collected: ProfileCaps,
    pub notes: Vec<ProfileNote>,
    pub raw: Option<RawPlan>,
}

impl Profile {
    pub fn new(engine: Engine, depth: ProfileDepth) -> Self {
        Self {
            id: ProfileId::new(),
            format: PROFILE_FORMAT,
            engine,
            depth,
            statement: StatementProfile::default(),
            nodes: Vec::new(),
            roots: Vec::new(),
            collected: ProfileCaps::NONE,
            notes: Vec::new(),
            raw: None,
        }
    }

    pub fn node(&self, id: NodeId) -> &ProfileNode {
        &self.nodes[id.index()]
    }

    pub fn get(&self, id: NodeId) -> Option<&ProfileNode> {
        self.nodes.get(id.index())
    }

    /// Adds `node` under `parent` (`None`: a root), after its siblings: its id.
    pub fn push(&mut self, parent: Option<NodeId>, mut node: ProfileNode) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        node.parent = parent.filter(|p| p.index() < self.nodes.len());
        node.children.clear();
        match node.parent {
            Some(p) => self.nodes[p.index()].children.push(id),
            None => self.roots.push(id),
        }
        self.nodes.push(node);
        id
    }

    /// Every node depth first (a parent before its children, in their order), with its
    /// depth. A node reached twice (a broken list) is walked once.
    pub fn walk(&self) -> Vec<(NodeId, usize)> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut seen = vec![false; self.nodes.len()];
        let mut stack: Vec<(NodeId, usize)> = self.roots.iter().rev().map(|r| (*r, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            if id.index() >= self.nodes.len() || seen[id.index()] {
                continue;
            }
            seen[id.index()] = true;
            out.push((id, depth));
            for child in self.nodes[id.index()].children.iter().rev() {
                stack.push((*child, depth + 1));
            }
        }
        out
    }

    pub fn has_note(&self, note: ProfileNote) -> bool {
        self.notes.contains(&note)
    }

    /// Notes `note` once.
    pub fn note(&mut self, note: ProfileNote) {
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    /// Keeps the engine's answer, if it's at most `max_bytes` (else a note says so).
    pub fn keep_raw(&mut self, format: RawFormat, text: &str, max_bytes: usize) {
        if text.len() > max_bytes {
            self.raw = None;
            self.note(ProfileNote::RawTooLarge { bytes: text.len() });
        } else {
            self.raw = Some(RawPlan { format, text: Arc::from(text) });
        }
    }

    /// What a large plan's views must know ([`LARGE_PLAN`]).
    pub fn mark_size(&mut self) {
        if self.nodes.len() > LARGE_PLAN {
            self.note(ProfileNote::Truncated { nodes: self.nodes.len() });
        }
    }

    /// Every node's I/O added up into [`StatementProfile::io_total`] (none known: none).
    pub fn sum_io(&mut self) {
        let mut total: Option<Io> = None;
        for node in &self.nodes {
            if let Some(io) = &node.io {
                total = Some(match total {
                    Some(t) => t.add(io),
                    None => *io,
                });
            }
        }
        self.statement.io_total = total;
    }

    /// About how many bytes it holds (the console's memory, the Task Manager).
    pub fn memory(&self) -> usize {
        fn extra(list: &[Extra]) -> usize {
            list.iter()
                .map(|e| {
                    64 + e.key.len()
                        + e.label.as_ref().map_or(0, String::len)
                        + e.group.as_ref().map_or(0, String::len)
                        + match &e.value {
                            ExtraValue::Text(t) => t.len(),
                            ExtraValue::List(items) => items.len() * 32,
                            ExtraValue::Group(items) => extra(items),
                            _ => 0,
                        }
                })
                .sum()
        }
        let nodes: usize = self
            .nodes
            .iter()
            .map(|n| {
                std::mem::size_of::<ProfileNode>()
                    + n.name.len()
                    + n.children.len() * 4
                    + n.object.as_ref().map_or(0, |o| o.name.len() + 32)
                    + n.conditions.iter().map(|c| c.text.len() + 32).sum::<usize>()
                    + n.output.iter().map(String::len).sum::<usize>()
                    + n.parallel.as_ref().map_or(0, |p| p.per_worker.len() * 160)
                    + extra(&n.extra)
            })
            .sum();
        std::mem::size_of::<Profile>()
            + nodes
            + extra(&self.statement.extra)
            + self.raw.as_ref().map_or(0, |r| r.text.len())
    }

    /// A profile from a plan (a driver that only explains: protocol 2, the toy driver):
    /// each step's words, its kind guessed from them, its object, its detail as one
    /// condition, its costs, rows and times.
    pub fn from_plan(roots: &[PlanNode], depth: ProfileDepth, engine: Engine) -> Self {
        fn add(profile: &mut Profile, parent: Option<NodeId>, plan: &PlanNode) {
            let mut node = ProfileNode::new(OpKind::guess(&plan.operation), plan.operation.clone());
            node.object = plan.object.clone().map(|o| ObjectRef::new(ObjectKind::Other, o));
            if let Some(detail) = plan.detail.as_ref().filter(|d| !d.is_empty()) {
                node.conditions
                    .push(Condition::new(CondKind::Other(String::new()), detail.clone()));
            }
            node.estimates = Estimates {
                startup_cost: plan.startup_cost,
                total_cost: plan.total_cost,
                rows: plan.plan_rows,
                width: None,
            };
            node.actuals.rows = plan.actual_rows;
            node.actuals.total_ms = plan.actual_time_ms;
            node.actuals.loops = plan.loops;
            node.actuals.never_executed = plan.loops == Some(0.0);
            let id = profile.push(parent, node);
            for child in &plan.children {
                add(profile, Some(id), child);
            }
        }
        let mut profile = Profile::new(engine, depth);
        for root in roots {
            add(&mut profile, None, root);
        }
        let flat = crate::plan::flatten(roots);
        profile.collected = ProfileCaps {
            estimated: true,
            actual: depth.runs(),
            node_timing: flat.iter().any(|f| f.node.actual_time_ms.is_some()),
            ..ProfileCaps::NONE
        };
        profile.mark_size();
        profile
    }

    /// The plan this profile is (the Output, the Plan tab, the protocol's version 2): each
    /// step's words (with its join's kind when the words don't say it), object, conditions
    /// (those its words don't already say), costs, rows, time and loops.
    pub fn plan_nodes(&self) -> Vec<PlanNode> {
        fn build(profile: &Profile, id: NodeId, seen: &mut [bool]) -> PlanNode {
            seen[id.index()] = true;
            let n = profile.node(id);
            let mut operation = n.name.clone();
            if let Some(join) = n.join.filter(|j| *j != JoinKind::Inner)
                && operation.contains("Join")
                && !operation.contains(&format!("({})", join.words()))
            {
                operation = format!("{operation} ({})", join.words());
            }
            let mut details = Vec::new();
            if let (Some(index), Some(ObjectRef { kind: ObjectKind::Table, .. })) =
                (&n.index, &n.object)
                && !export::has_word(&n.name, index)
            {
                details.push(format!("using {index}"));
            }
            for c in &n.conditions {
                if c.text.is_empty() || n.name.contains(c.text.as_str()) {
                    continue;
                }
                let label = c.kind.label();
                if label.is_empty() {
                    details.push(c.text.clone());
                } else {
                    details.push(format!("{label}: {}", c.text));
                }
            }
            let mut children = Vec::new();
            for c in &n.children {
                if c.index() < seen.len() && !seen[c.index()] {
                    children.push(build(profile, *c, seen));
                }
            }
            PlanNode {
                operation,
                // A CTE's own plan (SQLite's `CO-ROUTINE x`) works on nothing but itself.
                object: n
                    .object
                    .as_ref()
                    .filter(|o| !(o.kind == ObjectKind::Cte && n.role == Some(ChildRole::Cte)))
                    .map(ObjectRef::label),
                detail: (!details.is_empty()).then(|| details.join("; ")),
                startup_cost: n.estimates.startup_cost,
                total_cost: n.estimates.total_cost,
                plan_rows: n.estimates.rows,
                actual_rows: n.actuals.rows,
                actual_time_ms: n.actuals.total_ms,
                loops: n.actuals.loops,
                children,
            }
        }
        let mut seen = vec![false; self.nodes.len()];
        let roots: Vec<NodeId> =
            self.roots.iter().filter(|r| r.index() < self.nodes.len()).copied().collect();
        roots.into_iter().fold(Vec::new(), |mut out, r| {
            if !seen[r.index()] {
                out.push(build(self, r, &mut seen));
            }
            out
        })
    }
}

/// What a profiling asks for (the driver ignores what it can't do, and says in
/// [`Profile::collected`] what it did).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileOptions {
    pub depth: ProfileDepth,
    /// Blocks read and found, per node.
    pub buffers: bool,
    /// The columns each step gives (and PostgreSQL's query identifier).
    pub verbose: bool,
    /// The planner's settings that differ.
    pub settings: bool,
    /// The WAL a write wrote.
    pub wal: bool,
    /// The time to turn the rows into what's sent (PostgreSQL 17's `SERIALIZE`).
    pub serialize: bool,
    /// The server's history of the same query.
    pub statement_history: bool,
    /// The statement stopped past this (a profile run): the engine's own timeout.
    pub timeout: Option<Duration>,
    pub max_raw_bytes: usize,
}

impl ProfileOptions {
    /// The plan only.
    pub const ESTIMATED: Self = Self {
        depth: ProfileDepth::Estimated,
        buffers: true,
        verbose: false,
        settings: true,
        wal: true,
        serialize: false,
        statement_history: false,
        timeout: None,
        max_raw_bytes: MAX_RAW_BYTES,
    };

    /// Run, with everything cheap measured.
    pub const ACTUAL: Self =
        Self { depth: ProfileDepth::Actual { timing: true }, ..Self::ESTIMATED };

    pub fn of(depth: ProfileDepth) -> Self {
        Self { depth, ..Self::ESTIMATED }
    }
}

impl Default for ProfileOptions {
    fn default() -> Self {
        Self::ESTIMATED
    }
}

/// What a session without a profile of its own gives ([`Session::profile`]'s default):
/// its plan ([`Session::explain`], as it runs when asked) as a profile.
pub fn by_explain<S: Session + ?Sized>(
    session: &mut S,
    statement: &str,
    options: &ProfileOptions,
    cancel: &Cancel,
) -> Result<Profile, DbError> {
    let plan = session.explain(statement, options.depth.runs(), cancel)?;
    let engine = Engine { name: session.server(), ..Engine::default() };
    Ok(Profile::from_plan(&plan, options.depth, engine))
}

/// Why `statement` can't be profiled as it runs (it can't run in the transaction that
/// undoes it): its words, `VACUUM`, `CREATE INDEX CONCURRENTLY`, `CREATE DATABASE`,
/// `ALTER SYSTEM`, a transaction's own control. `None`: it can.
pub fn actual_refused(statement: &str, flavor: Flavor) -> Option<String> {
    let tokens = sql::lex(statement, flavor);
    let words: Vec<String> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Word)
        .take(4)
        .map(|t| t.text(statement).to_ascii_uppercase())
        .collect();
    let at = |i: usize| words.get(i).map_or("", String::as_str);
    match (at(0), at(1), at(2)) {
        ("VACUUM", ..) => Some("VACUUM".to_owned()),
        ("CREATE" | "DROP", "DATABASE" | "TABLESPACE", _) => Some(format!("{} {}", at(0), at(1))),
        ("CREATE", "INDEX", "CONCURRENTLY") => Some("CREATE INDEX CONCURRENTLY".to_owned()),
        ("CREATE", "UNIQUE", "INDEX") if at(3) == "CONCURRENTLY" => {
            Some("CREATE UNIQUE INDEX CONCURRENTLY".to_owned())
        }
        ("DROP", "INDEX", "CONCURRENTLY") => Some("DROP INDEX CONCURRENTLY".to_owned()),
        ("REINDEX", ..) if words.iter().any(|w| w == "CONCURRENTLY") => {
            Some("REINDEX CONCURRENTLY".to_owned())
        }
        ("ALTER", "SYSTEM", _) => Some("ALTER SYSTEM".to_owned()),
        (
            "BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "ABORT",
            ..,
        ) => Some(at(0).to_owned()),
        ("ATTACH" | "DETACH", ..) if matches!(flavor, Flavor::Sqlite) => Some(at(0).to_owned()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile.rs"]
mod tests;
