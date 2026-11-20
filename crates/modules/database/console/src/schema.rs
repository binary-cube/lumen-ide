//! A project's schema indexes for completion ([`database::schema_index::SchemaIndex`]), one
//! per source and database its consoles use: read in the background on the pool
//! `db-schema-index`, on a connection of its own (made through the explorer's
//! [`ConsoleAccess::connector`]: never a console's, whose transaction stays its own), only
//! when a console first asks. Two tiers: the tables' names always; their columns and keys at
//! once up to `console_eager_tables` tables (and the memory limit), else those a statement
//! names, read when asked (the connection kept for that, closed when the index goes).
//!
//! The old index serves until the new one comes. It's read again when it's stale
//! ([`database::events::SchemaInvalidated`]: DDL run from a console, the "Database" window's
//! Refresh, a connection changed; a structure changed by the explorer's modals; the
//! relations the user decided changed), forgotten with "Clear and Rebuild Caches", and let go
//! once no console of its source is open for a while. Every arrival bumps
//! [`Indexes::generation`] and wakes the app: an open completion list is made again.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use database::console::{Connector, ConsoleAccess, Overrides};
use database::relations::RelationNotes;
use database::schema_index::{IndexParts, SchemaIndex};
use database::{Cancel, ColumnBrief, DataSourceId, Session, TableRef};

use crate::runtime::Runtime;

/// About what a column takes in an index (bytes): to tell a schema too big to read whole.
const COLUMN_BYTES: usize = 120;
/// About how many columns a table has.
const COLUMNS_PER_TABLE: usize = 30;

/// A source's (and database's) index, and what's being done for it.
struct Entry {
    source: DataSourceId,
    database: Option<String>,
    index: Option<Arc<SchemaIndex>>,
    /// A read under way (its ticket: a result of an older one is dropped).
    building: Option<u64>,
    /// Read again when next asked (the old one serves meanwhile).
    stale: bool,
    /// Tables whose columns are being read.
    loading: Vec<TableRef>,
    /// The second tier's connection (kept while the index is).
    session: Arc<Mutex<Option<Box<dyn Session>>>>,
    connector: Option<Connector>,
    /// What failed last (said in the completion list, read again when invalidated).
    error: Option<String>,
    used: Instant,
    /// Bumped when the index is replaced or made stale: columns read for an older one are
    /// dropped.
    epoch: u64,
}

/// A project's schema indexes (see the module's doc).
pub struct Indexes {
    entries: Mutex<Vec<Entry>>,
    generation: AtomicU64,
    tickets: AtomicU64,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Told when an index or columns arrive (the diagnostics, checked again).
    arrivals: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
    runtime: Mutex<Runtime>,
    /// The tables and bytes read at once, when not the settings' (the tests').
    limits: Mutex<Option<(usize, usize)>>,
}

impl std::fmt::Debug for Indexes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Indexes").field("generation", &self.generation()).finish_non_exhaustive()
    }
}

impl Indexes {
    pub fn new(runtime: Runtime) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::default(),
            generation: AtomicU64::new(1),
            tickets: AtomicU64::new(0),
            wake: Mutex::default(),
            arrivals: Mutex::default(),
            runtime: Mutex::new(runtime),
            limits: Mutex::default(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its work runs on `runtime` from now (the tests').
    pub fn set_runtime(&self, runtime: Runtime) {
        *self.runtime.lock().unwrap_or_else(PoisonError::into_inner) = runtime;
    }

    /// Up to `tables` tables (and `bytes`) read at once, whatever the settings say (the
    /// tests', the snapshots').
    pub fn set_limits(&self, tables: usize, bytes: usize) {
        *self.limits.lock().unwrap_or_else(PoisonError::into_inner) = Some((tables, bytes));
    }

    /// What arrives wakes the app with `wake`.
    pub fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        *self.wake.lock().unwrap_or_else(PoisonError::into_inner) = Some(wake);
    }

    /// `then` is called each time an index or columns arrive (on the thread that read them).
    pub fn on_arrival(&self, then: Arc<dyn Fn() + Send + Sync>) {
        self.arrivals.lock().unwrap_or_else(PoisonError::into_inner).push(then);
    }

    /// Wakes the app (something it shows changed in the background).
    pub fn wake_app(&self) {
        let wake = self.wake.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    /// Bumped by every index or columns arriving (what a completion list follows).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn arrived(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let arrivals = self.arrivals.lock().unwrap_or_else(PoisonError::into_inner).clone();
        for then in arrivals {
            then();
        }
        self.wake_app();
    }

    /// The index of `source`'s `database` as built so far (`None`: not yet): a read started
    /// when there's none, or it's stale (the old one serving meanwhile).
    pub fn index(
        self: &Arc<Self>,
        access: &dyn ConsoleAccess,
        source: DataSourceId,
        database: Option<&str>,
        now: Instant,
    ) -> Option<Arc<SchemaIndex>> {
        let mut entries = self.lock();
        let at = match entries
            .iter()
            .position(|e| e.source == source && e.database.as_deref() == database)
        {
            Some(at) => at,
            None => {
                entries.push(Entry {
                    source,
                    database: database.map(str::to_owned),
                    index: None,
                    building: None,
                    stale: true,
                    loading: Vec::new(),
                    session: Arc::default(),
                    connector: None,
                    error: None,
                    used: now,
                    epoch: 0,
                });
                entries.len() - 1
            }
        };
        let entry = &mut entries[at];
        entry.used = now;
        let current = entry.index.clone();
        if entry.stale && entry.building.is_none() {
            entry.stale = false;
            let overrides = Overrides { database: entry.database.clone() };
            match access.connector(source, &overrides) {
                Ok(connector) => {
                    let ticket = self.tickets.fetch_add(1, Ordering::AcqRel) + 1;
                    entry.building = Some(ticket);
                    entry.connector = Some(Arc::clone(&connector));
                    let notes = access.notes(source);
                    let database = entry.database.clone();
                    drop(entries);
                    self.build(source, database, connector, notes, ticket);
                }
                Err(e) => entry.error = Some(e.to_string()),
            }
        }
        current
    }

    /// Reads the index in the background (see the module's doc).
    fn build(
        self: &Arc<Self>,
        source: DataSourceId,
        database: Option<String>,
        connector: Connector,
        notes: RelationNotes,
        ticket: u64,
    ) {
        let this = Arc::clone(self);
        let (eager, memory) =
            self.limits.lock().unwrap_or_else(PoisonError::into_inner).unwrap_or_else(|| {
                (crate::settings::eager_tables(), crate::settings::index_memory())
            });
        let runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner).clone();
        runtime.spawn_index(move || {
            let generation = this.tickets.fetch_add(1, Ordering::AcqRel) + 1;
            let read =
                read_schema(&connector, source, database.clone(), notes, eager, memory, generation);
            let mut entries = this.lock();
            let Some(entry) = entries.iter_mut().find(|e| {
                e.source == source && e.database == database && e.building == Some(ticket)
            }) else {
                return;
            };
            entry.building = None;
            match read {
                Ok((index, session)) => {
                    tracing::debug!(
                        tables = index.tables().len(),
                        complete = index.is_complete(),
                        memory = index.memory(),
                        "a source's schema was read for completion"
                    );
                    entry.index = Some(Arc::new(index));
                    entry.epoch += 1;
                    entry.error = None;
                    entry.loading.clear();
                    let old = std::mem::replace(
                        &mut *entry.session.lock().unwrap_or_else(PoisonError::into_inner),
                        session,
                    );
                    drop(entries);
                    // Closed once nothing waits on the lock.
                    drop(old);
                    this.arrived();
                    return;
                }
                Err(e) => {
                    tracing::info!(error = %e, "a source's schema wasn't read for completion");
                    entry.error = Some(e.to_string());
                }
            }
            drop(entries);
            this.arrived();
        });
    }

    /// `tables`' columns (of `source`'s `database`, an index of the second tier), read in
    /// the background unless they are being read already; the index replaced when they
    /// come.
    pub fn request(
        self: &Arc<Self>,
        source: DataSourceId,
        database: Option<&str>,
        tables: &[TableRef],
    ) {
        let mut entries = self.lock();
        let Some(entry) =
            entries.iter_mut().find(|e| e.source == source && e.database.as_deref() == database)
        else {
            return;
        };
        let Some(index) = entry.index.clone() else { return };
        let wanted: Vec<TableRef> =
            index.missing(tables).into_iter().filter(|t| !entry.loading.contains(t)).collect();
        if wanted.is_empty() {
            return;
        }
        let Some(connector) = entry.connector.clone() else { return };
        entry.loading.extend(wanted.iter().cloned());
        let session = Arc::clone(&entry.session);
        let epoch = entry.epoch;
        let database = entry.database.clone();
        drop(entries);
        let this = Arc::clone(self);
        let runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner).clone();
        runtime.spawn_index(move || {
            let loaded = {
                let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
                if session.is_none() {
                    *session = connector().ok();
                }
                match session.as_mut() {
                    Some(s) => wanted
                        .iter()
                        .filter_map(|t| {
                            let found = index.find(t).map(|i| index.table(i).table.clone())?;
                            let structure = s.structure(&found).ok()?;
                            let columns = structure.columns.iter().map(ColumnBrief::from).collect();
                            Some((found, columns, structure.foreign_keys))
                        })
                        .collect::<Vec<_>>(),
                    None => Vec::new(),
                }
            };
            let generation = this.tickets.fetch_add(1, Ordering::AcqRel) + 1;
            // The new index made outside the lock (completion takes it at each key): put in
            // only if the one it was made from is still there, else made again from the
            // newer one (another table's columns came meanwhile).
            let mut made: Option<(Arc<SchemaIndex>, Arc<SchemaIndex>)> = None;
            loop {
                let mut entries = this.lock();
                let Some(entry) =
                    entries.iter_mut().find(|e| e.source == source && e.database == database)
                else {
                    return;
                };
                // A newer read meanwhile: its own columns come with it.
                if entry.epoch != epoch {
                    entry.loading.retain(|t| !wanted.contains(t));
                    return;
                }
                let Some(current) = entry.index.clone().filter(|_| !loaded.is_empty()) else {
                    entry.loading.retain(|t| !wanted.contains(t));
                    break;
                };
                if let Some((from, next)) = made.take()
                    && Arc::ptr_eq(&from, &current)
                {
                    entry.index = Some(next);
                    entry.loading.retain(|t| !wanted.contains(t));
                    break;
                }
                drop(entries);
                let next = Arc::new(current.with_columns(loaded.clone(), generation));
                made = Some((current, next));
            }
            this.arrived();
        });
    }

    /// `source`'s indexes are stale: read again when next asked (the old serving), their
    /// second tier's connection closed.
    pub fn invalidate(&self, source: DataSourceId) {
        let mut entries = self.lock();
        let mut closed = Vec::new();
        for e in entries.iter_mut().filter(|e| e.source == source) {
            e.stale = true;
            e.epoch += 1;
            // A read under way is followed by another (its result is still the old schema).
            e.building = None;
            e.loading.clear();
            closed.extend(e.session.lock().unwrap_or_else(PoisonError::into_inner).take());
            e.error = None;
        }
        drop(entries);
        self.close_later(closed);
    }

    /// Connections (or entries holding them) closed on the index's worker, never under the
    /// lock nor on the caller's thread (the app's: an event's handler): a server may take
    /// seconds to say goodbye.
    fn close_later<T: Send + 'static>(&self, closed: Vec<T>) {
        if closed.is_empty() {
            return;
        }
        let runtime = self.runtime.lock().unwrap_or_else(PoisonError::into_inner).clone();
        runtime.spawn_index(move || drop(closed));
    }

    /// The entries `gone` says, taken out (closed by [`Self::close_later`]).
    fn take_out(&self, gone: impl Fn(&Entry) -> bool) -> Vec<Entry> {
        let mut entries = self.lock();
        let (out, kept) = std::mem::take(&mut *entries).into_iter().partition(|e| gone(e));
        *entries = kept;
        out
    }

    /// Every source's (a list changed): those gone forgotten, those whose decided
    /// relations changed read again.
    pub fn sources_changed(&self, access: &dyn ConsoleAccess) {
        let sources: Vec<(DataSourceId, bool)> = self
            .lock()
            .iter()
            .map(|e| {
                let notes = e.index.as_ref().map(|i| i.notes().clone());
                (
                    e.source,
                    access.source(e.source).is_some()
                        && notes.is_none_or(|n| n == access.notes(e.source)),
                )
            })
            .collect();
        for (source, same) in sources {
            if access.source(source).is_none() {
                let gone = self.take_out(|e| e.source == source);
                self.close_later(gone);
            } else if !same {
                self.invalidate(source);
            }
        }
    }

    /// Everything forgotten ("Clear and Rebuild Caches"): read again when next asked.
    pub fn purge(&self) {
        let all = std::mem::take(&mut *self.lock());
        self.close_later(all);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// The indexes of sources none of whose consoles is open (`open` says) and unused for
    /// `idle` let go; how many.
    pub fn release_idle(
        &self,
        now: Instant,
        idle: std::time::Duration,
        open: impl Fn(DataSourceId) -> bool,
    ) -> usize {
        let gone = self.take_out(|e| {
            !(open(e.source)
                || now.saturating_duration_since(e.used) < idle
                || e.building.is_some())
        });
        let count = gone.len();
        self.close_later(gone);
        count
    }

    /// When the next index unused by an open console may be let go (`None`: none will).
    pub fn next_due(
        &self,
        idle: std::time::Duration,
        open: impl Fn(DataSourceId) -> bool,
    ) -> Option<Instant> {
        self.lock().iter().filter(|e| !open(e.source)).map(|e| e.used + idle).min()
    }

    /// What the last read of `source`'s `database` failed with.
    pub fn error(&self, source: DataSourceId, database: Option<&str>) -> Option<String> {
        self.lock()
            .iter()
            .find(|e| e.source == source && e.database.as_deref() == database)
            .and_then(|e| e.error.clone())
    }

    /// A read of `source`'s `database` is under way (none built yet, or a newer).
    pub fn building(&self, source: DataSourceId, database: Option<&str>) -> bool {
        self.lock().iter().any(|e| {
            e.source == source
                && e.database.as_deref() == database
                && (e.building.is_some() || !e.loading.is_empty())
        })
    }

    /// Anything being read (the snapshots wait for it).
    pub fn busy(&self) -> bool {
        self.lock().iter().any(|e| e.building.is_some() || !e.loading.is_empty())
    }

    /// What the indexes hold (bytes).
    pub fn memory(&self) -> usize {
        self.lock().iter().filter_map(|e| e.index.as_ref()).map(|i| i.memory()).sum()
    }

    /// How many indexes are kept.
    pub fn len(&self) -> usize {
        self.lock().iter().filter(|e| e.index.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Reads a source's schema through `connector` (on the calling thread: a background one):
/// its tables, their columns and keys at once when they're few enough (`eager` tables,
/// `memory` bytes), its routines. The connection is given back for the second tier (none
/// when every column was read).
fn read_schema(
    connector: &Connector,
    source: DataSourceId,
    database: Option<String>,
    notes: RelationNotes,
    eager: usize,
    memory: usize,
    generation: u64,
) -> Result<(SchemaIndex, Option<Box<dyn Session>>), database::DbError> {
    let mut session = connector()?;
    let cancel = Cancel::default();
    let place = session.current().unwrap_or_default();
    let tables = session.tables(&[])?;
    let complete =
        tables.len() <= eager && tables.len() * COLUMNS_PER_TABLE * COLUMN_BYTES <= memory;
    let (columns, keys) = if complete {
        let columns = session.catalog_columns(None, &cancel)?;
        let keys = session.catalog_keys(None, &cancel).unwrap_or_else(|e| {
            tracing::debug!(error = %e, "the source's keys weren't read");
            Vec::new()
        });
        (columns, keys)
    } else {
        (Vec::new(), Vec::new())
    };
    let routines = session.routines(None, &cancel).unwrap_or_default();
    let readable = session.catalog_relations(&cancel).unwrap_or_else(|e| {
        tracing::debug!(error = %e, "the source's other relations weren't read");
        Vec::new()
    });
    let mut schemas: Vec<String> = Vec::new();
    for t in &tables {
        if let Some(s) = &t.table.schema
            && !schemas.contains(s)
        {
            schemas.push(s.clone());
        }
    }
    if let Some(s) = &place.schema
        && !schemas.iter().any(|x| x.eq_ignore_ascii_case(s))
    {
        schemas.insert(0, s.clone());
    }
    let index = SchemaIndex::build(IndexParts {
        source,
        database: database.or(place.database),
        default_schema: place.schema,
        schemas,
        tables,
        columns,
        keys,
        routines,
        readable,
        notes,
        complete,
        generation,
    });
    Ok((index, (!complete).then_some(session)))
}

#[cfg(test)]
#[path = "../tests/unit/schema.rs"]
mod tests;
