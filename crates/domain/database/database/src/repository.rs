//! A table as the grid reads it: the one place its rows, its total and its structure are
//! read from and its changes go through, whatever is underneath (an SQL engine, a CSV
//! file, a later driver). The grid never waits: what isn't known yet is [`Fetch::Pending`]
//! (asked for in the background), and when it comes, [`TableRepository::poll`] says so.

use std::sync::Arc;

use crate::{Applied, ChangeSet, Collect, Column, DbError, Filter, Page, RowKey, Structure, Value};

/// What a repository gives at once: there, still coming (asked for), or failed.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetch<T> {
    Ready(T),
    /// Asked for: [`TableRepository::poll`] says when it came.
    Pending,
    Failed(DbError),
}

impl<T> Fetch<T> {
    pub fn ready(self) -> Option<T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}

/// A page of rows, as read: the columns, the values and each row's key (when the table
/// has one: the rows can then be edited).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rows {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    /// One per row, or none (a table without a key: only read).
    pub keys: Vec<RowKey>,
}

impl Rows {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The memory it holds, estimated (what a cache of pages counts against its limit).
    pub fn bytes(&self) -> usize {
        fn value(v: &Value) -> usize {
            let heap = match v {
                Value::Text(t) | Value::Json(t) | Value::Other(t) => t.len(),
                Value::Bytes(b) => b.len(),
                _ => 0,
            };
            std::mem::size_of::<Value>() + heap
        }
        fn key(k: &RowKey) -> usize {
            let heap = match k {
                RowKey::Primary(columns) => columns.iter().map(|(c, v)| c.len() + value(v)).sum(),
                _ => 0,
            };
            std::mem::size_of::<RowKey>() + heap
        }
        let columns: usize = self.columns.iter().map(|c| c.name.len() + c.type_name.len()).sum();
        let rows: usize = self
            .rows
            .iter()
            .map(|r| std::mem::size_of::<Vec<Value>>() + r.iter().map(value).sum::<usize>())
            .sum();
        std::mem::size_of::<Self>() + columns + rows + self.keys.iter().map(key).sum::<usize>()
    }
}

impl From<Collect> for Rows {
    fn from(collect: Collect) -> Self {
        Self { columns: collect.columns, rows: collect.rows, keys: collect.keys }
    }
}

/// What came since the last [`TableRepository::poll`]: the view asks again for what it
/// shows.
#[derive(Debug, Clone, PartialEq)]
pub enum RepositoryEvent {
    /// The structure came (or failed).
    Structure,
    /// A total came (or failed).
    Count,
    /// A page came (or failed).
    Page(Page),
    /// The changes sent were applied (all of them), or none was.
    Applied(Result<Applied, DbError>),
    /// Everything known was let go (a commit, here or in another view, or asked): read
    /// again.
    Refreshed,
}

/// A table (or a query's result), read and changed through one contract: what the grid
/// sees, never what's underneath. Nothing blocks — what isn't known is asked for and
/// [`Fetch::Pending`] until [`Self::poll`] says it came.
pub trait TableRepository: Send {
    /// The rows `filter` keeps (`1–500 of N`).
    fn count(&mut self, filter: Option<&Filter>) -> Fetch<u64>;
    /// A page of rows, in its order, kept by its filter.
    fn page(&mut self, page: &Page) -> Fetch<Arc<Rows>>;
    /// Its columns, indexes and keys.
    fn structure(&mut self) -> Fetch<Arc<Structure>>;
    /// What applying `changes` would run, as shown before it does (SQL; none for a source
    /// that writes no SQL: its summary is the view's).
    fn preview(&self, changes: &ChangeSet) -> Result<Vec<String>, DbError>;
    /// Applies `changes` in one transaction (all or none): [`RepositoryEvent::Applied`]
    /// says how it went, then [`RepositoryEvent::Refreshed`].
    fn apply(&mut self, changes: ChangeSet);
    /// Lets go of everything known: read again when asked.
    fn refresh(&mut self);
    /// What came since the last call (on the UI thread, after a wake).
    fn poll(&mut self) -> Vec<RepositoryEvent>;
    /// The memory its kept rows hold (bytes, estimated), for the Task Manager.
    fn memory(&self) -> usize {
        0
    }
}

#[cfg(test)]
#[path = "../tests/unit/repository.rs"]
mod tests;
