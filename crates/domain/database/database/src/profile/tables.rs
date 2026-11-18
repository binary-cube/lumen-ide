//! What the server knows of the tables a statement touches ([`TableStats`], from
//! [`crate::Session::table_stats`]): their rows, sizes, scans, maintenance and indexes; and
//! what the statement did with each ([`usage`], plan 39h6: how it read it, the rows read and
//! kept, its part of the time, the findings about it).

use crate::TableRef;
use crate::profile::extra::Extra;

/// An index's statistics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexStats {
    pub name: String,
    /// Its columns in order (an expression as written).
    pub columns: Vec<String>,
    pub unique: bool,
    pub bytes: Option<u64>,
    /// Times it was used to scan (PostgreSQL's `idx_scan`).
    pub scans: Option<u64>,
}

/// A table's statistics.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableStats {
    pub table: TableRef,
    pub live_rows: Option<u64>,
    pub dead_rows: Option<u64>,
    pub table_bytes: Option<u64>,
    pub index_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub seq_scans: Option<u64>,
    pub idx_scans: Option<u64>,
    /// As the server writes them (ISO text: the domain has no clock).
    pub last_analyze: Option<String>,
    pub last_autoanalyze: Option<String>,
    pub last_vacuum: Option<String>,
    pub modified_since_analyze: Option<u64>,
    /// The planner has statistics for it (SQLite's `sqlite_stat1`, after `ANALYZE`).
    pub stats_present: Option<bool>,
    pub indexes: Vec<IndexStats>,
    pub extra: Vec<Extra>,
}

impl TableStats {
    pub fn new(table: TableRef) -> Self {
        Self { table, ..Self::default() }
    }
}

use crate::profile::derive::ProfileReport;
use crate::profile::node::{NodeId, ObjectKind, OpKind};
use crate::profile::rules::{EvidenceKey, node_table, same_table};
use crate::profile::sql_map::SqlMap;

/// How a table was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Access {
    /// Read whole (`Seq Scan`, SQLite's `SCAN t`).
    Scan,
    /// Through an index, then the table (`Index Scan`, SQLite's `SEARCH … USING INDEX`).
    Index,
    /// From an index alone.
    IndexOnly,
    /// A bitmap of an index's matches, then the table's blocks.
    Bitmap,
    /// By its row id (SQLite's `INTEGER PRIMARY KEY`, PostgreSQL's TID).
    RowId,
    /// Through an index the engine made for this statement (SQLite's automatic index).
    AutoIndex,
    /// Written (an `UPDATE`, `DELETE`, `INSERT`).
    Write,
}

impl Access {
    /// Stable (the locales' keys): `scan`, `index`…
    pub fn id(self) -> &'static str {
        match self {
            Access::Scan => "scan",
            Access::Index => "index",
            Access::IndexOnly => "index_only",
            Access::Bitmap => "bitmap",
            Access::RowId => "row_id",
            Access::AutoIndex => "auto_index",
            Access::Write => "write",
        }
    }

    /// How a step of kind `op` reads its table (`None`: it doesn't).
    pub fn of(op: OpKind) -> Option<Self> {
        Some(match op {
            OpKind::SeqScan => Access::Scan,
            OpKind::IndexScan => Access::Index,
            OpKind::IndexOnlyScan => Access::IndexOnly,
            OpKind::BitmapHeapScan => Access::Bitmap,
            OpKind::RowidLookup | OpKind::TidScan => Access::RowId,
            OpKind::AutoIndex => Access::AutoIndex,
            OpKind::ModifyTable(_) => Access::Write,
            _ => return None,
        })
    }
}

/// A step reading (or writing) a table.
#[derive(Debug, Clone, PartialEq)]
pub struct TableScan {
    pub node: NodeId,
    pub access: Access,
    /// The index it went through (a bitmap's: its index scans'), as the engine names it.
    pub index: Option<String>,
    /// The alias the statement gives the table there.
    pub alias: Option<String>,
}

/// What a statement did with one table ([`usage`]): how it read it (each step that did), the
/// rows read and kept, its part of the time, the findings about it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableUsage {
    pub table: TableRef,
    /// The statement's names for it (`o`, `o2`: the same table twice), in order.
    pub aliases: Vec<String>,
    pub scans: Vec<TableScan>,
    /// The rows its steps went through (kept, and removed by a filter or a recheck), in all
    /// their loops; on a plan only estimated, the rows expected.
    pub rows_read: Option<f64>,
    /// The rows its steps gave up (in all their loops).
    pub rows_returned: Option<f64>,
    /// Its steps' own time (ms), and its share of the statement's (0–1); on a plan only
    /// estimated, the share of the cost.
    pub self_ms: Option<f64>,
    pub share: Option<f64>,
    /// The rows were estimated, not counted.
    pub estimated: bool,
    /// The findings about it (indices into the report's), the worst first.
    pub findings: Vec<usize>,
}

impl TableUsage {
    /// Its steps.
    pub fn nodes(&self) -> Vec<NodeId> {
        self.scans.iter().map(|s| s.node).collect()
    }

    /// The indexes it was read through, each once, in order.
    pub fn indexes_used(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for s in &self.scans {
            if let Some(i) = s.index.as_deref()
                && !out.iter().any(|o| o.eq_ignore_ascii_case(i))
            {
                out.push(i);
            }
        }
        out
    }

    /// Index `name` was used by this statement.
    pub fn uses_index(&self, name: &str) -> bool {
        self.scans.iter().any(|s| s.index.as_deref().is_some_and(|i| i.eq_ignore_ascii_case(name)))
    }

    /// How it was read, each way once, the costliest first (a scan before an index).
    pub fn accesses(&self) -> Vec<Access> {
        let mut out: Vec<Access> = self.scans.iter().map(|s| s.access).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The tables `report`'s statement touches (`map`: its text's, an alias taken back to its
/// table), the heaviest first (by share; then as the plan meets them): the steps reading or
/// writing each, the rows, the time, the findings.
pub fn usage(report: &ProfileReport, map: &SqlMap) -> Vec<TableUsage> {
    let p = &report.profile;
    let estimated = !p.depth.runs() || p.nodes.iter().all(|n| !n.ran());
    let mut out: Vec<TableUsage> = Vec::new();
    for &id in &report.order {
        let node = p.node(id);
        let Some(access) = Access::of(node.op) else { continue };
        let Some(table) = node_table(node, map) else { continue };
        let alias = node
            .object
            .as_ref()
            .and_then(|o| o.alias.clone())
            .filter(|a| !a.eq_ignore_ascii_case(&table.name))
            .or_else(|| {
                // SQLite names a table by its alias (`SCAN o`).
                let named = node.object.as_ref()?.name.clone();
                (!named.eq_ignore_ascii_case(&table.name)).then_some(named)
            });
        // A bitmap's index is its index scans'.
        let index = node.index.clone().or_else(|| {
            (node.op == OpKind::BitmapHeapScan)
                .then(|| {
                    let names: Vec<String> = descendants(report, id)
                        .into_iter()
                        .filter(|c| p.node(*c).op == OpKind::BitmapIndexScan)
                        .filter_map(|c| {
                            let n = p.node(c);
                            n.index.clone().or_else(|| {
                                n.object
                                    .as_ref()
                                    .filter(|o| o.kind == ObjectKind::Index)
                                    .map(|o| o.name.clone())
                            })
                        })
                        .collect();
                    (!names.is_empty()).then(|| names.join(", "))
                })
                .flatten()
        });
        let at = match out.iter().position(|u| same_table(&u.table, &table)) {
            Some(i) => i,
            None => {
                out.push(TableUsage { table, estimated, ..TableUsage::default() });
                out.len() - 1
            }
        };
        let u = &mut out[at];
        if let Some(a) = &alias
            && !u.aliases.iter().any(|x| x.eq_ignore_ascii_case(a))
        {
            u.aliases.push(a.clone());
        }
        u.scans.push(TableScan { node: id, access, index, alias });
        if access == Access::Write {
            continue;
        }
        let d = report.derived(id);
        let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
        let read = if estimated {
            node.estimates.rows.map(|r| r * loops)
        } else {
            d.total_rows.map(|kept| {
                let a = &node.actuals;
                kept + (a.rows_removed_by_filter.unwrap_or(0.0)
                    + a.rows_removed_by_recheck.unwrap_or(0.0))
                    * loops
            })
        };
        let returned = if estimated { read } else { d.total_rows };
        add(&mut u.rows_read, read);
        add(&mut u.rows_returned, returned);
        add(&mut u.self_ms, d.self_ms);
        add(&mut u.share, if estimated { d.cost_pct } else { d.self_pct.or(d.cost_pct) });
    }
    for u in &mut out {
        if let Some(s) = u.share.as_mut() {
            *s = s.min(1.0);
        }
        let nodes = u.nodes();
        let mut found: Vec<usize> = (0..report.findings.len())
            .filter(|&i| {
                let f = &report.findings[i];
                f.is_problem()
                    && (f.nodes.iter().any(|n| nodes.contains(n))
                        || f.text_of(EvidenceKey::Table).is_some_and(|t| names(t, &u.table)))
            })
            .collect();
        found.sort_by(|a, b| {
            let (fa, fb) = (&report.findings[*a], &report.findings[*b]);
            fb.severity.cmp(&fa.severity).then(fb.impact.total_cmp(&fa.impact))
        });
        u.findings = found;
    }
    // The heaviest first; ties as the plan met them.
    out.sort_by(|a, b| b.share.unwrap_or(0.0).total_cmp(&a.share.unwrap_or(0.0)));
    out
}

fn add(sum: &mut Option<f64>, value: Option<f64>) {
    if let Some(v) = value {
        *sum = Some(sum.unwrap_or(0.0) + v);
    }
}

/// `text` names `table` (`orders`, `public.orders`, without case).
fn names(text: &str, table: &TableRef) -> bool {
    match text.rsplit_once('.') {
        Some((schema, name)) => {
            name.eq_ignore_ascii_case(&table.name)
                && table.schema.as_deref().is_none_or(|s| s.eq_ignore_ascii_case(schema))
        }
        None => text.eq_ignore_ascii_case(&table.name),
    }
}

/// Every step under `id` (not itself).
fn descendants(report: &ProfileReport, id: NodeId) -> Vec<NodeId> {
    let p = &report.profile;
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = p.node(id).children.clone();
    while let Some(c) = stack.pop() {
        if c.index() >= p.nodes.len() || out.contains(&c) {
            continue;
        }
        out.push(c);
        stack.extend(p.node(c).children.iter().copied());
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/profile/tables.rs"]
mod tests;
