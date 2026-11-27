//! SQLite for the `database` domain, through `rusqlite` with SQLite compiled in (the same
//! version on every system, nothing to install). A session is one connection to a file:
//!
//! - opened read-write, or read-only for a read-only source ([`Target::read_only`]); a
//!   missing file is an error (made only when the target asks: `create=true`); a file
//!   that isn't a database is refused when it opens, not at the first query. A file
//!   busy in another program is waited for ([`BUSY_TIMEOUT`], or the target's
//!   `busy_timeout_ms`); its journal is left as it
//!   is (a WAL database stays WAL). Foreign keys are enforced unless the target says
//!   `foreign_keys=false`;
//! - its tables are those of every schema (`main`, `temp`, the attached ones), with the
//!   row estimates `ANALYZE` left (nothing counted to list them); its structures, and
//!   what a table's rebuild needs, come from the catalog ([`catalog`]);
//! - a page is read with its rows' keys (the primary key's values, or the rowid — by
//!   `_rowid_` or `oid` when a column is named `rowid`); without an order, by rowid — and
//!   the page right after one read starts after its last rowid instead of skipping
//!   `OFFSET` rows;
//! - values come by their storage class (`INTEGER`, `REAL`, `TEXT`, `BLOB`, `NULL`), text
//!   as JSON only in a column declared so; a write says how many rows it changed (an
//!   update or a delete must change exactly one);
//! - a running statement is interrupted when its [`Cancel`] is set (from another thread):
//!   the connection's progress handler, called every few thousand steps of SQLite's
//!   machine, looks at it and stops the statement (`SQLITE_INTERRUPT`, as
//!   `sqlite3_interrupt` would);
//! - for the console: a statement's first page streamed (a query stopped one row past it, a
//!   write run to its end), syntax errors with their place (`sqlite3_error_offset`), the
//!   plan from `EXPLAIN QUERY PLAN` and a profile of the statement run and measured
//!   ([`profile`]: its counters, each plan element's real rows, a write undone), the
//!   tables' statistics, transactions as `BEGIN DEFERRED | IMMEDIATE |
//!   EXCLUSIVE` (open while the connection isn't in autocommit), the attached files as its
//!   schemas, every column of a schema in one query and the functions from
//!   `pragma_function_list`. No cursor: a pure query's next page is run again.

pub mod catalog;
pub mod profile;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use database::profile::{Profile, ProfileDepth, ProfileOptions, RawFormat, TableStats};
use database::run::{Limited, Overflow};
use database::sql::EnginePosition;
use database::sqlite::{RebuildSource, Sqlite};
use database::{
    Cancel, Capabilities, Column, ConsoleCaps, CursorToken, DbError, Dialect, Driver,
    DriverFactory, ExplainKinds, Filter, Flow, Isolation, Isolations, KeyKind, Kind, Language,
    Node, Outcome, Page, Place, PlanNode, RowKey, RowSink, RunOptions, RunOutcome, SchemaChanges,
    SchemaSwitch, Secret, Session, SqlError, Structure, TableColumns, TableInfo, TableKind,
    TableRef, Target, TxOptions, TypeClass, Value,
};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, OpenFlags};

/// The first 16 bytes of every SQLite database file.
pub const HEADER: &[u8; 16] = b"SQLite format 3\0";

/// `head` (a file's first bytes) is a SQLite database's.
pub fn is_database(head: &[u8]) -> bool {
    head.starts_with(HEADER)
}

/// The driver as the app lists it (created only when a connection opens).
pub fn factory() -> DriverFactory {
    DriverFactory::new("sqlite", "SQLite", Kind::Relational, None, || Arc::new(SqliteDriver))
}

/// How long a statement waits for a file another program holds locked.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// How many steps of SQLite's machine run between two looks at a statement's cancel.
const CANCEL_CHECK: i32 = 4096;
/// The pages' places remembered to read the next one by rowid (a few tables at once).
const CURSORS: usize = 16;

/// What a SQLite session can do: everything, a table being rebuilt where `ALTER TABLE`
/// can't change it.
pub const CAPABILITIES: Capabilities = Capabilities {
    transactions: true,
    edit_rows: true,
    schema_changes: SchemaChanges { databases: false, schemas: false, ..SchemaChanges::ALL },
    schemas: true,
    file_based: true,
    cancel: true,
    returning: true,
    sorting: database::Sorting::Any,
    console: Some(CONSOLE),
};

/// What its console can do: no cursor (a page after the first is the query run again),
/// the plan, and as it runs (measured by the driver: [`profile`]), `BEGIN`'s three kinds, the attached files as schemas
/// (nothing to switch on the connection), errors with their place.
pub const CONSOLE: ConsoleCaps = ConsoleCaps {
    cursors: false,
    explain: ExplainKinds { plan: true, analyze: true },
    isolation: Isolations::of(&[Isolation::Deferred, Isolation::Immediate, Isolation::Exclusive]),
    read_only: false,
    switch_schema: SchemaSwitch::NamesOnly,
    databases: false,
    error_positions: true,
    notices: false,
    limited: false,
    profile: profile::CAPS,
};

#[derive(Debug, Default)]
pub struct SqliteDriver;

impl Driver for SqliteDriver {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        Some(Arc::new(Sqlite))
    }

    /// Its file (no password).
    fn connection(&self) -> database::ConnectionSpec {
        database::ConnectionSpec::file(&["sqlite", "sqlite3", "db", "db3"], "sqlite")
    }

    /// Opens `target.database` (the file); no password.
    fn connect(&self, target: &Target, _secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        SqliteSession::open(target).map(|s| Box::new(s) as Box<dyn Session>)
    }
}

/// A failure, as the domain says it.
pub(crate) fn error(e: rusqlite::Error) -> DbError {
    let message = |m: &Option<String>, fallback: &dyn std::fmt::Display| {
        m.clone().unwrap_or_else(|| fallback.to_string())
    };
    match &e {
        rusqlite::Error::SqliteFailure(f, m) => match f.code {
            ErrorCode::OperationInterrupted => DbError::Cancelled,
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => DbError::InUse(format!(
                "the database file is in use by another program ({})",
                message(m, f)
            )),
            ErrorCode::ReadOnly => DbError::ReadOnly(message(m, f)),
            ErrorCode::NotADatabase => DbError::Connect("not a SQLite database".to_owned()),
            ErrorCode::CannotOpen | ErrorCode::PermissionDenied => DbError::Connect(message(m, f)),
            _ => DbError::Query(message(m, f)),
        },
        rusqlite::Error::SqlInputError { error, msg, offset, .. } => match error.code {
            ErrorCode::OperationInterrupted => DbError::Cancelled,
            _ => DbError::sql(SqlError {
                position: usize::try_from(*offset).ok().map(EnginePosition::Byte),
                ..SqlError::new(msg.clone())
            }),
        },
        rusqlite::Error::MultipleStatement => DbError::Query("one statement at a time".to_owned()),
        _ => DbError::Query(e.to_string()),
    }
}

/// A value read, by its storage class (text in a JSON column: JSON).
pub fn value(value: ValueRef<'_>, json: bool) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => {
            let text = String::from_utf8_lossy(t).into_owned();
            if json { Value::Json(text) } else { Value::Text(text) }
        }
        ValueRef::Blob(b) => Value::Bytes(b.to_vec()),
    }
}

/// Where a table's last page read ended: the next one starts after its rowid.
#[derive(Debug, Clone)]
struct Cursor {
    table: TableRef,
    /// The page's filter, as SQL.
    filter: String,
    /// The offset of the page after it.
    next: u64,
    last: i64,
}

/// One connection to a SQLite file (see the crate's doc).
pub struct SqliteSession {
    conn: Connection,
    cursors: Vec<Cursor>,
    /// The schema the console names (SQLite has no default one to change).
    schema: Option<String>,
}

impl std::fmt::Debug for SqliteSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteSession").finish_non_exhaustive()
    }
}

impl SqliteSession {
    /// Opens `target`'s file (see the crate's doc).
    pub fn open(target: &Target) -> Result<Self, DbError> {
        let file = target.database.trim();
        if file.is_empty() {
            return Err(DbError::Connect("no database file given".to_owned()));
        }
        let path = Path::new(file);
        let read_only = target.read_only();
        let create = !read_only && target.flag("create").unwrap_or(false);
        if !create && !path.is_file() {
            return Err(DbError::Connect(format!("no such database file: {}", path.display())));
        }
        let mut flags = OpenFlags::SQLITE_OPEN_NO_MUTEX;
        flags |= if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        if create {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        let conn = Connection::open_with_flags(path, flags).map_err(error)?;
        let busy = target
            .options
            .get("busy_timeout_ms")
            .and_then(|ms| ms.trim().parse().ok())
            .map_or(BUSY_TIMEOUT, Duration::from_millis);
        conn.busy_timeout(busy).map_err(error)?;
        // Each plan element's figures are collected only while a profile runs.
        let _ =
            conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_STMT_SCANSTATUS, false);
        // Read now: a file that isn't a database fails here (and is left as it is).
        conn.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get::<_, i64>(0))
            .map_err(error)?;
        let foreign_keys = target.flag("foreign_keys").unwrap_or(true);
        conn.execute_batch(if foreign_keys {
            "PRAGMA foreign_keys = ON"
        } else {
            "PRAGMA foreign_keys = OFF"
        })
        .map_err(error)?;
        Ok(Self { conn, cursors: Vec::new(), schema: None })
    }

    /// Runs `sql` (one statement): its columns to `columns`, each row to `row` (until it
    /// says stop), interrupted when `cancel` is set. A write says the rows it changed.
    fn stream(
        &mut self,
        sql: &str,
        cancel: &Cancel,
        columns: &mut dyn FnMut(&[Column]),
        row: &mut dyn FnMut(Vec<Value>) -> Flow,
    ) -> Result<Outcome, DbError> {
        let watched = cancel.clone();
        self.conn
            .progress_handler(CANCEL_CHECK, Some(move || watched.is_cancelled()))
            .map_err(error)?;
        let result = self.stream_watched(sql, cancel, columns, row);
        // Failing, the handler stays: it only looks at a cancel no one sets any more.
        let _ = self.conn.progress_handler(0, None::<fn() -> bool>);
        result
    }

    fn stream_watched(
        &self,
        sql: &str,
        cancel: &Cancel,
        columns: &mut dyn FnMut(&[Column]),
        row: &mut dyn FnMut(Vec<Value>) -> Flow,
    ) -> Result<Outcome, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let before = self.conn.total_changes();
        let mut stmt = self.conn.prepare(sql).map_err(error)?;
        let write = !stmt.readonly();
        let set: Vec<Column> = stmt
            .columns()
            .map(|c| Column {
                name: c.name().to_owned(),
                type_name: c.decl_type().unwrap_or_default().to_owned(),
            })
            .collect();
        let json: Vec<bool> =
            set.iter().map(|c| TypeClass::of(&c.type_name) == TypeClass::Json).collect();
        columns(&set);
        let mut outcome = Outcome::default();
        let mut rows = stmt.raw_query();
        while let Some(r) = rows.next().map_err(error)? {
            let values = (0..set.len())
                .map(|i| r.get_ref(i).map(|v| value(v, json[i])))
                .collect::<Result<Vec<Value>, _>>()
                .map_err(error)?;
            outcome.rows += 1;
            if row(values) == Flow::Stop {
                outcome.stopped = true;
                break;
            }
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
        }
        drop(rows);
        drop(stmt);
        if write {
            // `changes()` is the last INSERT / UPDATE / DELETE's: DDL changes none.
            let changed = self.conn.total_changes() != before;
            outcome.affected = Some(if changed { self.conn.changes() } else { 0 });
        }
        Ok(outcome)
    }

    fn node(kind: &'static str, name: &str, detail: String, parent: &[String], open: bool) -> Node {
        let mut path = parent.to_vec();
        path.push(name.to_owned());
        Node { kind, name: name.to_owned(), detail, path, expandable: open }
    }

    /// Remembers where a page of `table` read by rowid ended.
    fn remember(&mut self, cursor: Cursor) {
        self.cursors.retain(|c| !(c.table == cursor.table && c.filter == cursor.filter));
        if self.cursors.len() >= CURSORS {
            self.cursors.remove(0);
        }
        self.cursors.push(cursor);
    }
}

impl Session for SqliteSession {
    fn server(&self) -> String {
        format!("SQLite {}", rusqlite::version())
    }

    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        match path {
            [] => {
                let mut nodes = Vec::new();
                for schema in catalog::schemas(&self.conn)? {
                    // `temp` only once something is in it.
                    if schema == "temp" && catalog::tables(&self.conn, &schema)?.is_empty() {
                        continue;
                    }
                    nodes.push(Self::node("schema", &schema, String::new(), path, true));
                }
                Ok(nodes)
            }
            [schema] => Ok(catalog::tables(&self.conn, schema)?
                .into_iter()
                .map(|t| {
                    let kind = if t.kind == TableKind::View { "view" } else { "table" };
                    let detail = t.rows.map(|n| n.to_string()).unwrap_or_default();
                    Self::node(kind, &t.table.name, detail, path, true)
                })
                .collect()),
            [schema, table] => {
                let s = catalog::structure(&self.conn, &catalog::table_ref(schema, table))?;
                let mut nodes: Vec<Node> = s
                    .columns
                    .iter()
                    .map(|c| Self::node("column", &c.name, c.declared_type.clone(), path, false))
                    .collect();
                nodes.extend(
                    s.indexes
                        .iter()
                        .map(|i| Self::node("index", &i.name, i.columns.join(", "), path, false)),
                );
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    fn execute(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let cell = std::cell::RefCell::new(sink);
        let outcome =
            self.stream(query, cancel, &mut |set| cell.borrow_mut().columns(set), &mut |values| {
                cell.borrow_mut().row(values)
            });
        // A write (or a failed one): the pages' places may not hold any more.
        if !matches!(outcome, Ok(Outcome { affected: None, .. })) {
            self.cursors.clear();
        }
        outcome
    }

    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        let schemas = match path {
            [] => catalog::schemas(&self.conn)?,
            [schema] => vec![schema.clone()],
            _ => return Ok(Vec::new()),
        };
        let mut tables = Vec::new();
        for schema in schemas {
            tables.extend(catalog::tables(&self.conn, &schema)?);
        }
        Ok(tables)
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        catalog::structure(&self.conn, table)
    }

    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let structure = catalog::structure(&self.conn, table)?;
        let d = Sqlite;
        // The row id by a name no column hides (`rowid`, `_rowid_`, `oid`).
        let row_id = structure.row_id_name();
        let by_rowid = row_id.is_some();
        let row_id = row_id.unwrap_or("rowid");
        let filter = match &page.filter {
            Some(f) => format!("({})", d.filter(f)?),
            None => String::new(),
        };
        // No order asked: the rowid's (and the page after one read starts after it).
        let natural = by_rowid && page.order.is_empty();
        let after = if natural {
            self.cursors
                .iter()
                .find(|c| c.table == *table && c.filter == filter && c.next == page.offset)
                .map(|c| c.last)
        } else {
            None
        };

        let mut sql = format!(
            "SELECT {}* FROM {}",
            if by_rowid { format!("{row_id}, ") } else { String::new() },
            d.table_name(table)?
        );
        let mut conditions: Vec<String> = Vec::new();
        if !filter.is_empty() {
            conditions.push(filter.clone());
        }
        if let Some(last) = after {
            conditions.push(format!("{row_id} > {last}"));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        if natural {
            sql.push_str(&format!(" ORDER BY {row_id}"));
        } else if !page.order.is_empty() {
            let order: Vec<String> = page
                .order
                .iter()
                .map(|o| {
                    let c = d.quote_identifier(&o.column)?;
                    Ok(if o.descending { format!("{c} DESC") } else { format!("{c} ASC") })
                })
                .collect::<Result<_, DbError>>()?;
            sql.push_str(" ORDER BY ");
            sql.push_str(&order.join(", "));
        }
        sql.push(' ');
        sql.push_str(&d.pagination(page.limit, if after.is_some() { 0 } else { page.offset }));

        let key = structure.key_kind();
        // The key's columns among the row's (`*`: the structure's, in order).
        let key_at: Vec<(String, usize)> = match &key {
            KeyKind::Primary(columns) => columns
                .iter()
                .filter_map(|k| {
                    Some((k.clone(), structure.columns.iter().position(|c| c.name == *k)?))
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut last = None;
        let outcome = {
            let cell = std::cell::RefCell::new(&mut *sink);
            let last = &mut last;
            self.stream(
                &sql,
                cancel,
                &mut |set| {
                    let set = if by_rowid { &set[1..] } else { set };
                    cell.borrow_mut().columns(set);
                },
                &mut |mut values| {
                    let mut sink = cell.borrow_mut();
                    let rowid = if by_rowid {
                        match values.remove(0) {
                            Value::Int(id) => Some(id),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    *last = rowid.or(*last);
                    match &key {
                        KeyKind::Primary(columns)
                            if key_at.len() == columns.len()
                                && key_at.iter().all(|(_, at)| *at < values.len()) =>
                        {
                            let key = key_at
                                .iter()
                                .map(|(c, at)| (c.clone(), values[*at].clone()))
                                .collect();
                            sink.row_key(RowKey::Primary(key));
                        }
                        KeyKind::RowId => {
                            if let Some(id) = rowid {
                                sink.row_key(RowKey::RowId(id));
                            }
                        }
                        _ => {}
                    }
                    sink.row(values)
                },
            )?
        };
        if natural
            && let Some(last) = last
            && outcome.rows >= u64::from(page.limit)
            && !outcome.stopped
        {
            let next = page.offset + outcome.rows;
            self.remember(Cursor { table: table.clone(), filter, next, last });
        }
        Ok(outcome)
    }

    fn count(
        &mut self,
        table: &TableRef,
        filter: Option<&Filter>,
        cancel: &Cancel,
    ) -> Result<u64, DbError> {
        let sql = Sqlite.count(table, filter)?;
        let mut count = None;
        self.stream(&sql, cancel, &mut |_| {}, &mut |values| {
            if let Some(Value::Int(n)) = values.first() {
                count = Some(*n);
            }
            Flow::Stop
        })?;
        count.map(|n| n.max(0) as u64).ok_or_else(|| DbError::Query(format!("no count of {table}")))
    }

    fn console(&self) -> Option<ConsoleCaps> {
        Some(CONSOLE)
    }

    /// The first page of each set; a query is stopped one row past it, a write runs to its
    /// end (the rows past the page dropped).
    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let write = !self.conn.prepare(statement).map_err(error)?.readonly();
        let overflow = if write { Overflow::Drop } else { Overflow::Stop };
        let mut limited = Limited::new(sink, options.fetch_limit, overflow);
        let outcome = self.execute(statement, &mut limited, cancel)?;
        Ok(RunOutcome {
            affected: outcome.affected,
            rows: limited.delivered,
            truncated: limited.truncated,
            more: None,
            stopped: limited.stopped,
            notices: Vec::new(),
        })
    }

    fn fetch(
        &mut self,
        _cursor: &CursorToken,
        _rows: u32,
        _sink: &mut dyn RowSink,
        _cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        Err(DbError::Unsupported("SQLite keeps no cursor: the query runs again".to_owned()))
    }

    /// The profile's projection: `EXPLAIN QUERY PLAN` as a tree, or the statement run and
    /// measured (`analyze`: a write undone).
    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        let depth =
            if analyze { ProfileDepth::Actual { timing: true } } else { ProfileDepth::Estimated };
        Ok(self.profile(statement, &ProfileOptions::of(depth), cancel)?.plan_nodes())
    }

    /// The plan, normalized; run, measured on this connection ([`profile::measure`]).
    fn profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        if options.depth.runs() {
            let timing = options.depth == (ProfileDepth::Actual { timing: true });
            return profile::measure(
                &mut self.conn,
                statement,
                options.timeout,
                timing,
                options.max_raw_bytes,
                cancel,
            );
        }
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = statement.trim().trim_end_matches(';').trim_end();
        let rows = profile::query_plan(&self.conn, sql)?;
        let (mut p, _) = profile::from_rows(&rows, ProfileDepth::Estimated);
        p.engine = profile::engine();
        p.collected = database::ProfileCaps { estimated: true, ..database::ProfileCaps::NONE };
        p.keep_raw(RawFormat::Rows, &profile::rows_text(&rows), options.max_raw_bytes);
        Ok(p)
    }

    fn table_stats(
        &mut self,
        tables: &[TableRef],
        cancel: &Cancel,
    ) -> Result<Vec<TableStats>, DbError> {
        profile::table_stats(&self.conn, tables, cancel)
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        if options.read_only {
            return Err(DbError::Unsupported(
                "read-only transactions in SQLite (a read-only source's file is opened so)"
                    .to_owned(),
            ));
        }
        let begin = match options.isolation {
            None => "BEGIN".to_owned(),
            Some(kind) if CONSOLE.isolation.contains(kind) => format!("BEGIN {}", kind.sql()),
            Some(other) => {
                return Err(DbError::Unsupported(format!("{} in SQLite", other.id())));
            }
        };
        self.conn.execute_batch(&begin).map_err(error)
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.cursors.clear();
        self.conn.execute_batch("COMMIT").map_err(error)
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        self.cursors.clear();
        self.conn.execute_batch("ROLLBACK").map_err(error)
    }

    fn in_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }

    /// Only what the console names: SQLite looks for an unqualified name in `temp`, `main`,
    /// then the attached files, whatever is chosen.
    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        let found = catalog::schemas(&self.conn)?
            .into_iter()
            .find(|s| s.eq_ignore_ascii_case(schema))
            .ok_or_else(|| DbError::Query(format!("no schema {schema}")))?;
        self.schema = Some(found);
        Ok(())
    }

    fn current(&mut self) -> Result<Place, DbError> {
        let schema = self.schema.clone().unwrap_or_else(|| catalog::MAIN.to_owned());
        Ok(Place { database: None, schema: Some(schema) })
    }

    /// The main file and the attached ones, by their schemas' names.
    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(catalog::schemas(&self.conn)?.into_iter().filter(|s| s != "temp").collect())
    }

    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        let schemas = match schema {
            Some(schema) => vec![schema.to_owned()],
            None => catalog::schemas(&self.conn)?,
        };
        let mut all = Vec::new();
        for schema in schemas {
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            all.extend(catalog::catalog_columns(&self.conn, &schema)?);
        }
        Ok(all)
    }

    fn catalog_keys(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        let schemas = match schema {
            Some(schema) => vec![schema.to_owned()],
            None => catalog::schemas(&self.conn)?,
        };
        let mut all = Vec::new();
        for schema in schemas {
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            all.extend(catalog::catalog_keys(&self.conn, &schema)?);
        }
        Ok(all)
    }

    fn routines(
        &mut self,
        _schema: Option<&str>,
        _cancel: &Cancel,
    ) -> Result<Vec<database::Routine>, DbError> {
        catalog::routines(&self.conn)
    }

    fn rebuild_source(&mut self, table: &TableRef) -> Result<RebuildSource, DbError> {
        catalog::rebuild_source(&self.conn, table)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
