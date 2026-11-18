//! An external driver as an ordinary [`Driver`]: what it is and can do comes from its
//! description (its manifest: nothing is started to list it, show its connection's fields
//! or tell its files), and each session is its program, started when the session opens
//! (the handshake, then `connect` with the password, only on the pipe) and told to `exit`
//! (stopped if it doesn't) when the session closes — the app's idle closing included.
//! The rest of the app doesn't know the driver is external.
//!
//! A driver of protocol 2 has a console of its own: `run` with its rows streamed
//! (`$/rows`), cursors, plans, transactions, the schema, the catalog in bulk, errors with
//! their place; one of protocol 3 its profiles too (`profile`, `table_stats`: one of 2 gives its
//! plans as profiles). One of the first version still works, with a limited console: its
//! statements through `execute` (at most [`EXECUTE_ROWS`] rows, whole, no place in its
//! errors), transactions by its dialect's words ([`ConsoleCaps::limited`] says so).

use std::sync::Arc;
use std::time::Duration;

use database::profile::{Profile, ProfileOptions, TableStats};
use database::run as console;
use database::{
    Applied, Cancel, Capabilities, ChangeSet, ConnectionSpec, ConsoleCaps, CursorToken, DbError,
    Dialect, Driver, DriverFailure, Filter, Flow, Kind, Language, Node, Outcome, Page, Place,
    PlanNode, Routine, RowSink, RunOptions, RunOutcome, SchemaChange, Script, Secret, Session,
    Structure, TableColumns, TableInfo, TableRef, Target, TxOptions,
};

use crate::client::{Connection, Transport};
use crate::convert::language;
use crate::dto;
use crate::wire::method;

/// The protocol's versions this editor speaks.
pub const VERSIONS: dto::Versions = dto::Versions { min: 1, max: 3 };
/// The first version with the console's methods.
pub const CONSOLE_VERSION: u32 = 2;
/// The first version with profiles (`profile`, `table_stats`); before it, a profile is the
/// driver's plan ([`database::profile::by_explain`]).
pub const PROFILE_VERSION: u32 = 3;

/// How long a driver has to answer the handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a request may take by default (a slow query is cancelled by the user first).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
/// The most rows a statement sends back (a check, a count, the keys of rows inserted:
/// never the whole of a large result).
pub const EXECUTE_ROWS: u64 = 100_000;
/// The most rows a `$/rows` notification brings…
pub const CHUNK_ROWS: usize = 1000;
/// …and about the most bytes (a chunk ends once past it).
pub const CHUNK_BYTES: usize = 1024 * 1024;

/// The console a driver speaking `protocol` has, by what it says it can (`capabilities`)
/// and whether the editor writes its SQL (`sql`): one of protocol 2 as it says (or the
/// basic one, when it writes SQL and says nothing); one of the first, limited.
pub fn console_of(
    protocol: u32,
    capabilities: &dto::Capabilities,
    sql: dto::Sql,
) -> Option<ConsoleCaps> {
    let declared = capabilities.console.clone().map(ConsoleCaps::from);
    let any = declared.is_some() || sql != dto::Sql::None;
    if protocol < CONSOLE_VERSION {
        return any.then_some(ConsoleCaps::LIMITED);
    }
    let mut console = declared.or(any.then_some(ConsoleCaps::BASIC))?;
    // Before protocol 3, its profiles are its plans, whatever it says.
    if protocol < PROFILE_VERSION {
        console.profile = database::ProfileCaps::from_explain(console.explain);
    }
    Some(console)
}

/// Starts a driver: its program (or, in the tests, a driver in this process).
pub trait Launch: Send + Sync {
    fn launch(&self) -> Result<Transport, DbError>;
}

/// A driver speaking the protocol (see the module's doc).
#[derive(Clone)]
pub struct ProtocolDriver {
    description: Arc<dto::Description>,
    /// The protocol's versions its manifest says it speaks.
    speaks: dto::Versions,
    timeout: Duration,
    launch: Arc<dyn Launch>,
}

impl std::fmt::Debug for ProtocolDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolDriver").field("id", &self.description.id).finish_non_exhaustive()
    }
}

/// The dialect `sql` names (`version`: the server's, when the dialect depends on it).
pub fn dialect(sql: dto::Sql, version: Option<u32>) -> Option<Arc<dyn Dialect>> {
    match sql {
        dto::Sql::None => None,
        dto::Sql::Generic => Some(Arc::new(database::GenericSql)),
        dto::Sql::Sqlite => Some(Arc::new(database::sqlite::Sqlite)),
        dto::Sql::Postgres => Some(Arc::new(database::postgres::Postgres { version })),
    }
}

/// The connection's fields `description` says, or a server's (a file's, when file-based).
pub fn connection(description: &dto::Description) -> ConnectionSpec {
    if let Some(connection) = &description.connection {
        return connection.clone().into();
    }
    if description.capabilities.file_based {
        let extensions: Vec<&str> = description
            .files
            .iter()
            .flat_map(|f| f.extensions.iter().map(String::as_str))
            .collect();
        ConnectionSpec::file(&extensions, &description.id)
    } else {
        ConnectionSpec::server(description.default_port.unwrap_or(0), &[])
    }
}

impl ProtocolDriver {
    /// The driver `description` says, speaking versions `speaks`, started by `launch`; a
    /// request waits `timeout` at most.
    pub fn new(
        description: Arc<dto::Description>,
        speaks: dto::Versions,
        timeout: Duration,
        launch: Arc<dyn Launch>,
    ) -> Self {
        Self { description, speaks, timeout, launch }
    }

    /// A session: the driver's program started, the handshake (no version in common:
    /// refused, with both ranges), then `connect` with the password, only on the pipe.
    pub fn open(&self, target: &Target, secret: &Secret) -> Result<ProtocolSession, DbError> {
        let name = self.description.name.clone();
        // A version its manifest says it can't speak: refused before anything starts.
        if VERSIONS.common(self.speaks).is_none() {
            return Err(self.refused(self.speaks.pair()));
        }
        let connection = Connection::new(&name, self.launch.launch()?)?;
        let ours = VERSIONS.pair();
        let initialize =
            dto::Initialize { protocol: VERSIONS, editor: kernel::brand::version().to_owned() };
        let hello: dto::Initialized =
            connection.call(method::INITIALIZE, &initialize, None, HANDSHAKE_TIMEOUT, ours)?;
        if !VERSIONS.contains(hello.protocol) {
            return Err(self.refused((hello.protocol, hello.protocol)));
        }
        if hello.driver.id != self.description.id {
            return Err(DbError::Driver(DriverFailure::NotStarted {
                driver: name,
                why: format!("it says it's the driver `{}`", hello.driver.id),
            }));
        }
        // Who writes the SQL is what the previews were made by (its manifest's): the
        // same, or nothing runs.
        if hello.driver.sql != self.description.sql {
            return Err(DbError::Driver(DriverFailure::NotStarted {
                driver: name,
                why: format!(
                    "its manifest says sql = {:?}, the driver {:?}",
                    self.description.sql, hello.driver.sql
                ),
            }));
        }
        // The password: hidden from its error output, then sent (only on the pipe).
        connection.hide(secret);
        let connect = dto::Connect { target: target.into(), password: secret.expose().to_owned() };
        let connected: dto::Connected =
            connection.call(method::CONNECT, &connect, None, self.timeout, ours)?;
        drop(connect);
        Ok(ProtocolSession {
            dialect: dialect(hello.driver.sql, connected.version),
            console: console_of(hello.protocol, &hello.driver.capabilities, hello.driver.sql),
            protocol: hello.protocol,
            in_tx: false,
            capabilities: hello.driver.capabilities.into(),
            sql: hello.driver.sql,
            server: connected.server,
            timeout: self.timeout,
            connection,
        })
    }

    fn refused(&self, theirs: (u32, u32)) -> DbError {
        DbError::Driver(DriverFailure::Protocol {
            driver: self.description.name.clone(),
            theirs,
            ours: VERSIONS.pair(),
        })
    }
}

impl Driver for ProtocolDriver {
    fn kind(&self) -> Kind {
        self.description.kind.into()
    }

    fn language(&self) -> Language {
        language(self.description.kind)
    }

    /// As its manifest says; its console by the version it will speak.
    fn capabilities(&self) -> Capabilities {
        let mut capabilities: Capabilities = self.description.capabilities.clone().into();
        let protocol = VERSIONS.common(self.speaks).unwrap_or(1);
        capabilities.console =
            console_of(protocol, &self.description.capabilities, self.description.sql);
        capabilities
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        dialect(self.description.sql, None)
    }

    fn connection(&self) -> ConnectionSpec {
        connection(&self.description)
    }

    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        Ok(Box::new(self.open(target, secret)?))
    }
}

/// A session of an external driver: its program, until the session is dropped.
pub struct ProtocolSession {
    connection: Connection,
    server: String,
    dialect: Option<Arc<dyn Dialect>>,
    /// What the driver said it does at the handshake: nothing else is asked of it.
    capabilities: Capabilities,
    sql: dto::Sql,
    timeout: Duration,
    /// The version spoken (chosen at the handshake).
    protocol: u32,
    console: Option<ConsoleCaps>,
    /// A transaction is open (as the driver says after a run, or by `begin`).
    in_tx: bool,
}

impl std::fmt::Debug for ProtocolSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolSession")
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl ProtocolSession {
    fn call<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        params: &P,
        cancel: Option<&Cancel>,
    ) -> Result<R, DbError> {
        self.connection.call(name, params, cancel, self.timeout, VERSIONS.pair())
    }

    /// The last lines of its program's error output (its secrets taken out).
    pub fn log(&self) -> Vec<String> {
        self.connection.log()
    }

    /// The protocol's version it speaks.
    pub fn protocol(&self) -> u32 {
        self.protocol
    }

    fn console_methods(&self) -> bool {
        self.protocol >= CONSOLE_VERSION
    }

    /// `run` or `fetch` (`method`, `params`): its rows to `sink` as they come, the outcome.
    fn streamed<P: serde::Serialize>(
        &mut self,
        method: &str,
        params: &P,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let mut delivered = 0u64;
        let mut stopped = false;
        let (answer, enough) = self.connection.call_streaming::<_, dto::RunOutcome>(
            method,
            params,
            Some(cancel),
            self.timeout,
            VERSIONS.pair(),
            &mut |chunk| {
                if let Some(columns) = chunk.columns {
                    let columns: Vec<database::Column> =
                        columns.into_iter().map(Into::into).collect();
                    sink.columns(&columns);
                }
                for row in chunk.rows {
                    delivered += 1;
                    if sink.row(row.into_iter().map(Into::into).collect()) == Flow::Stop {
                        stopped = true;
                        return Flow::Stop;
                    }
                }
                Flow::Continue
            },
        )?;
        let Some(answer) = answer else {
            return Ok(RunOutcome { rows: delivered, stopped: true, ..RunOutcome::default() });
        };
        if let Some(open) = answer.in_transaction {
            self.in_tx = open;
        }
        let mut outcome: RunOutcome = answer.into();
        if enough || stopped {
            outcome.stopped = true;
            outcome.rows = delivered;
        }
        Ok(outcome)
    }
}

/// Feeds `rows` to `sink` (each row's key first, when there are keys): how it went.
fn feed(rows: dto::Rows, sink: &mut dyn RowSink) -> Outcome {
    let columns: Vec<database::Column> = rows.columns.into_iter().map(Into::into).collect();
    sink.columns(&columns);
    let mut keys = rows.keys.into_iter();
    let mut outcome = Outcome { affected: rows.affected, rows: 0, stopped: rows.stopped };
    for row in rows.rows {
        if let Some(key) = keys.next() {
            sink.row_key(key.into());
        }
        outcome.rows += 1;
        if sink.row(row.into_iter().map(Into::into).collect()) == database::Flow::Stop {
            outcome.stopped = true;
            break;
        }
    }
    outcome
}

impl Session for ProtocolSession {
    fn server(&self) -> String {
        self.server.clone()
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        self.dialect.clone()
    }

    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        let nodes: Vec<dto::Node> =
            self.call(method::CHILDREN, &dto::PathParams { path: path.to_vec() }, None)?;
        Ok(nodes.into_iter().map(Into::into).collect())
    }

    fn execute(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let asked = dto::Execute { sql: query.to_owned(), max_rows: Some(EXECUTE_ROWS) };
        let rows: dto::Rows = self.call(method::EXECUTE, &asked, Some(cancel))?;
        Ok(feed(rows, sink))
    }

    fn console(&self) -> Option<ConsoleCaps> {
        self.console
    }

    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        if !self.console_methods() {
            let outcome = console::run_by_execute(self, statement, options, sink, cancel)?;
            let flavor = self.dialect.as_ref().map(|d| d.flavor()).unwrap_or_default();
            self.in_tx = console::transaction_after(statement, flavor, self.in_tx);
            return Ok(outcome);
        }
        let asked = dto::Run {
            sql: statement.to_owned(),
            fetch_limit: options.fetch_limit,
            keep_cursor: options.keep_cursor,
        };
        self.streamed(method::RUN, &asked, sink, cancel)
    }

    fn fetch(
        &mut self,
        cursor: &CursorToken,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        if !self.console_methods() {
            return Err(DbError::Unsupported("cursors with a driver of protocol 1".to_owned()));
        }
        let asked = dto::Fetch { cursor: cursor.0.clone(), rows };
        self.streamed(method::FETCH, &asked, sink, cancel)
    }

    fn close_cursor(&mut self, cursor: &CursorToken) -> Result<(), DbError> {
        if !self.console_methods() {
            return Ok(());
        }
        self.call(method::CLOSE_CURSOR, &dto::CursorParams { cursor: cursor.0.clone() }, None)
    }

    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        if !self.console_methods() {
            return Err(DbError::Unsupported("plans with a driver of protocol 1".to_owned()));
        }
        let asked = dto::Explain { sql: statement.to_owned(), analyze };
        let plan: Vec<dto::PlanNode> = self.call(method::EXPLAIN, &asked, Some(cancel))?;
        Ok(plan.into_iter().map(Into::into).collect())
    }

    /// Protocol 3: the driver's own; before it, its plan as a profile.
    fn profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        if self.protocol < PROFILE_VERSION {
            return database::profile::by_explain(self, statement, options, cancel);
        }
        let asked = dto::ProfileParams { sql: statement.to_owned(), options: options.into() };
        let answer: dto::Profile = self.call(method::PROFILE, &asked, Some(cancel))?;
        Ok(crate::convert::profile_from(answer))
    }

    fn table_stats(
        &mut self,
        tables: &[TableRef],
        cancel: &Cancel,
    ) -> Result<Vec<TableStats>, DbError> {
        if self.protocol < PROFILE_VERSION {
            return Err(DbError::Unsupported(
                "tables' statistics with a driver of protocol 2".to_owned(),
            ));
        }
        let asked =
            dto::TableStatsParams { tables: tables.iter().cloned().map(Into::into).collect() };
        let answer: Vec<dto::TableStats> = self.call(method::TABLE_STATS, &asked, Some(cancel))?;
        Ok(answer.into_iter().map(Into::into).collect())
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        if self.console_methods() {
            self.call::<_, ()>(method::BEGIN, &dto::Begin::from(options), None)?;
        } else {
            console::begin_by_execute(self, options)?;
        }
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.in_tx = false;
        if self.console_methods() {
            self.call(method::COMMIT, &serde_json::json!({}), None)
        } else {
            console::commit_by_execute(self)
        }
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        self.in_tx = false;
        if self.console_methods() {
            self.call(method::ROLLBACK, &serde_json::json!({}), None)
        } else {
            console::rollback_by_execute(self)
        }
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        if !self.console_methods() {
            return Err(DbError::Unsupported("changing the schema with protocol 1".to_owned()));
        }
        self.call(method::SET_SCHEMA, &dto::SchemaParams { schema: schema.to_owned() }, None)
    }

    fn current(&mut self) -> Result<Place, DbError> {
        if !self.console_methods() {
            return Ok(Place::default());
        }
        let place: dto::Place = self.call(method::CURRENT, &serde_json::json!({}), None)?;
        Ok(place.into())
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        if !self.console_methods() {
            return Ok(Vec::new());
        }
        self.call(method::DATABASES, &serde_json::json!({}), None)
    }

    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        if !self.console_methods() {
            return console::catalog_columns_by_structure(self, schema, cancel);
        }
        let asked = dto::CatalogParams { schema: schema.map(str::to_owned) };
        let all: Vec<dto::TableColumns> =
            self.call(method::CATALOG_COLUMNS, &asked, Some(cancel))?;
        Ok(all.into_iter().map(Into::into).collect())
    }

    fn routines(&mut self, schema: Option<&str>, cancel: &Cancel) -> Result<Vec<Routine>, DbError> {
        if !self.console_methods() {
            return Err(DbError::Unsupported("listing routines with protocol 1".to_owned()));
        }
        let asked = dto::CatalogParams { schema: schema.map(str::to_owned) };
        let all: Vec<dto::Routine> = self.call(method::ROUTINES, &asked, Some(cancel))?;
        Ok(all.into_iter().map(Into::into).collect())
    }

    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        let tables: Vec<dto::TableInfo> =
            self.call(method::TABLES, &dto::PathParams { path: path.to_vec() }, None)?;
        Ok(tables.into_iter().map(Into::into).collect())
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        let structure: dto::Structure =
            self.call(method::STRUCTURE, &dto::TableParams { table: table.into() }, None)?;
        Ok(structure.into())
    }

    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let asked = dto::Read { table: table.into(), page: page.clone().into() };
        let rows: dto::Rows = self.call(method::READ, &asked, Some(cancel))?;
        Ok(feed(rows, sink))
    }

    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, DbError> {
        if !self.capabilities.edit_rows {
            return Err(DbError::ReadOnly("the driver doesn't edit rows".to_owned()));
        }
        let applied: dto::Applied =
            self.call(method::APPLY, &dto::Apply { changes: changes.clone().into() }, None)?;
        Ok(applied.into())
    }

    fn alter(&mut self, changes: &[SchemaChange]) -> Result<Applied, DbError> {
        allowed(self.capabilities, changes)?;
        let applied: dto::Applied = self.call(method::ALTER, &alter_params(changes), None)?;
        Ok(applied.into())
    }

    fn plan_alter(&mut self, changes: &[SchemaChange]) -> Result<Option<Script>, DbError> {
        // A driver the editor writes the SQL for plans nothing of its own.
        if self.sql != dto::Sql::None {
            return Ok(None);
        }
        allowed(self.capabilities, changes)?;
        let plan: Option<dto::Script> =
            self.call(method::PLAN_ALTER, &alter_params(changes), None)?;
        Ok(plan.map(Into::into))
    }

    fn count(
        &mut self,
        table: &TableRef,
        filter: Option<&Filter>,
        cancel: &Cancel,
    ) -> Result<u64, DbError> {
        let asked = dto::Count { table: table.into(), filter: filter.cloned().map(Into::into) };
        self.call(method::COUNT, &asked, Some(cancel))
    }

    fn rebuild_source(
        &mut self,
        table: &TableRef,
    ) -> Result<database::sqlite::RebuildSource, DbError> {
        let source: dto::RebuildSource =
            self.call(method::REBUILD_SOURCE, &dto::TableParams { table: table.into() }, None)?;
        Ok(source.into())
    }
}

fn alter_params(changes: &[SchemaChange]) -> dto::Alter {
    dto::Alter { changes: changes.iter().cloned().map(Into::into).collect() }
}

/// Whether the driver said it makes each of `changes`.
fn allowed(capabilities: Capabilities, changes: &[SchemaChange]) -> Result<(), DbError> {
    match changes.iter().find(|c| !capabilities.schema_changes.allows(c)) {
        Some(change) => Err(DbError::Unsupported(format!("{change:?} with this driver"))),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "../tests/unit/driver.rs"]
mod tests;
