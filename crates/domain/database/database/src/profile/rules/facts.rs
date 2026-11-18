//! What the rules know beyond the profile ([`Facts`], gathered by the console's module in the
//! background, on the console's connection, only for the profile's tables — at most
//! [`MAX_TABLES`]): each table's structure (its columns, its indexes with their order,
//! uniqueness, predicate and expressions, its foreign keys), the server's statistics when
//! read, and the relations the user decided (the virtual keys accepted, plan 37). A rule
//! needing what isn't here says nothing: an index that may exist is never guessed.
//!
//! [`RecentRuns`] are the fingerprints of the statements a console's session ran, with their
//! times (N+1).

use std::collections::VecDeque;
use std::time::Duration;

use crate::profile::Profile;
use crate::profile::node::{ObjectKind, ProfileNode};
use crate::profile::sql_map::SqlMap;
use crate::profile::tables::TableStats;
use crate::relations::RelationNotes;
use crate::{IndexInfo, Structure, TableRef};

/// The most tables facts are gathered for.
pub const MAX_TABLES: usize = 50;

/// What's known of a table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableFacts {
    pub table: TableRef,
    pub structure: Option<Structure>,
    pub stats: Option<TableStats>,
    /// The catalog's estimate of its rows.
    pub rows: Option<u64>,
}

impl TableFacts {
    pub fn new(table: TableRef) -> Self {
        Self { table, ..Self::default() }
    }

    /// Its indexes are known (its structure, or the server's statistics of them).
    pub fn knows_indexes(&self) -> bool {
        self.structure.is_some() || self.stats.as_ref().is_some_and(|s| !s.indexes.is_empty())
    }

    /// Its indexes: the structure's (SQLite's `INTEGER PRIMARY KEY`, the table's own order,
    /// counted as one), else the statistics'.
    pub fn indexes(&self) -> Vec<IndexInfo> {
        if let Some(s) = &self.structure {
            let mut out = s.indexes.clone();
            let key = s.primary_key();
            if s.rowid
                && key.len() == 1
                && s.column(&key[0])
                    .is_some_and(|c| c.declared_type.eq_ignore_ascii_case("INTEGER"))
                && !out
                    .iter()
                    .any(|i| i.columns.first().is_some_and(|c| c.eq_ignore_ascii_case(&key[0])))
            {
                out.push(IndexInfo::new("PRIMARY KEY", key, true));
            }
            return out;
        }
        self.stats
            .as_ref()
            .map(|s| {
                s.indexes
                    .iter()
                    .map(|i| IndexInfo::new(i.name.clone(), i.columns.clone(), i.unique))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The first index whose columns start with `columns` (in their order, without case;
    /// expressions never match a plain column).
    pub fn index_starting_with(&self, columns: &[String]) -> Option<IndexInfo> {
        if columns.is_empty() {
            return None;
        }
        self.indexes().into_iter().find(|i| {
            i.columns.len() >= columns.len()
                && i.columns.iter().zip(columns).all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
    }

    /// An index on an expression of `column` (`lower(email)`), or one whose expression isn't
    /// known (SQLite lists an expression's column without a name).
    pub fn expression_index_on(&self, column: &str) -> Option<IndexInfo> {
        let lower = column.to_ascii_lowercase();
        self.indexes().into_iter().find(|i| {
            i.columns.is_empty()
                || i.columns
                    .iter()
                    .any(|c| c.contains('(') && c.to_ascii_lowercase().contains(&lower))
        })
    }

    /// Its rows: the server's statistics, else the catalog's estimate.
    pub fn row_count(&self) -> Option<u64> {
        self.stats.as_ref().and_then(|s| s.live_rows).or(self.rows)
    }

    /// The column is one of its own (its structure known).
    pub fn has_column(&self, column: &str) -> Option<bool> {
        self.structure
            .as_ref()
            .map(|s| s.columns.iter().any(|c| c.name.eq_ignore_ascii_case(column)))
    }
}

/// A foreign key of a table, the engine's or one the user accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFact {
    pub columns: Vec<String>,
    pub target: TableRef,
    pub references: Vec<String>,
    /// Accepted by the user (plan 37), not kept by the engine.
    pub accepted: bool,
}

/// What the rules know beyond the profile.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    pub tables: Vec<TableFacts>,
    /// The relations decided for the source (its accepted virtual keys).
    pub notes: RelationNotes,
}

impl Facts {
    /// What's known of `table` (by name without case; by schema too when both have one).
    pub fn table(&self, table: &TableRef) -> Option<&TableFacts> {
        self.tables.iter().find(|t| same_table(&t.table, table))
    }

    /// The table named `name` (one only).
    pub fn named(&self, name: &str) -> Option<&TableFacts> {
        let mut found = self.tables.iter().filter(|t| t.table.name.eq_ignore_ascii_case(name));
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    /// `table`'s foreign keys: the engine's, then the virtual keys accepted.
    pub fn foreign_keys(&self, table: &TableRef) -> Vec<KeyFact> {
        let mut out: Vec<KeyFact> = self
            .table(table)
            .and_then(|t| t.structure.as_ref())
            .map(|s| {
                s.foreign_keys
                    .iter()
                    .map(|k| KeyFact {
                        columns: k.columns.clone(),
                        target: k.table.clone(),
                        references: k.references.clone(),
                        accepted: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for key in &self.notes.virtual_keys {
            if same_table(&key.table, table) {
                out.push(KeyFact {
                    columns: key.columns.clone(),
                    target: key.target.clone(),
                    references: key.references.clone(),
                    accepted: true,
                });
            }
        }
        out
    }
}

/// Two names of a table are the same one (without case; schemas compared when both say).
pub fn same_table(a: &TableRef, b: &TableRef) -> bool {
    a.name.eq_ignore_ascii_case(&b.name)
        && match (&a.schema, &b.schema) {
            (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
            _ => true,
        }
}

/// The tables `profile` touches (its nodes' tables, an alias taken back to its table through
/// the statement's `map`; the statement's own tables when no node names one), each once, at
/// most [`MAX_TABLES`]: what the module gathers facts for.
pub fn tables_of(profile: &Profile, map: &SqlMap) -> Vec<TableRef> {
    let mut out: Vec<TableRef> = Vec::new();
    let mut push = |t: TableRef| {
        if out.len() < MAX_TABLES && !out.iter().any(|o| same_table(o, &t)) {
            out.push(t);
        }
    };
    for node in &profile.nodes {
        if let Some(t) = node_table(node, map) {
            push(t);
        }
    }
    for t in map.tables() {
        push(t.table.clone());
    }
    out
}

/// A node's table, as the statement names it (an alias taken back to its table).
pub fn node_table(node: &ProfileNode, map: &SqlMap) -> Option<TableRef> {
    let object = node.object.as_ref().filter(|o| o.kind == ObjectKind::Table)?;
    if object.name.starts_with('(') {
        return None;
    }
    // SQLite may name a table by its alias (`SCAN o`).
    if let Some(found) = map.resolve(&object.name)
        && (found.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(&object.name))
            || found.table.name.eq_ignore_ascii_case(&object.name))
    {
        let mut table = found.table.clone();
        if table.schema.is_none() {
            table.schema.clone_from(&object.schema);
        }
        return Some(table);
    }
    Some(TableRef { schema: object.schema.clone(), name: object.name.clone() })
}

/// The statements a console's session ran: their fingerprints with the times they ran
/// (from the runner's own start), at most [`RecentRuns::KEEP`] fingerprints of
/// [`RecentRuns::TIMES`] times each.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecentRuns {
    runs: Vec<(u64, VecDeque<Duration>)>,
    /// The latest time told.
    pub now: Duration,
}

impl RecentRuns {
    pub const KEEP: usize = 256;
    pub const TIMES: usize = 64;

    /// The statement of fingerprint `hash` ran at `at`.
    pub fn record(&mut self, hash: u64, at: Duration) {
        self.now = self.now.max(at);
        let at_index = self.runs.iter().position(|(h, _)| *h == hash);
        let mut entry = match at_index {
            Some(i) => self.runs.remove(i),
            None => (hash, VecDeque::new()),
        };
        entry.1.push_back(at);
        while entry.1.len() > Self::TIMES {
            entry.1.pop_front();
        }
        // The newest last; the oldest go past the limit.
        self.runs.push(entry);
        if self.runs.len() > Self::KEEP {
            self.runs.remove(0);
        }
    }

    /// How many times fingerprint `hash` ran in the `window` up to [`RecentRuns::now`].
    pub fn count_within(&self, hash: u64, window: Duration) -> usize {
        let since = self.now.saturating_sub(window);
        self.runs
            .iter()
            .find(|(h, _)| *h == hash)
            .map_or(0, |(_, times)| times.iter().filter(|t| **t >= since).count())
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/facts.rs"]
mod tests;
