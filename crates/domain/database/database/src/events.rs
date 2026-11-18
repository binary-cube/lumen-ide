//! The databases context's domain events (on the project's bus): the sources listed, a
//! console's run, the data a commit changed — so every view of it refreshes — and a schema
//! known to be stale.

use kernel::DomainEvent;

use crate::{DataSourceId, Scope, TableRef};

/// A source of `scope`'s list was added, changed or removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataSourcesChanged {
    pub scope: Scope,
}

impl DomainEvent for DataSourcesChanged {}

/// A commit changed `table` of `source` (its rows, or its structure): its other views read
/// it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataChanged {
    pub source: DataSourceId,
    pub table: TableRef,
    /// Its structure changed too (not only its rows).
    pub structure: bool,
}

impl DomainEvent for DataChanged {}

/// Console `console` of `source` opened a transaction (`open`) or ended it (committed,
/// rolled back, its connection gone). What shows a console's state (its tab, the Services
/// and Database windows) and what explains a lock (a grid's write refused) hears it here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionChanged {
    pub console: crate::console::ConsoleId,
    pub source: DataSourceId,
    pub open: bool,
}

impl DomainEvent for TransactionChanged {}

/// What's known of `source`'s schema is stale (a console ran DDL, the "Database" window's
/// Refresh, its connection changed): what keeps an index of it reads it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaInvalidated {
    pub source: DataSourceId,
}

impl DomainEvent for SchemaInvalidated {}
