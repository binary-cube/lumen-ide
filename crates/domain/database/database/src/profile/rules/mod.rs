//! The analysis of a profile (plan 39h3): pure rules on the normalized model, each in its own
//! file with what evidence it needs, its threshold and why. A rule never runs anything: it
//! reads the profile and what's worked out of it ([`ProfileReport`]), what's known of the
//! tables ([`Facts`]: their indexes, foreign keys — the engine's and the virtual ones accepted
//! — and statistics), the statement's text (its tokens, its clauses, where a node's table or
//! condition is: [`SqlMap`]), and the console's recent runs ([`RecentRuns`]). What it finds
//! ([`Finding`]) says how bad it is, how much of the time (or cost) it touches, on which
//! nodes, the figures it rests on, what could be done ([`Suggestion`]: SQL written through the
//! dialect, never run) and where it is in the statement.
//!
//! The domain has no UI text: a finding's words are the locales' `profile.finding.<id>.title`,
//! `.why` and `.fix` (or a variant's `why_<variant>` / `fix_<variant>`), its evidence their
//! arguments by [`EvidenceKey::id`].
//!
//! [`analyze`] runs every rule that applies: the hot spot first (where to start), then by
//! severity and impact.

pub mod facts;
pub mod suggest;

mod auto_index;
mod big_offset;
mod cold_cache;
mod dead_tuples;
mod disabled_node;
mod filter_removes_most;
mod fk_join_no_index;
mod function_on_column;
mod heap_fetches;
mod hot_spot;
mod implicit_cast;
mod jit_dominates;
mod leading_wildcard;
mod limit_without_order;
mod lossy_bitmap;
mod misestimate;
mod n_plus_one;
mod nested_loop_many;
mod no_statement_stats;
mod or_defeats_index;
mod planning_dominates;
mod select_star_wide;
mod seq_scan;
mod spill_to_disk;
mod sqlite_scan;
mod sqlite_temp_btree;
mod stale_stats;
mod trigger_time;
mod workers_not_launched;

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

pub use facts::{
    Facts, KeyFact, MAX_TABLES, RecentRuns, TableFacts, node_table, same_table, tables_of,
};
pub use suggest::{IndexSpec, RewriteHint, Suggestion};

use crate::TableRef;
use crate::console::PlannedStatement;
use crate::dialect::Dialect;
use crate::plan::Measure;
use crate::profile::cond::{self, ParsedCondition, Predicate};
use crate::profile::derive::{Derived, ProfileReport};
use crate::profile::extra::{Quantity, Unit};
use crate::profile::node::{CondKind, NodeId, ObjectKind, ObjectRef, ProfileNode};
use crate::profile::sql_map::SqlMap;
use crate::profile::{Profile, export};
use crate::sql::Flavor;

/// How bad a finding is (ordered: the worst last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn id(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

/// What a figure of a finding is (its argument's name in the locales' texts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceKey {
    /// A node's words (`Seq Scan on orders o`).
    Step,
    Table,
    Column,
    Columns,
    Index,
    Function,
    /// A cast's type.
    Type,
    /// The share of the statement's time (or cost).
    Share,
    Time,
    Cost,
    /// Rows read (kept and removed).
    Read,
    Kept,
    KeptShare,
    Removed,
    RemovedShare,
    Estimated,
    Actual,
    /// Real over estimated rows (`120`: ×120 more, `0.01`: a hundred times fewer).
    Factor,
    Loops,
    /// What spilled (`sort`, `hash`…).
    Spill,
    Disk,
    Batches,
    Planned,
    Launched,
    Jit,
    Execution,
    Planning,
    HeapFetches,
    Lossy,
    Dead,
    Live,
    DeadShare,
    Modified,
    ModifiedShare,
    /// Shared blocks read from disk, and all those touched.
    BlocksRead,
    BlocksTouched,
    ReadShare,
    Width,
    ColumnCount,
    Offset,
    Runs,
    Window,
    Calls,
    Mean,
    Trigger,
    /// What a temporary B-tree is for (`ORDER BY`).
    Purpose,
    Setting,
    Value,
    /// The fullscan steps of SQLite's counters, and the rows returned.
    Fullscan,
    Returned,
    /// The table and column a foreign key points to.
    Target,
}

impl EvidenceKey {
    /// Every key.
    pub const ALL: [EvidenceKey; 51] = [
        EvidenceKey::Step,
        EvidenceKey::Table,
        EvidenceKey::Column,
        EvidenceKey::Columns,
        EvidenceKey::Index,
        EvidenceKey::Function,
        EvidenceKey::Type,
        EvidenceKey::Share,
        EvidenceKey::Time,
        EvidenceKey::Cost,
        EvidenceKey::Read,
        EvidenceKey::Kept,
        EvidenceKey::KeptShare,
        EvidenceKey::Removed,
        EvidenceKey::RemovedShare,
        EvidenceKey::Estimated,
        EvidenceKey::Actual,
        EvidenceKey::Factor,
        EvidenceKey::Loops,
        EvidenceKey::Spill,
        EvidenceKey::Disk,
        EvidenceKey::Batches,
        EvidenceKey::Planned,
        EvidenceKey::Launched,
        EvidenceKey::Jit,
        EvidenceKey::Execution,
        EvidenceKey::Planning,
        EvidenceKey::HeapFetches,
        EvidenceKey::Lossy,
        EvidenceKey::Dead,
        EvidenceKey::Live,
        EvidenceKey::DeadShare,
        EvidenceKey::Modified,
        EvidenceKey::ModifiedShare,
        EvidenceKey::BlocksRead,
        EvidenceKey::BlocksTouched,
        EvidenceKey::ReadShare,
        EvidenceKey::Width,
        EvidenceKey::ColumnCount,
        EvidenceKey::Offset,
        EvidenceKey::Runs,
        EvidenceKey::Window,
        EvidenceKey::Calls,
        EvidenceKey::Mean,
        EvidenceKey::Trigger,
        EvidenceKey::Purpose,
        EvidenceKey::Setting,
        EvidenceKey::Value,
        EvidenceKey::Fullscan,
        EvidenceKey::Returned,
        EvidenceKey::Target,
    ];

    /// Its argument's name in the texts (`step`, `kept_share`…).
    pub fn id(self) -> &'static str {
        use EvidenceKey as E;
        match self {
            E::Step => "step",
            E::Table => "table",
            E::Column => "column",
            E::Columns => "columns",
            E::Index => "index",
            E::Function => "function",
            E::Type => "type",
            E::Share => "share",
            E::Time => "time",
            E::Cost => "cost",
            E::Read => "read",
            E::Kept => "kept",
            E::KeptShare => "kept_share",
            E::Removed => "removed",
            E::RemovedShare => "removed_share",
            E::Estimated => "estimated",
            E::Actual => "actual",
            E::Factor => "factor",
            E::Loops => "loops",
            E::Spill => "spill",
            E::Disk => "disk",
            E::Batches => "batches",
            E::Planned => "planned",
            E::Launched => "launched",
            E::Jit => "jit",
            E::Execution => "execution",
            E::Planning => "planning",
            E::HeapFetches => "heap_fetches",
            E::Lossy => "lossy",
            E::Dead => "dead",
            E::Live => "live",
            E::DeadShare => "dead_share",
            E::Modified => "modified",
            E::ModifiedShare => "modified_share",
            E::BlocksRead => "blocks_read",
            E::BlocksTouched => "blocks_touched",
            E::ReadShare => "read_share",
            E::Width => "width",
            E::ColumnCount => "column_count",
            E::Offset => "offset",
            E::Runs => "runs",
            E::Window => "window",
            E::Calls => "calls",
            E::Mean => "mean",
            E::Trigger => "trigger",
            E::Purpose => "purpose",
            E::Setting => "setting",
            E::Value => "value",
            E::Fullscan => "fullscan",
            E::Returned => "returned",
            E::Target => "target",
        }
    }
}

/// A figure of a finding: a number with its unit, or a name (a table, a column, a step).
#[derive(Debug, Clone, PartialEq)]
pub enum Evidence {
    Quantity(Quantity),
    Text(String),
}

/// What a rule found.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// The rule's id (`seq-scan-selective`).
    pub rule: &'static str,
    pub severity: Severity,
    /// The share of the time (or cost) it touches, 0–1: what findings are sorted by.
    pub impact: f64,
    pub nodes: Vec<NodeId>,
    pub evidence: Vec<(EvidenceKey, Evidence)>,
    /// Which of the rule's texts say it (`None`: `why` / `fix`; `Some(v)`: `why_v` / `fix_v`).
    pub variant: Option<&'static str>,
    pub suggestion: Option<Suggestion>,
    /// Its place in the document (the statement as written: taken back from the text sent).
    pub sql_range: Option<Range<usize>>,
}

impl Finding {
    pub fn new(rule: &'static str, severity: Severity) -> Self {
        Self {
            rule,
            severity,
            impact: 0.0,
            nodes: Vec::new(),
            evidence: Vec::new(),
            variant: None,
            suggestion: None,
            sql_range: None,
        }
    }

    pub fn node(mut self, id: NodeId) -> Self {
        if !self.nodes.contains(&id) {
            self.nodes.push(id);
        }
        self
    }

    pub fn impact(mut self, impact: f64) -> Self {
        self.impact = if impact.is_finite() { impact.clamp(0.0, 1.0) } else { 0.0 };
        self
    }

    /// A figure.
    pub fn with(mut self, key: EvidenceKey, value: f64, unit: Unit) -> Self {
        self.evidence.push((key, Evidence::Quantity(Quantity::new(value, unit))));
        self
    }

    /// A name.
    pub fn text(mut self, key: EvidenceKey, text: impl Into<String>) -> Self {
        self.evidence.push((key, Evidence::Text(text.into())));
        self
    }

    pub fn variant(mut self, variant: &'static str) -> Self {
        self.variant = Some(variant);
        self
    }

    pub fn suggest(mut self, suggestion: Option<Suggestion>) -> Self {
        self.suggestion = suggestion;
        self
    }

    pub fn at(mut self, range: Option<Range<usize>>) -> Self {
        self.sql_range = range;
        self
    }

    /// Its figure `key`.
    pub fn quantity(&self, key: EvidenceKey) -> Option<Quantity> {
        self.evidence.iter().find_map(|(k, e)| match e {
            Evidence::Quantity(q) if *k == key => Some(*q),
            _ => None,
        })
    }

    /// Its name `key`.
    pub fn text_of(&self, key: EvidenceKey) -> Option<&str> {
        self.evidence.iter().find_map(|(k, e)| match e {
            Evidence::Text(t) if *k == key => Some(t.as_str()),
            _ => None,
        })
    }

    /// The locales' key of its `part` (`title`, `why`, `fix`), its variant's where it has
    /// one (the title is the rule's).
    pub fn text_key(&self, part: &str) -> String {
        match self.variant.filter(|_| part != "title") {
            Some(v) => format!("profile.finding.{}.{part}_{v}", self.rule),
            None => format!("profile.finding.{}.{part}", self.rule),
        }
    }

    /// It's a problem (not where to start).
    pub fn is_problem(&self) -> bool {
        self.rule != hot_spot::ID
    }
}

/// A rule of the analysis.
pub trait Rule: Send + Sync {
    /// Stable and technical (`seq-scan-selective`): its texts' key and what "ignore" keeps.
    fn id(&self) -> &'static str;
    /// It has anything to look at in `report` (cheap).
    fn applies(&self, report: &ProfileReport) -> bool;
    fn check(&self, cx: &RuleContext<'_>) -> Vec<Finding>;
    /// The variants of its texts (each has `why_<v>` and `fix_<v>`).
    fn variants(&self) -> &'static [&'static str] {
        &[]
    }
}

/// The thresholds of the rules (named constants, not settings, in this first version).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// `seq-scan-selective`: rows read, and the most kept.
    pub seq_scan_rows: f64,
    pub seq_scan_kept: f64,
    /// A share of the time past which a warning is critical.
    pub critical_share: f64,
    /// `misestimate`: the factor (either way) and the rows on one side.
    pub misestimate_factor: f64,
    pub misestimate_rows: f64,
    /// `nested-loop-many`: the inner side's loops, and its share of the time.
    pub nested_loops: f64,
    pub nested_share: f64,
    /// `filter-removes-most`: the share removed and the rows removed.
    pub filter_removed_share: f64,
    pub filter_removed_rows: f64,
    /// `sqlite-scan`: fullscan steps over rows returned that make it worse.
    pub sqlite_fullscan_factor: f64,
    /// `sqlite-temp-btree`: rows sorted (info, then warning).
    pub temp_btree_rows: f64,
    pub temp_btree_warning_rows: f64,
    /// `select-star-wide`: a row's bytes or its columns.
    pub wide_row_bytes: f64,
    pub wide_columns: usize,
    pub big_offset: u64,
    /// `n-plus-one`: runs of a fingerprint in a window; the server's calls with a small mean.
    pub repeated_runs: usize,
    pub repeated_window: Duration,
    pub repeated_calls: u64,
    pub repeated_mean_ms: f64,
    /// `jit-dominates`: its share of the execution and its least time.
    pub jit_share: f64,
    pub jit_ms: f64,
    /// `planning-dominates`: its least time.
    pub planning_ms: f64,
    /// `heap-fetches`: the share of the rows fetched from the table.
    pub heap_fetches_share: f64,
    /// `dead-tuples`: dead rows over live ones (info, then warning).
    pub dead_share: f64,
    pub dead_warning_share: f64,
    /// `stale-stats`: rows changed since the last `ANALYZE` over live ones.
    pub modified_share: f64,
    /// `cold-cache`: blocks read from disk over those touched, and the least touched.
    pub cold_read_share: f64,
    pub cold_blocks: u64,
    /// A cost this high marks a step the planner was told not to use (before 18).
    pub disabled_cost: f64,
    /// `trigger-time`: the triggers' share of a write's time.
    pub trigger_share: f64,
    /// Tables smaller than this are never worth an index.
    pub small_table_rows: f64,
}

impl Thresholds {
    pub const DEFAULT: Self = Self {
        seq_scan_rows: 10_000.0,
        seq_scan_kept: 0.10,
        critical_share: 0.30,
        misestimate_factor: 10.0,
        misestimate_rows: 100.0,
        nested_loops: 1_000.0,
        nested_share: 0.20,
        filter_removed_share: 0.90,
        filter_removed_rows: 1_000.0,
        sqlite_fullscan_factor: 10.0,
        temp_btree_rows: 1_000.0,
        temp_btree_warning_rows: 100_000.0,
        wide_row_bytes: 500.0,
        wide_columns: 20,
        big_offset: 10_000,
        repeated_runs: 20,
        repeated_window: Duration::from_secs(60),
        repeated_calls: 1_000,
        repeated_mean_ms: 1.0,
        jit_share: 0.30,
        jit_ms: 50.0,
        planning_ms: 10.0,
        heap_fetches_share: 0.10,
        dead_share: 0.20,
        dead_warning_share: 0.50,
        modified_share: 0.20,
        cold_read_share: 0.90,
        cold_blocks: 1_000,
        disabled_cost: 1e10,
        trigger_share: 0.30,
        small_table_rows: 1_000.0,
    };
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the rules read, with what's worked out once for all of them (each node's conditions
/// read, the statement's map).
pub struct RuleContext<'a> {
    pub report: &'a ProfileReport,
    pub facts: &'a Facts,
    /// The statement profiled (its text sent is what the map reads; its map back to the
    /// document places a finding).
    pub statement: &'a PlannedStatement,
    pub flavor: Flavor,
    pub recent: &'a RecentRuns,
    pub thresholds: &'a Thresholds,
    /// The source is marked as production (an index made `CONCURRENTLY`).
    pub production: bool,
    pub map: SqlMap,
    pub dialect: Arc<dyn Dialect>,
    /// Each node's conditions, read.
    conditions: Vec<Vec<(CondKind, ParsedCondition)>>,
    /// The top level's `WHERE`, read (what SQLite's plan doesn't say).
    pub where_condition: Option<ParsedCondition>,
    /// The pairs of columns the statement's `WHERE`s and `ON`s compare (its joins).
    pub statement_joins: Vec<cond::JoinPair>,
}

impl std::fmt::Debug for RuleContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleContext")
            .field("flavor", &self.flavor)
            .field("production", &self.production)
            .finish_non_exhaustive()
    }
}

impl<'a> RuleContext<'a> {
    pub fn new(
        report: &'a ProfileReport,
        facts: &'a Facts,
        statement: &'a PlannedStatement,
        flavor: Flavor,
        recent: &'a RecentRuns,
        thresholds: &'a Thresholds,
        production: bool,
    ) -> Self {
        let map = SqlMap::new(statement.sent(), flavor);
        let conditions = report
            .profile
            .nodes
            .iter()
            .map(|n| {
                n.conditions
                    .iter()
                    .map(|c| (c.kind.clone(), cond::parse(&c.text, flavor)))
                    .collect()
            })
            .collect();
        use crate::profile::sql_map::ClauseKind;
        let where_condition =
            map.top(ClauseKind::Where).map(|c| cond::parse(&map.text()[c.range.clone()], flavor));
        let statement_joins = map
            .clauses()
            .iter()
            .filter(|c| matches!(c.kind, ClauseKind::Where | ClauseKind::On))
            .flat_map(|c| cond::parse(&map.text()[c.range.clone()], flavor).joins)
            .collect();
        Self {
            report,
            facts,
            statement,
            flavor,
            recent,
            thresholds,
            production,
            map,
            dialect: crate::console::dialect_for(flavor),
            conditions,
            where_condition,
            statement_joins,
        }
    }

    pub fn profile(&self) -> &Profile {
        &self.report.profile
    }

    pub fn node(&self, id: NodeId) -> &ProfileNode {
        self.report.profile.node(id)
    }

    pub fn derived(&self, id: NodeId) -> &Derived {
        self.report.derived(id)
    }

    /// The nodes depth first.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &ProfileNode)> {
        self.report.order.iter().map(|id| (*id, self.node(*id)))
    }

    pub fn is_postgres(&self) -> bool {
        self.flavor.is_postgres() || self.profile().engine.driver == "postgres"
    }

    pub fn is_sqlite(&self) -> bool {
        self.flavor == Flavor::Sqlite || self.profile().engine.driver == "sqlite"
    }

    /// PostgreSQL's version (`server_version_num`), when known.
    pub fn version(&self) -> Option<u32> {
        match self.flavor {
            Flavor::Postgres { version } => version.or(self.profile().engine.version_num),
            _ => self.profile().engine.version_num,
        }
    }

    /// The node's table object (a table, not a CTE or a subquery).
    pub fn object(&self, id: NodeId) -> Option<&ObjectRef> {
        self.node(id).object.as_ref().filter(|o| o.kind == ObjectKind::Table)
    }

    /// The node's table, as the statement names it.
    pub fn table_of(&self, id: NodeId) -> Option<TableRef> {
        let table = node_table(self.node(id), &self.map)?;
        // The facts' name (its schema) when they know it.
        Some(self.facts.table(&table).map_or(table, |f| f.table.clone()))
    }

    pub fn table_facts(&self, id: NodeId) -> Option<&TableFacts> {
        self.facts.table(&self.table_of(id)?)
    }

    /// What the node's columns may be qualified by: its alias, its name, and the statement's
    /// aliases of its table.
    pub fn qualifiers(&self, id: NodeId) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(o) = self.object(id) {
            out.push(o.name.clone());
            if let Some(a) = &o.alias {
                out.push(a.clone());
            }
        }
        if let Some(t) = self.table_of(id) {
            for occ in self.map.tables().iter().filter(|o| same_table(&o.table, &t)) {
                if let Some(a) = &occ.alias {
                    out.push(a.clone());
                }
                out.push(occ.table.name.clone());
            }
        }
        out.dedup();
        out
    }

    /// The node's conditions of `kinds`, read.
    pub fn conditions(
        &self,
        id: NodeId,
        kinds: &[CondKind],
    ) -> impl Iterator<Item = &ParsedCondition> {
        self.conditions[id.index()].iter().filter(move |(k, _)| kinds.contains(k)).map(|(_, c)| c)
    }

    /// The predicates on the node's own columns that decide its rows (in its conditions of
    /// `kinds`; for a node whose engine doesn't say them, SQLite's, the top `WHERE`'s on its
    /// table).
    pub fn predicates(&self, id: NodeId, kinds: &[CondKind]) -> Vec<&Predicate> {
        let Some(object) = self.node(id).object.as_ref() else { return Vec::new() };
        let mine = self.object_with_aliases(id, object);
        self.conditions(id, kinds)
            .flat_map(|c| c.predicates.iter())
            .filter(|p| !p.in_or && mine.iter().any(|o| p.column.of_object(o)))
            .collect()
    }

    /// The top `WHERE`'s predicates on the node's table (unqualified ones only when the
    /// statement names one table, or the table's structure has the column).
    pub fn where_predicates(&self, id: NodeId) -> Vec<&Predicate> {
        let Some(w) = &self.where_condition else { return Vec::new() };
        let qualifiers = self.qualifiers(id);
        let facts = self.table_facts(id);
        let single = self.map.tables().len() == 1;
        w.predicates
            .iter()
            .filter(|p| !p.in_or)
            .filter(|p| match &p.column.qualifier {
                Some(q) => qualifiers.iter().any(|x| x.eq_ignore_ascii_case(q)),
                None => single || facts.and_then(|f| f.has_column(&p.column.name)) == Some(true),
            })
            .collect()
    }

    fn object_with_aliases(&self, id: NodeId, object: &ObjectRef) -> Vec<ObjectRef> {
        let mut out = vec![object.clone()];
        for q in self.qualifiers(id) {
            out.push(ObjectRef { alias: Some(q), ..object.clone() });
        }
        out
    }

    /// `p` compares its column as stored: neither wrapped nor cast — or cast to a text type
    /// from a text column (PostgreSQL writes `(varchar_column)::text`, and an index on the
    /// column serves it).
    pub fn plain(&self, id: NodeId, p: &Predicate) -> bool {
        if p.wrapper.is_some() {
            return false;
        }
        let Some(ty) = &p.cast else { return true };
        if !texty(ty) {
            return false;
        }
        let column = self
            .table_facts(id)
            .and_then(|f| f.structure.as_ref())
            .and_then(|s| s.columns.iter().find(|c| p.column.is(&c.name)));
        column.is_none_or(|c| c.class == crate::TypeClass::Text)
    }

    /// The columns an index for the node's `predicates` would have: the equalities first
    /// (each once), then one range — columns compared as stored only ([`Self::plain`]).
    pub fn index_columns(&self, id: NodeId, predicates: &[&Predicate]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let add = |name: &str, out: &mut Vec<String>| {
            if !out.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                out.push(name.to_owned());
            }
        };
        for p in predicates.iter().filter(|p| self.plain(id, p) && p.compare.is_equality()) {
            add(&p.column.name, &mut out);
        }
        if let Some(p) =
            predicates.iter().find(|p| self.plain(id, p) && p.compare == cond::Compare::Range)
        {
            add(&p.column.name, &mut out);
        }
        out
    }

    /// The node's own share of the time (else of the cost), 0–1.
    pub fn self_share(&self, id: NodeId) -> f64 {
        let d = self.derived(id);
        d.self_pct.or(d.cost_pct).unwrap_or(0.0)
    }

    /// The node's share of the time (else the cost) with its children, 0–1.
    pub fn inclusive_share(&self, id: NodeId) -> f64 {
        let t = &self.report.totals;
        let d = self.derived(id);
        match t.measure {
            Some(Measure::Time) => match (d.inclusive_ms, t.time_ms) {
                (Some(own), Some(all)) if all > 0.0 => (own / all).min(1.0),
                _ => 0.0,
            },
            Some(Measure::Cost) => match (self.node(id).estimates.total_cost, t.cost) {
                (Some(own), Some(all)) if all > 0.0 => (own / all).min(1.0),
                _ => 0.0,
            },
            None => 0.0,
        }
    }

    /// A node's words, as the findings name it.
    pub fn step(&self, id: NodeId) -> String {
        export::head(self.node(id))
    }

    /// `range` of the text sent, in the document.
    pub fn to_document(&self, range: Range<usize>) -> Range<usize> {
        let map = self.statement.map();
        map.to_document(range.start)..map.to_document(range.end)
    }

    /// Where the node's table is in the document (named once).
    pub fn node_range(&self, id: NodeId) -> Option<Range<usize>> {
        let object = self.node(id).object.as_ref()?;
        let range = self.map.table_range(object).or_else(|| {
            // SQLite names a table by its alias: the occurrence answering to it.
            let found = self.map.resolve(&object.name)?;
            Some(found.range.clone())
        })?;
        Some(self.to_document(range))
    }

    /// Where the node's `column` is compared in the document (once), else its table.
    pub fn column_range(&self, id: NodeId, column: &str) -> Option<Range<usize>> {
        let qualifiers = self.qualifiers(id);
        let refs: Vec<&str> = qualifiers.iter().map(String::as_str).collect();
        self.map
            .predicate_range(column, &refs)
            .map(|r| self.to_document(r))
            .or_else(|| self.node_range(id))
    }

    /// An index suggested on `table` — only when its indexes are known and none already
    /// starts with these columns (an index that may exist is never guessed; a partial one,
    /// on an expression or of another method is another index).
    pub fn create_index(&self, table: &TableRef, spec: IndexSpec) -> Option<Suggestion> {
        let facts = self.facts.table(table).filter(|f| f.knows_indexes())?;
        if spec.expression.is_none()
            && spec.method.is_none()
            && spec.where_clause.is_none()
            && facts.index_starting_with(&spec.columns).is_some()
        {
            return None;
        }
        suggest::create_index(&*self.dialect, table, &spec, self.production && self.is_postgres())
    }

    pub fn analyze_sql(&self, table: &TableRef) -> Option<Suggestion> {
        suggest::analyze(&*self.dialect, table)
    }

    pub fn vacuum_sql(&self, table: &TableRef) -> Option<Suggestion> {
        suggest::vacuum(&*self.dialect, table)
    }

    /// A setting's value in the profile's planner settings (PostgreSQL's `SETTINGS`).
    pub fn setting(&self, name: &str) -> Option<&str> {
        self.profile()
            .statement
            .settings
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .map(|s| s.value.trim_matches('\'').trim())
    }

    /// The node's ancestors, nearest first.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut at = self.node(id).parent;
        std::iter::from_fn(move || {
            let here = at?;
            at = self.profile().get(here).and_then(|n| n.parent).filter(|p| *p != here);
            Some(here)
        })
        .take(self.profile().nodes.len())
    }
}

/// A text type's name (`text`, `varchar`, `character varying(20)`, `bpchar`, `name`).
pub(crate) fn texty(ty: &str) -> bool {
    let t = ty.trim().to_ascii_lowercase();
    ["text", "varchar", "character varying", "character", "char", "bpchar", "name", "citext"]
        .iter()
        .any(|w| t == *w || t.starts_with(&format!("{w}(")) || t.starts_with(&format!("{w}[")))
}

/// Every rule, in a fixed order.
pub fn rules() -> &'static [&'static dyn Rule] {
    &[
        &hot_spot::HotSpot,
        &seq_scan::SeqScanSelective,
        &misestimate::Misestimate,
        &nested_loop_many::NestedLoopMany,
        &spill_to_disk::SpillToDisk,
        &filter_removes_most::FilterRemovesMost,
        &fk_join_no_index::FkJoinNoIndex,
        &sqlite_scan::SqliteScan,
        &sqlite_temp_btree::SqliteTempBTree,
        &auto_index::AutoIndex,
        &function_on_column::FunctionOnColumn,
        &implicit_cast::ImplicitCast,
        &or_defeats_index::OrDefeatsIndex,
        &leading_wildcard::LeadingWildcard,
        &select_star_wide::SelectStarWide,
        &limit_without_order::LimitWithoutOrder,
        &big_offset::BigOffset,
        &n_plus_one::NPlusOne,
        &workers_not_launched::WorkersNotLaunched,
        &jit_dominates::JitDominates,
        &planning_dominates::PlanningDominates,
        &heap_fetches::HeapFetches,
        &lossy_bitmap::LossyBitmap,
        &dead_tuples::DeadTuples,
        &stale_stats::StaleStats,
        &cold_cache::ColdCache,
        &disabled_node::DisabledNode,
        &trigger_time::TriggerTime,
        &no_statement_stats::NoStatementStats,
    ]
}

/// Every rule that applies, run: the hot spot first, then by severity (the worst first) and
/// impact.
pub fn analyze(cx: &RuleContext<'_>) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for rule in rules() {
        if rule.applies(cx.report) {
            out.extend(rule.check(cx));
        }
    }
    out.sort_by(|a, b| {
        // `false` (the hot spot) first.
        a.is_problem()
            .cmp(&b.is_problem())
            .then(b.severity.cmp(&a.severity))
            .then(b.impact.total_cmp(&a.impact))
    });
    out
}

/// The problems among `findings` (the hot spot isn't one) and the critical ones.
pub fn problems(findings: &[Finding]) -> (usize, usize) {
    let problems = findings.iter().filter(|f| f.is_problem()).count();
    let critical = findings.iter().filter(|f| f.severity == Severity::Critical).count();
    (problems, critical)
}

/// Every locale key the findings' texts may use (the tests check both languages have them).
pub fn text_keys() -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        let id = rule.id();
        for part in ["title", "why", "fix"] {
            out.push(format!("profile.finding.{id}.{part}"));
        }
        for v in rule.variants() {
            out.push(format!("profile.finding.{id}.why_{v}"));
            out.push(format!("profile.finding.{id}.fix_{v}"));
        }
    }
    out
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules.rs"]
mod tests;
