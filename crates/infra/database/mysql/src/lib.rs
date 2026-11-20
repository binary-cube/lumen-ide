//! MySQL and MariaDB for the `database` domain, one driver for every server from MySQL 5.7
//! and MariaDB 10.2 to the newest: the protocol is the same, the sign-in is whatever the
//! server asks (`caching_sha2_password` — over a plain connection through the server's RSA
//! key —, `mysql_native_password`, MariaDB's `ed25519` aside), and what differs — the
//! catalog's columns, the SQL a version takes, its plans — is read from `VERSION()` when it
//! connects ([`database::mysql`]: its features, its dialect). A session owns a small
//! single-threaded runtime of its own (made when it connects: nothing runs before) driving
//! `mysql_async` (pure Rust, rustls). Queries go through the text protocol, streamed to the
//! sink and typed by their columns' metadata; cancelling kills the statement from a second,
//! short-lived connection (`KILL QUERY <id>`), as does a sink that has enough of a query's
//! rows (the rest drained, the connection kept).
//!
//! TLS goes by `sslmode` (the data sources' six steps, `prefer` by default: [`tls`]).
//! Tables are read a page at a time, by their primary key when no other order is asked
//! (the next page starts after the last key read, no growing `OFFSET`), and changed by the
//! domain's transactions in the version's dialect (`CLIENT_FOUND_ROWS`: an update counts
//! the rows it found, so one setting a value already there is no conflict). The explorer:
//! schemas (MySQL's databases: one session reads them all) › tables and views › columns
//! and indexes.
//!
//! For the console ([`console`]): no server-side cursor (a query's next page is run again
//! from the row after), the server's warnings come with the outcome (`SHOW WARNINGS`),
//! errors keep their SQLSTATE, code and place (MySQL's `near '…' at line N` found in the
//! text), the transaction's state is the server's own (`SERVER_STATUS_IN_TRANS`: a DDL's
//! implicit commit ends it), transactions begin with their isolation, the schema is `USE`,
//! and the catalog is read in bulk from `information_schema`. Profiles ([`profile`]):
//! `EXPLAIN FORMAT=JSON` for the plan, MySQL 8.0.18's `EXPLAIN ANALYZE` and MariaDB's
//! `ANALYZE FORMAT=JSON` as it ran — in a read-only transaction of its own, a write's undone
//! (a savepoint inside the user's), refused on tables that can't be rolled back —, with the
//! session's counters before and after and `performance_schema`'s history of the
//! statement; the tables' statistics from `information_schema` and `performance_schema`
//! ([`stats`]). A read-only source's sessions are `SET SESSION TRANSACTION READ ONLY`.

pub mod catalog;
pub mod console;
pub mod profile;
pub mod stats;
pub mod tls;

use std::sync::Arc;
use std::time::{Duration, Instant};

use database::fields::{Binding, ConnectionSpec, FieldKind, FieldSpec, FieldTab};
use database::mysql::{MySql as MyDialect, ServerVersion, quote_backtick};
use database::profile::{Profile, ProfileDepth, ProfileOptions};
use database::run::{Limited, Overflow};
use database::sql::StatementKind;
use database::tls::{SSLCERT, SSLKEY, SSLMODE, SSLROOTCERT};
use database::{
    Cancel, Capabilities, ColumnBrief, ColumnInfo, ConsoleCaps, DbError, Dialect, Driver,
    DriverFactory, Filter, Flow, ForeignKey, Generated, IndexInfo, Kind, Language, Node, Notice,
    Outcome, Page, Place, PlanNode, Routine, RoutineKind, RowKey, RowSink, RunOptions, RunOutcome,
    SchemaChanges, Secret, Session, Sorting, SslMode, Structure, TableColumns, TableInfo,
    TableKind, TableRef, Target, TxOptions, Value,
};
use mysql_async::consts::{ColumnFlags, ColumnType, StatusFlags};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder};

use crate::tls::Attempt;

/// The driver's id (MySQL's servers; MariaDB's have their own, the same driver).
pub const ID: &str = "mysql";
/// MariaDB's id in the data sources' list.
pub const MARIADB_ID: &str = "mariadb";

/// The driver as the app lists it for MySQL (created only when a connection opens).
pub fn factory() -> DriverFactory {
    DriverFactory::new(ID, "MySQL", Kind::Relational, Some(3306), || Arc::new(MySqlDriver))
}

/// The same driver, listed for MariaDB.
pub fn mariadb_factory() -> DriverFactory {
    DriverFactory::new(MARIADB_ID, "MariaDB", Kind::Relational, Some(3306), || {
        Arc::new(MySqlDriver)
    })
}

/// How long a connection may take (unless the source's `connect_timeout` says).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a running statement looks whether it was cancelled.
const CANCEL_CHECK: Duration = Duration::from_millis(50);
/// How long the connection that kills a statement may take.
const KILL_TIMEOUT: Duration = Duration::from_secs(5);
/// The places remembered to start a table's next page after its last key.
const CURSORS: usize = 8;
/// The most warnings kept from one statement.
const WARNINGS: usize = 1000;

/// The connection's option its field sets (the others go nowhere: no engine option is
/// passed blindly).
pub const CONNECT_TIMEOUT_OPTION: &str = "connect_timeout";

#[derive(Debug)]
pub struct MySqlDriver;

/// What the driver can do (every supported version: the same; a session's console says
/// its own server's).
pub fn capabilities() -> Capabilities {
    Capabilities {
        transactions: true,
        edit_rows: true,
        schema_changes: SchemaChanges { reorder: false, ..SchemaChanges::ALL },
        schemas: true,
        file_based: false,
        cancel: true,
        returning: false,
        sorting: Sorting::Any,
        console: Some(console::CONSOLE),
    }
}

/// Its connection's fields: a server's, then TLS (`sslmode` and its files) and its options.
pub fn connection() -> ConnectionSpec {
    let modes = SslMode::ALL
        .iter()
        .map(|m| {
            (m.as_str().to_owned(), format!("datasources.sslmode_{}", m.as_str().replace('-', "_")))
        })
        .collect();
    let pem = || FieldKind::File { extensions: vec!["pem".into(), "crt".into(), "key".into()] };
    let option = |name: &str| Binding::Option(name.to_owned());
    ConnectionSpec::server(3306, &["mysql", "mariadb"])
        .with(
            FieldSpec::new(
                "sslmode",
                "datasources.field_sslmode",
                FieldKind::Choice(modes),
                option(SSLMODE),
                FieldTab::Ssl,
            )
            .with_default(SslMode::Prefer.as_str()),
        )
        .with(FieldSpec::new(
            "sslrootcert",
            "datasources.field_sslrootcert",
            pem(),
            option(SSLROOTCERT),
            FieldTab::Ssl,
        ))
        .with(FieldSpec::new(
            "sslcert",
            "datasources.field_sslcert",
            pem(),
            option(SSLCERT),
            FieldTab::Ssl,
        ))
        .with(FieldSpec::new(
            "sslkey",
            "datasources.field_sslkey",
            pem(),
            option(SSLKEY),
            FieldTab::Ssl,
        ))
        .with(
            FieldSpec::new(
                "connect_timeout",
                "datasources.field_connect_timeout",
                FieldKind::Number { min: 1, max: 600 },
                option(CONNECT_TIMEOUT_OPTION),
                FieldTab::Options,
            )
            .with_default(CONNECT_TIMEOUT.as_secs().to_string()),
        )
}

impl Driver for MySqlDriver {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    /// Before it connects: what every supported version takes (the session gives its own).
    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        Some(Arc::new(MyDialect::default()))
    }

    fn connection(&self) -> ConnectionSpec {
        connection()
    }

    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        let mode = SslMode::of(target).map_err(DbError::Connect)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| DbError::Connect(e.to_string()))?;
        let timeout = target
            .options
            .get(CONNECT_TIMEOUT_OPTION)
            .and_then(|t| t.trim().parse::<u64>().ok())
            .filter(|t| *t > 0)
            .map_or(CONNECT_TIMEOUT, Duration::from_secs);
        let password = secret.expose();
        let base = OptsBuilder::default()
            .ip_or_hostname(if target.host.is_empty() { "localhost" } else { &target.host })
            .tcp_port(target.port.unwrap_or(3306))
            .user(Some(target.user.clone()))
            .pass((!password.is_empty()).then(|| password.to_owned()))
            .db_name((!target.database.is_empty()).then(|| target.database.clone()))
            // An update's count is the rows it found (the grid's "exactly one row").
            .client_found_rows(true)
            .prefer_socket(false)
            .stmt_cache_size(0)
            .connect_attribute("program_name", "editor");
        let has_root = tls::file_option(target, SSLROOTCERT).is_some();
        let mut last = None;
        let mut opened = None;
        for attempt in tls::attempts(mode, has_root) {
            let opts: Opts = match attempt {
                Attempt::Plain => base.clone().ssl_opts(None::<mysql_async::SslOpts>).into(),
                Attempt::TlsOrPlain(check) | Attempt::Tls(check) => {
                    base.clone().ssl_opts(tls::ssl_opts(target, check)?).into()
                }
            };
            let connected = runtime.block_on(async {
                match tokio::time::timeout(timeout, Conn::new(opts.clone())).await {
                    Ok(result) => result,
                    Err(_) => Err(mysql_async::Error::Io(mysql_async::IoError::Io(
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "connecting timed out"),
                    ))),
                }
            });
            match connected {
                Ok(conn) => {
                    opened = Some((conn, opts));
                    break;
                }
                // `prefer`: a server without TLS (or whose TLS can't be agreed on: an old
                // server's ciphers) is connected to plainly, as libpq does.
                Err(
                    mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer)
                    | mysql_async::Error::Io(_),
                ) if matches!(attempt, Attempt::TlsOrPlain(_)) => {
                    let plain: Opts = base.ssl_opts(None::<mysql_async::SslOpts>).into();
                    match runtime.block_on(async {
                        tokio::time::timeout(timeout, Conn::new(plain.clone())).await
                    }) {
                        Ok(Ok(conn)) => {
                            opened = Some((conn, plain));
                            break;
                        }
                        Ok(Err(e)) => last = Some(refused(e)),
                        Err(_) => last = Some(DbError::Connect("connecting timed out".into())),
                    }
                    break;
                }
                // `allow`: plain refused (the server wants TLS: 3159), so TLS next.
                Err(mysql_async::Error::Server(e)) if e.code == SECURE_TRANSPORT_REQUIRED => {
                    last = Some(refused(mysql_async::Error::Server(e)));
                }
                Err(e) => {
                    last = Some(refused(e));
                    break;
                }
            }
        }
        let Some((mut conn, opts)) = opened else {
            return Err(last.unwrap_or_else(|| DbError::Connect("no attempt".into())));
        };
        let version_text: String = runtime
            .block_on(conn.query_first::<String, _>("SELECT VERSION()"))
            .map_err(error)?
            .unwrap_or_default();
        let version = ServerVersion::parse(&version_text).unwrap_or_default();
        let server = format!("{} {}", version.engine(), version.text());
        // A read-only source writes nothing, whatever the app checks before.
        if target.read_only() {
            runtime.block_on(conn.query_drop(console::READ_ONLY_SESSION)).map_err(error)?;
        }
        let killer = Killer { opts, id: conn.id() };
        Ok(Box::new(MySession {
            conn: Some(conn),
            runtime,
            killer,
            version,
            server,
            in_tx: false,
            cursors: Vec::new(),
        }))
    }
}

/// The server refuses a plain connection (`require_secure_transport`).
const SECURE_TRANSPORT_REQUIRED: u16 = 3159;

/// A connection refused, as the domain says it.
fn refused(e: mysql_async::Error) -> DbError {
    error(e)
}

/// Whether a server's error code is a refused sign-in: a wrong password (1045), a database
/// the user may not open (1044), a plugin it can't use (1251, 1698), an account locked
/// (3118) or expired (1820, 1862).
fn auth_code(code: u16) -> bool {
    matches!(code, 1044 | 1045 | 1251 | 1698 | 3118 | 1820 | 1862 | 4151)
}

/// A failure, as the domain says it: the server's message when it gave one, with its code
/// and SQLSTATE.
fn error(e: mysql_async::Error) -> DbError {
    error_in(e, None)
}

/// A failure of `statement` (the text sent): its place found in it from the server's words.
fn error_in(e: mysql_async::Error, statement: Option<&str>) -> DbError {
    use mysql_async::{DriverError, Error};
    match e {
        Error::Server(s) if auth_code(s.code) => DbError::Auth(s.message),
        Error::Server(s) if s.code == console::INTERRUPTED => DbError::Cancelled,
        Error::Server(s) => {
            DbError::sql(console::sql_error(s.code, &s.state, &s.message, statement))
        }
        Error::Driver(DriverError::ConnectionClosed) => {
            DbError::Connect("the connection closed".to_owned())
        }
        Error::Driver(DriverError::UnknownAuthPlugin { name }) => {
            DbError::Auth(format!("the sign-in plugin `{name}` isn't supported"))
        }
        Error::Driver(DriverError::CleartextPluginDisabled) => DbError::Auth(
            "the server asks for the password in clear text (mysql_clear_password)".to_owned(),
        ),
        Error::Driver(DriverError::NoClientSslFlagFromServer) => {
            DbError::Connect("the server doesn't take TLS".to_owned())
        }
        Error::Io(e) => DbError::Connect(e.to_string()),
        other => DbError::Connect(other.to_string()),
    }
}

/// Kills a session's running statement from a connection of its own.
#[derive(Clone)]
struct Killer {
    opts: Opts,
    /// The session's `CONNECTION_ID()`.
    id: u32,
}

impl Killer {
    async fn kill(&self) {
        let Ok(Ok(mut conn)) =
            tokio::time::timeout(KILL_TIMEOUT, Conn::new(self.opts.clone())).await
        else {
            tracing::debug!("the connection to stop a statement didn't open");
            return;
        };
        if let Err(e) = conn.query_drop(format!("KILL QUERY {}", self.id)).await {
            tracing::debug!(error = %e, "the statement wasn't stopped");
        }
        let _ = conn.disconnect().await;
    }
}

struct MySession {
    /// Taken only when the session ends (closed politely, on its runtime).
    conn: Option<Conn>,
    runtime: tokio::runtime::Runtime,
    killer: Killer,
    version: ServerVersion,
    server: String,
    /// A transaction is open, as the server last said (`SERVER_STATUS_IN_TRANS`).
    in_tx: bool,
    /// Where a table's next page starts (after the last key read), the newest last.
    cursors: Vec<Cursor>,
}

impl Drop for MySession {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = self
                .runtime
                .block_on(async { tokio::time::timeout(KILL_TIMEOUT, conn.disconnect()).await });
        }
    }
}

/// A table read in its key's order: the page starting at `next` starts after `last`.
#[derive(Debug, Clone)]
struct Cursor {
    table: TableRef,
    filter: Option<Filter>,
    next: u64,
    last: Vec<Value>,
}

/// How a statement's stream takes a sink's stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnStop {
    /// The statement is killed (a query: nothing lost), the rest drained.
    Kill,
    /// The rest is drained only.
    Drain,
}

/// The type a column's metadata says, as MySQL writes it (`int unsigned`, `varchar`,
/// `json`).
pub fn type_name(column: &mysql_async::Column) -> String {
    let binary = column.character_set() == 63;
    let flags = column.flags();
    let base = match column.column_type() {
        ColumnType::MYSQL_TYPE_TINY => "tinyint",
        ColumnType::MYSQL_TYPE_SHORT => "smallint",
        ColumnType::MYSQL_TYPE_INT24 => "mediumint",
        ColumnType::MYSQL_TYPE_LONG => "int",
        ColumnType::MYSQL_TYPE_LONGLONG => "bigint",
        ColumnType::MYSQL_TYPE_FLOAT => "float",
        ColumnType::MYSQL_TYPE_DOUBLE => "double",
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "decimal",
        ColumnType::MYSQL_TYPE_YEAR => "year",
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "date",
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "time",
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "datetime",
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "timestamp",
        ColumnType::MYSQL_TYPE_BIT => "bit",
        ColumnType::MYSQL_TYPE_JSON => "json",
        ColumnType::MYSQL_TYPE_GEOMETRY => "geometry",
        ColumnType::MYSQL_TYPE_NULL => "null",
        ColumnType::MYSQL_TYPE_ENUM => "enum",
        ColumnType::MYSQL_TYPE_SET => "set",
        ColumnType::MYSQL_TYPE_VARCHAR | ColumnType::MYSQL_TYPE_VAR_STRING => {
            if flags.contains(ColumnFlags::ENUM_FLAG) {
                "enum"
            } else if flags.contains(ColumnFlags::SET_FLAG) {
                "set"
            } else if binary {
                "varbinary"
            } else {
                "varchar"
            }
        }
        ColumnType::MYSQL_TYPE_STRING => {
            if flags.contains(ColumnFlags::ENUM_FLAG) {
                "enum"
            } else if flags.contains(ColumnFlags::SET_FLAG) {
                "set"
            } else if binary {
                "binary"
            } else {
                "char"
            }
        }
        ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_BLOB => {
            if binary {
                "blob"
            } else {
                "text"
            }
        }
        _ => "",
    };
    let numeric = matches!(
        column.column_type(),
        ColumnType::MYSQL_TYPE_TINY
            | ColumnType::MYSQL_TYPE_SHORT
            | ColumnType::MYSQL_TYPE_INT24
            | ColumnType::MYSQL_TYPE_LONG
            | ColumnType::MYSQL_TYPE_LONGLONG
            | ColumnType::MYSQL_TYPE_DECIMAL
            | ColumnType::MYSQL_TYPE_NEWDECIMAL
            | ColumnType::MYSQL_TYPE_FLOAT
            | ColumnType::MYSQL_TYPE_DOUBLE
    );
    if numeric && flags.contains(ColumnFlags::UNSIGNED_FLAG) {
        format!("{base} unsigned")
    } else {
        base.to_owned()
    }
}

/// A value of a column of `type_name` (as [`type_name`] says it), from its text (`None`:
/// null).
pub fn value(type_name: &str, bytes: Option<&[u8]>) -> Value {
    let Some(bytes) = bytes else { return Value::Null };
    let text = || String::from_utf8_lossy(bytes).into_owned();
    let unsigned = type_name.ends_with(" unsigned");
    let base = type_name.trim_end_matches(" unsigned");
    match base {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" | "year" => {
            match text().parse::<i64>() {
                Ok(n) => Value::Int(n),
                // A `bigint unsigned` past i64: as written.
                Err(_) => Value::Other(text()),
            }
        }
        "float" | "double" => text().parse().map_or_else(|_| Value::Other(text()), Value::Float),
        "json" => Value::Json(text()),
        "varchar" | "char" | "text" => Value::Text(text()),
        "binary" | "varbinary" | "blob" | "geometry" => Value::Bytes(bytes.to_vec()),
        // `bit(n)`: its bytes, big-endian, as a number.
        "bit" if bytes.len() <= 8 => {
            Value::Int(bytes.iter().fold(0i64, |n, b| (n << 8) | i64::from(*b)))
        }
        "null" => Value::Null,
        _ if unsigned => Value::Other(text()),
        // Dates, times, decimals, enums, sets: as written.
        _ => Value::Other(text()),
    }
}

/// A result's column, as the domain says it.
fn column_of(column: &mysql_async::Column) -> database::Column {
    database::Column { name: column.name_str().into_owned(), type_name: type_name(column) }
}

/// Gives each row its key (the primary key's columns found among the result's) before the
/// row, and keeps the last key read.
struct Keyed<'a> {
    sink: &'a mut dyn RowSink,
    key: Vec<String>,
    at: Vec<usize>,
    last: Option<Vec<Value>>,
}

impl RowSink for Keyed<'_> {
    fn columns(&mut self, columns: &[database::Column]) {
        self.at =
            self.key.iter().filter_map(|k| columns.iter().position(|c| c.name == *k)).collect();
        self.sink.columns(columns);
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        if !self.key.is_empty() && self.at.len() == self.key.len() {
            let key: Vec<Value> = self.at.iter().filter_map(|i| values.get(*i).cloned()).collect();
            self.sink.row_key(RowKey::Primary(self.key.iter().cloned().zip(key.clone()).collect()));
            self.last = Some(key);
        }
        self.sink.row(values)
    }
}

/// A routine by its schema and name.
type RoutineKey = (Option<String>, String);

/// A catalog query's rows, each value as text (`None`: null).
type TextRows = Vec<Vec<Option<String>>>;

/// The text of a value read through the text protocol.
fn text_of(value: Option<mysql_async::Value>) -> Option<String> {
    match value? {
        mysql_async::Value::NULL => None,
        mysql_async::Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        mysql_async::Value::Int(n) => Some(n.to_string()),
        mysql_async::Value::UInt(n) => Some(n.to_string()),
        mysql_async::Value::Float(n) => Some(n.to_string()),
        mysql_async::Value::Double(n) => Some(n.to_string()),
        other => Some(other.as_sql(true).trim_matches('\'').to_owned()),
    }
}

/// Runs `query` on `conn` through the text protocol, its rows to `sink` typed by their
/// columns. `cancel` (or `deadline` passing) kills it from another connection; a sink's
/// stop kills a query ([`OnStop::Kill`]) and drains the rest. Errors are placed in
/// `query`.
async fn stream(
    conn: &mut Conn,
    killer: &Killer,
    query: &str,
    sink: &mut dyn RowSink,
    cancel: &Cancel,
    deadline: Option<Instant>,
    on_stop: OnStop,
) -> Result<Outcome, DbError> {
    let (stop, stopped_rx) = tokio::sync::oneshot::channel::<()>();
    let watcher = {
        let cancel = cancel.clone();
        let killer = killer.clone();
        tokio::spawn(async move {
            let mut stopped_rx = stopped_rx;
            let mut tick = tokio::time::interval(CANCEL_CHECK);
            loop {
                tokio::select! {
                    _ = &mut stopped_rx => return false,
                    _ = tick.tick() => {
                        let late = deadline.is_some_and(|d| Instant::now() >= d);
                        if cancel.is_cancelled() || late {
                            killer.kill().await;
                            return true;
                        }
                    }
                }
            }
        })
    };
    let mut outcome = Outcome::default();
    let mut killed_for_stop = false;
    let ran = async {
        let mut result = conn.query_iter(query).await?;
        loop {
            let columns = result.columns().unwrap_or_default();
            let set: Vec<database::Column> = columns.iter().map(column_of).collect();
            if set.is_empty() {
                // A statement without rows: its count now (the next set's comes after).
                outcome.affected = Some(result.affected_rows());
            }
            sink.columns(&set);
            while let Some(row) = result.next().await? {
                if outcome.stopped {
                    continue;
                }
                let raw = row.unwrap_raw();
                let values = raw
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let type_name = set.get(i).map_or("", |c| c.type_name.as_str());
                        match v {
                            Some(mysql_async::Value::Bytes(b)) => value(type_name, Some(&b)),
                            Some(mysql_async::Value::NULL) | None => Value::Null,
                            Some(other) => {
                                value(type_name, text_of(Some(other)).as_deref().map(str::as_bytes))
                            }
                        }
                    })
                    .collect();
                outcome.rows += 1;
                if sink.row(values) == Flow::Stop {
                    outcome.stopped = true;
                    if on_stop == OnStop::Kill {
                        // Enough: the server stops working on the rest.
                        killed_for_stop = true;
                        killer.kill().await;
                    }
                }
            }
            if result.is_empty() {
                break;
            }
        }
        Ok::<(), mysql_async::Error>(())
    }
    .await;
    let _ = stop.send(());
    let killed = watcher.await.unwrap_or(false);
    // Stopped as asked, even if what it was doing ended meanwhile (`SLEEP` returns 1).
    if killed && cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    match ran {
        Ok(()) => Ok(outcome),
        // Killed because the sink had enough: what it got is the outcome.
        Err(mysql_async::Error::Server(e)) if killed_for_stop && e.code == console::INTERRUPTED => {
            Ok(outcome)
        }
        Err(mysql_async::Error::Server(e))
            if killed && matches!(e.code, console::INTERRUPTED | console::TIMED_OUT) =>
        {
            Err(DbError::Query("stopped: it ran past its time limit".to_owned()))
        }
        Err(e) => Err(error_in(e, Some(query))),
    }
}

impl MySession {
    fn dialect(&self) -> MyDialect {
        MyDialect::new(self.version)
    }

    /// `query` streamed to `sink` (see [`stream`]), the transaction's state then read.
    fn stream(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
        deadline: Option<Instant>,
        on_stop: OnStop,
    ) -> Result<Outcome, DbError> {
        let killer = self.killer.clone();
        let conn = self.conn.as_mut().ok_or_else(|| DbError::Connect("closed".to_owned()))?;
        let result =
            self.runtime.block_on(stream(conn, &killer, query, sink, cancel, deadline, on_stop));
        self.read_tx();
        result
    }

    /// The transaction's state as the server last said it (an error said nothing: asked).
    fn read_tx(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        if conn.last_ok_packet().is_none() {
            let _ = self.runtime.block_on(conn.query_drop("DO 0"));
        }
        if let Some(ok) = conn.last_ok_packet() {
            self.in_tx = ok.status_flags().contains(StatusFlags::SERVER_STATUS_IN_TRANS);
        }
    }

    /// Statements run for their effect alone, the transaction's state then read.
    fn simple(&mut self, sql: &str) -> Result<(), DbError> {
        let conn = self.conn.as_mut().ok_or_else(|| DbError::Connect("closed".to_owned()))?;
        let result =
            self.runtime.block_on(conn.query_drop(sql)).map_err(|e| error_in(e, Some(sql)));
        self.read_tx();
        result
    }

    /// A catalog query's rows, each value as text.
    fn catalog(&mut self, sql: &str) -> Result<TextRows, DbError> {
        let conn = self.conn.as_mut().ok_or_else(|| DbError::Connect("closed".to_owned()))?;
        let rows: Vec<mysql_async::Row> =
            self.runtime.block_on(conn.query(sql)).map_err(|e| error_in(e, Some(sql)))?;
        Ok(rows
            .into_iter()
            .map(|row| row.unwrap_raw().into_iter().map(text_of).collect())
            .collect())
    }

    /// The tables and views of every user schema, or of `schema` alone.
    fn list_tables(&mut self, schema: Option<&str>) -> Result<Vec<TableInfo>, DbError> {
        Ok(self
            .catalog(&catalog::tables(schema))?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                let kind = catalog::table_kind(&text(2));
                TableInfo {
                    table: TableRef { schema: Some(text(0)), name: text(1) },
                    kind,
                    rows: if kind == TableKind::View { None } else { text(3).parse().ok() },
                }
            })
            .collect())
    }

    /// `table`'s structure from the catalog.
    fn read_structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        let schema = table.schema.as_deref();
        let mariadb = self.version.mariadb;
        let text = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten();
        let mut columns = Vec::new();
        for r in self.catalog(&catalog::columns(schema, &table.name))? {
            let mut column =
                ColumnInfo::new(text(&r, 0).unwrap_or_default(), text(&r, 1).unwrap_or_default());
            column.nullable = text(&r, 2).as_deref() == Some("YES");
            let extra = text(&r, 4).unwrap_or_default();
            let data_type = text(&r, 7).unwrap_or_default();
            match catalog::generated(&extra) {
                Some(stored) => {
                    column.generated =
                        Some(Generated { expression: text(&r, 5).unwrap_or_default(), stored });
                }
                None => {
                    column.auto_increment = catalog::auto_increment(&extra);
                    column.default =
                        catalog::default_of(text(&r, 3).as_deref(), &extra, &data_type, mariadb);
                }
            }
            column.collation = text(&r, 6);
            columns.push(column);
        }
        if columns.is_empty() {
            return Err(DbError::Query(format!("no table {table}")));
        }
        let mut indexes: Vec<IndexInfo> = Vec::new();
        for r in self.catalog(&catalog::indexes(self.version, schema, &table.name))? {
            let name = text(&r, 0).unwrap_or_default();
            let column =
                text(&r, 3).or_else(|| text(&r, 6).map(|e| format!("({e})"))).unwrap_or_default();
            let column = match text(&r, 5) {
                Some(prefix) => format!("{column}({prefix})"),
                None => column,
            };
            let descending = text(&r, 4).as_deref() == Some("D");
            if name == "PRIMARY" {
                let place =
                    indexes.first().filter(|i| i.name == "PRIMARY").map_or(0, |i| i.columns.len());
                if let Some(c) = columns.iter_mut().find(|c| c.name == column) {
                    c.in_key = true;
                    c.key_position = u16::try_from(place + 1).ok();
                }
            }
            match indexes.iter_mut().find(|i| i.name == name) {
                Some(index) => {
                    index.columns.push(column);
                    index.descending.push(descending);
                }
                None => indexes.push(IndexInfo {
                    implicit: name == "PRIMARY",
                    unique: text(&r, 1).as_deref() == Some("0"),
                    name,
                    columns: vec![column],
                    descending: vec![descending],
                    predicate: None,
                }),
            }
        }
        for index in &mut indexes {
            if index.descending.iter().all(|d| !d) {
                index.descending.clear();
            }
        }
        let foreign_keys = self
            .read_keys(schema, Some(&table.name))?
            .into_iter()
            .next()
            .map(|(_, k)| k)
            .unwrap_or_default();
        Ok(Structure {
            table: table.clone(),
            columns,
            indexes,
            foreign_keys,
            rowid: false,
            lines: false,
        })
    }

    /// Foreign keys from the catalog, grouped by table (see [`catalog::foreign_keys`]).
    fn read_keys(
        &mut self,
        schema: Option<&str>,
        table: Option<&str>,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        let mut all: Vec<database::TableKeys> = Vec::new();
        let mut last_key: Option<(TableRef, String)> = None;
        for r in self.catalog(&catalog::foreign_keys(schema, table))? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let owner = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let name = text(2).unwrap_or_default();
            if all.last().is_none_or(|(t, _)| *t != owner) {
                all.push((owner.clone(), Vec::new()));
            }
            let Some((_, keys)) = all.last_mut() else { continue };
            let same = last_key.as_ref().is_some_and(|(t, n)| *t == owner && *n == name);
            if !same {
                keys.push(ForeignKey {
                    name: Some(name.clone()),
                    columns: Vec::new(),
                    table: TableRef { schema: text(3), name: text(4).unwrap_or_default() },
                    references: Vec::new(),
                    on_update: catalog::action(&text(7).unwrap_or_default()),
                    on_delete: catalog::action(&text(8).unwrap_or_default()),
                });
                last_key = Some((owner, name));
            }
            if let Some(key) = keys.last_mut() {
                key.columns.extend(text(5));
                key.references.extend(text(6));
            }
        }
        Ok(all)
    }

    fn node(
        kind: &'static str,
        name: &str,
        detail: String,
        parent: &[String],
        expandable: bool,
    ) -> Node {
        let mut path = parent.to_vec();
        path.push(name.to_owned());
        Node { kind, name: name.to_owned(), detail, path, expandable }
    }

    /// The warnings the last statement left (`SHOW WARNINGS`), as notices.
    fn take_warnings(&mut self) -> Vec<Notice> {
        let Some(conn) = self.conn.as_mut() else { return Vec::new() };
        if conn.get_warnings() == 0 {
            return Vec::new();
        }
        match self.catalog(&format!("SHOW WARNINGS LIMIT {WARNINGS}")) {
            Ok(rows) => rows
                .into_iter()
                .map(|r| {
                    let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                    console::warning(&text(0), &text(1), &text(2))
                })
                .collect(),
            Err(e) => {
                tracing::debug!(error = %e, "the warnings weren't read");
                Vec::new()
            }
        }
    }
}

impl Session for MySession {
    fn server(&self) -> String {
        self.server.clone()
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        Some(Arc::new(self.dialect()))
    }

    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        match path {
            [] => self.list_tables(None),
            [schema] => self.list_tables(Some(schema)),
            _ => Ok(Vec::new()),
        }
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        self.read_structure(table)
    }

    /// A page: in the asked order (`OFFSET`), or, with none, in its primary key's — the
    /// page after one read starting after its last key (`(k1, k2) > (…)`), so a far page
    /// costs no more than the first.
    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let structure = self.read_structure(table)?;
        let d = self.dialect();
        let key = structure.primary_key();
        let natural = !key.is_empty() && page.order.is_empty();
        let after = if natural {
            self.cursors
                .iter()
                .find(|c| c.table == *table && c.filter == page.filter && c.next == page.offset)
                .map(|c| c.last.clone())
        } else {
            None
        };
        let mut sql = format!("SELECT * FROM {}", d.table_name(table)?);
        let mut conditions = Vec::new();
        if let Some(filter) = &page.filter {
            conditions.push(format!("({})", d.filter(filter)?));
        }
        if let Some(last) = &after {
            let names: Vec<String> =
                key.iter().map(|k| d.quote_identifier(k)).collect::<Result<_, _>>()?;
            let values: Vec<String> =
                last.iter().map(|v| d.literal(v)).collect::<Result<_, _>>()?;
            conditions.push(format!("({}) > ({})", names.join(", "), values.join(", ")));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        let order: Vec<String> = if natural {
            key.iter()
                .map(|k| d.quote_identifier(k).map(|c| format!("{c} ASC")))
                .collect::<Result<_, _>>()?
        } else {
            page.order
                .iter()
                .map(|o| {
                    let c = d.quote_identifier(&o.column)?;
                    Ok(if o.descending { format!("{c} DESC") } else { format!("{c} ASC") })
                })
                .collect::<Result<_, DbError>>()?
        };
        if !order.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&order.join(", "));
        }
        sql.push(' ');
        sql.push_str(&d.pagination(page.limit, if after.is_some() { 0 } else { page.offset }));
        let mut keyed = Keyed { sink, key, at: Vec::new(), last: None };
        let outcome = self.stream(&sql, &mut keyed, cancel, None, OnStop::Kill)?;
        if natural
            && let Some(last) = keyed.last
            && outcome.rows >= u64::from(page.limit)
            && !outcome.stopped
        {
            let next = page.offset + outcome.rows;
            self.cursors.retain(|c| !(c.table == *table && c.filter == page.filter));
            if self.cursors.len() >= CURSORS {
                self.cursors.remove(0);
            }
            self.cursors.push(Cursor {
                table: table.clone(),
                filter: page.filter.clone(),
                next,
                last,
            });
        }
        Ok(outcome)
    }

    /// Schemas (MySQL's databases) › tables and views › columns and indexes, from
    /// `information_schema`.
    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        match path {
            [] => Ok(self
                .catalog(&catalog::schemas())?
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
                .map(|name| Self::node("schema", &name, String::new(), path, true))
                .collect()),
            [schema] => Ok(self
                .list_tables(Some(schema))?
                .into_iter()
                .map(|t| {
                    let kind = if t.kind == TableKind::View { "view" } else { "table" };
                    Self::node(kind, &t.table.name, String::new(), path, true)
                })
                .collect()),
            [schema, table] => {
                let table = TableRef { schema: Some(schema.clone()), name: table.clone() };
                let structure = self.read_structure(&table)?;
                let mut nodes: Vec<Node> = structure
                    .columns
                    .iter()
                    .map(|c| {
                        let detail = if c.nullable {
                            format!("{}, null", c.declared_type)
                        } else {
                            c.declared_type.clone()
                        };
                        Self::node("column", &c.name, detail, path, false)
                    })
                    .collect();
                nodes.extend(structure.indexes.iter().map(|i| {
                    Self::node("index", &i.name, format!("({})", i.columns.join(", ")), path, false)
                }));
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
        // A write may move rows: the pages' places are read again.
        if !query.trim_start().get(..6).is_some_and(|w| w.eq_ignore_ascii_case("select")) {
            self.cursors.clear();
        }
        self.stream(query, sink, cancel, None, OnStop::Kill)
    }

    fn console(&self) -> Option<ConsoleCaps> {
        Some(console::caps(self.version))
    }

    /// Streamed: a single query stopped (killed) one row past the page, anything else read
    /// to its end, the rows past the page dropped; the warnings it left with the outcome.
    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let read = console::statement(statement);
        let query = read.kind == StatementKind::Query && !read.several;
        if !query {
            self.cursors.clear();
        }
        let overflow = if query { Overflow::Stop } else { Overflow::Drop };
        let on_stop = if query { OnStop::Kill } else { OnStop::Drain };
        let mut limited = Limited::new(sink, options.fetch_limit, overflow);
        let outcome = self.stream(statement, &mut limited, cancel, None, on_stop)?;
        let (rows, truncated, stopped) = (limited.delivered, limited.truncated, limited.stopped);
        Ok(RunOutcome {
            affected: outcome.affected,
            rows,
            truncated,
            more: None,
            stopped,
            notices: self.take_warnings(),
        })
    }

    /// The profile's projection: the plan, or as it ran (a write undone).
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

    fn profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        self.run_profile(statement, options, cancel)
    }

    fn table_stats(
        &mut self,
        tables: &[TableRef],
        cancel: &Cancel,
    ) -> Result<Vec<database::profile::TableStats>, DbError> {
        if tables.is_empty() {
            return Ok(Vec::new());
        }
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let rows = self.catalog(&stats::tables_sql(tables))?;
        let indexes = self.catalog(&stats::indexes_sql(tables))?;
        // `performance_schema`'s reads per table and index, when the user may see them.
        let io = match self.catalog(&stats::io_sql(tables)) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!(error = %e, "performance_schema's table I/O wasn't read");
                Vec::new()
            }
        };
        Ok(stats::assemble(tables, &rows, &indexes, &io))
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        for statement in console::begin_statements(options)? {
            self.simple(&statement)?;
        }
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.cursors.clear();
        // Failed or not, the transaction is over.
        self.in_tx = false;
        self.simple("COMMIT")
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        self.cursors.clear();
        self.in_tx = false;
        self.simple("ROLLBACK")
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    /// `USE schema`: MySQL's databases are its schemas.
    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        self.simple(&format!("USE {}", quote_backtick(schema)?))
    }

    /// The session's database, as its schema (MySQL has no level above).
    fn current(&mut self) -> Result<Place, DbError> {
        let rows = self.catalog("SELECT DATABASE()")?;
        let schema = rows.into_iter().next().and_then(|r| r.into_iter().next().flatten());
        Ok(Place { database: None, schema })
    }

    /// Its databases (schemas), those the user has any privilege on.
    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(self
            .catalog(&catalog::schemas())?
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }

    /// One query over `information_schema.COLUMNS` (with the primary keys' places).
    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let mariadb = self.version.mariadb;
        let mut all: Vec<TableColumns> = Vec::new();
        for r in self.catalog(&catalog::catalog_columns(schema))? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let table = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let extra = text(8).unwrap_or_default();
            let column = ColumnBrief {
                name: text(2).unwrap_or_default(),
                type_name: text(3).unwrap_or_default(),
                nullable: text(4).as_deref() == Some("YES"),
                default: catalog::default_of(
                    text(5).as_deref(),
                    &extra,
                    &text(9).unwrap_or_default(),
                    mariadb,
                ),
                comment: text(6).filter(|c| !c.is_empty()),
                key_position: text(7).and_then(|k| k.parse().ok()),
                // In its type (`enum('S','M')`): read from there.
                values: None,
            };
            match all.last_mut() {
                Some((last, columns)) if *last == table => columns.push(column),
                _ => all.push((table, vec![column])),
            }
        }
        Ok(all)
    }

    fn catalog_keys(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        // `None`: every user schema (not only the session's).
        match schema {
            Some(schema) => self.read_keys(Some(schema), None),
            None => self.read_keys(None, None),
        }
    }

    /// MariaDB's sequences: names a query may read that the tables don't list.
    fn catalog_relations(&mut self, cancel: &Cancel) -> Result<Vec<TableRef>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        if !self.version.has(database::mysql::Feature::Sequences) {
            return Ok(Vec::new());
        }
        Ok(self
            .catalog(&catalog::sequences())?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                TableRef { schema: text(0), name: text(1).unwrap_or_default() }
            })
            .collect())
    }

    /// The user's routines from `information_schema.ROUTINES` and `PARAMETERS`; for every
    /// schema (`None`), the server's built-in functions too.
    fn routines(&mut self, schema: Option<&str>, cancel: &Cancel) -> Result<Vec<Routine>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let mut parameters: Vec<(RoutineKey, Vec<String>)> = Vec::new();
        for r in self.catalog(&catalog::parameters(schema))? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let key = (text(0), text(1).unwrap_or_default());
            let mut argument = String::new();
            if let Some(mode) = text(4).filter(|m| m != "IN") {
                argument.push_str(&mode);
                argument.push(' ');
            }
            if let Some(name) = text(2) {
                argument.push_str(&name);
                argument.push(' ');
            }
            argument.push_str(&text(3).unwrap_or_default());
            match parameters.last_mut() {
                Some((k, list)) if *k == key => list.push(argument),
                _ => parameters.push((key, vec![argument])),
            }
        }
        let mut out: Vec<Routine> = self
            .catalog(&catalog::routines(schema))?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                let key = (text(0), text(1).unwrap_or_default());
                let arguments = parameters
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, list)| list.clone())
                    .unwrap_or_default();
                let procedure = text(2).as_deref() == Some("PROCEDURE");
                Routine {
                    schema: key.0,
                    name: key.1,
                    arity: u16::try_from(arguments.len()).ok(),
                    arguments: arguments.join(", "),
                    result: if procedure { None } else { text(3) },
                    kind: if procedure { RoutineKind::Procedure } else { RoutineKind::Function },
                    builtin: false,
                }
            })
            .collect();
        if schema.is_none() {
            out.extend(catalog::BUILTINS.iter().map(|(name, arity)| Routine {
                schema: None,
                name: (*name).to_owned(),
                arguments: String::new(),
                result: None,
                kind: catalog::builtin_kind(name),
                arity: *arity,
                builtin: true,
            }));
        }
        Ok(out)
    }
}

impl MySession {
    /// See [`profile`]: the statement explained, or run and measured — in a transaction of
    /// its own (read-only for a read: a function that writes, called from a `SELECT`,
    /// fails instead) or under a savepoint inside the user's, rolled back at the end:
    /// nothing it did stays. What can't be rolled back isn't run (a schema change commits
    /// by itself; a table of an engine without transactions keeps its changes).
    fn run_profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        let read = console::statement(statement);
        let v = self.version;
        let runs = options.depth.runs();
        let writes = read.kind.writes();
        if runs {
            if !profile::runs(v) {
                return Err(DbError::Unsupported(format!(
                    "profiling as it runs on {} {} (MySQL 8.0.18 or MariaDB 10.1 or newer)",
                    v.engine(),
                    v.text()
                )));
            }
            if let Some(why) = profile::refused(read.kind, read.body) {
                return Err(DbError::Unsupported(format!(
                    "profiling {why} as it runs: it can't be undone"
                )));
            }
            if writes {
                self.check_undoable(read.body)?;
            }
        }
        let deadline = options.timeout.map(|t| Instant::now() + t);
        let shift =
            |e: DbError, prefix: &str| console::shifted(e, prefix.chars().count(), read.leading);
        if !runs {
            let prefix = "EXPLAIN FORMAT=JSON ";
            let json = self
                .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                .map_err(|e| shift(e, prefix))?;
            let mut p = profile::parse_json(&json, options.depth)?;
            self.stamp(&mut p, options, false, writes);
            p.keep_raw(database::profile::RawFormat::Json, &json, options.max_raw_bytes);
            return Ok(p);
        }
        let user_tx = self.in_tx;
        if user_tx {
            self.simple(&format!("SAVEPOINT {}", profile::SAVEPOINT))?;
        } else {
            let locks = read.locks;
            self.simple(if writes || locks {
                "START TRANSACTION"
            } else {
                "START TRANSACTION READ ONLY"
            })?;
        }
        let measured = self.measure(&read, options, cancel, deadline);
        // Undone whatever happened (a failure leaves nothing more to undo).
        if user_tx {
            let point = profile::SAVEPOINT;
            let _ = self.simple(&format!("ROLLBACK TO SAVEPOINT {point}"));
            let _ = self.simple(&format!("RELEASE SAVEPOINT {point}"));
        } else {
            let _ = self.simple("ROLLBACK");
        }
        let (mut p, raw, format) = measured?;
        self.stamp(&mut p, options, true, writes);
        p.keep_raw(format, &raw, options.max_raw_bytes);
        Ok(p)
    }

    /// The statement run inside the profile's transaction, measured: its plan as it ran
    /// (MariaDB's `ANALYZE FORMAT=JSON`, MySQL's `EXPLAIN ANALYZE`; for what MySQL can't
    /// run as a tree, its plan and the statement run by itself), the session's counters
    /// around it, its time.
    fn measure(
        &mut self,
        read: &console::Statement<'_>,
        options: &ProfileOptions,
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<(Profile, String, database::profile::RawFormat), DbError> {
        use database::profile::RawFormat;
        let shift =
            |e: DbError, prefix: &str| console::shifted(e, prefix.chars().count(), read.leading);
        let before = self.status();
        let started = Instant::now();
        let (mut p, raw, format) = if self.version.mariadb {
            let prefix = "ANALYZE FORMAT=JSON ";
            let json = self
                .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                .map_err(|e| shift(e, prefix))?;
            (profile::parse_json(&json, options.depth)?, json, RawFormat::Json)
        } else {
            let prefix = "EXPLAIN ANALYZE ";
            let tree = self
                .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                .map_err(|e| shift(e, prefix))?;
            if tree.trim().trim_start_matches("->").trim() == profile::NOT_A_TREE {
                // Its plan, then the statement itself, timed.
                let prefix = "EXPLAIN FORMAT=JSON ";
                let json = self
                    .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                    .map_err(|e| shift(e, prefix))?;
                let mut p = profile::parse_json(&json, options.depth)?;
                let run = Instant::now();
                let outcome = self
                    .stream(read.body, &mut Discard, cancel, deadline, OnStop::Drain)
                    .map_err(|e| shift(e, ""))?;
                p.statement.execution_ms = Some(run.elapsed().as_secs_f64() * 1000.0);
                p.statement.rows_affected = outcome.affected;
                (p, json, RawFormat::Json)
            } else {
                (profile::parse_tree(&tree, options.depth)?, tree, RawFormat::Text)
            }
        };
        p.statement.wall_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        let after = self.status();
        if !before.is_empty() && !after.is_empty() {
            let (counters, extra) = profile::counters(&before, &after);
            p.statement.counters = Some(counters);
            p.statement.extra.extend(extra);
        }
        // What `performance_schema` kept of it, when the user may read it.
        let prefix = if self.version.mariadb { "ANALYZE" } else { "EXPLAIN ANALYZE" };
        match self.catalog(&profile::history_sql(prefix)) {
            Ok(rows) => {
                if let Some(row) = rows.first() {
                    let (examined, extra) = profile::history_row(row);
                    if let (Some(examined), Some(counters)) =
                        (examined, p.statement.counters.as_mut())
                    {
                        counters.rows_examined = Some(examined);
                    }
                    p.statement.extra.extend(extra);
                }
            }
            Err(e) => tracing::debug!(error = %e, "performance_schema's history wasn't read"),
        }
        Ok((p, raw, format))
    }

    /// The profile's engine, what it holds and its notes.
    fn stamp(&self, p: &mut Profile, options: &ProfileOptions, runs: bool, writes: bool) {
        use database::profile::{Engine, ProfileNote};
        let v = self.version;
        p.engine = Engine {
            driver: if v.mariadb { MARIADB_ID } else { ID }.to_owned(),
            name: v.engine().to_owned(),
            version: Some(v.text()),
            version_num: Some(v.number),
        };
        p.depth = options.depth;
        p.collected = database::ProfileCaps {
            estimated: true,
            actual: runs,
            node_timing: runs && p.nodes.iter().any(|n| n.actuals.total_ms.is_some()),
            engine_counters: p.statement.counters.is_some(),
            undo_writes: runs && writes,
            ..database::ProfileCaps::NONE
        };
        if runs && writes {
            p.note(ProfileNote::RolledBack);
        }
        if options.depth == (ProfileDepth::Actual { timing: true }) {
            p.note(ProfileNote::InstrumentationOverhead);
        }
        if runs && !writes {
            p.statement.rows_returned =
                p.roots.first().and_then(|r| p.node(*r).total_rows()).map(|r| r.round() as u64);
        }
    }

    /// The first column of every row `sql` gives, as one text (an `EXPLAIN`'s answer).
    fn single_text(
        &mut self,
        sql: &str,
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<String, DbError> {
        let mut rows = database::Collect::default();
        self.stream(sql, &mut rows, cancel, deadline, OnStop::Drain)?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| r.first())
            .map(Value::display)
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// The session's counters ([`profile::COUNTERS`]); none if they can't be read.
    fn status(&mut self) -> Vec<(String, u64)> {
        match self.catalog(&profile::status_sql()) {
            Ok(rows) => profile::status_values(&rows),
            Err(e) => {
                tracing::debug!(error = %e, "the session's counters weren't read");
                Vec::new()
            }
        }
    }

    /// Refused when a table `statement` writes is of an engine without transactions
    /// (MyISAM, MEMORY…): its changes couldn't be undone.
    fn check_undoable(&mut self, statement: &str) -> Result<(), DbError> {
        use database::sql::{self, Flavor, SplitOptions};
        let parsed = sql::parse(statement, Flavor::MySql, &SplitOptions::default());
        let Some(first) = parsed.statements.first() else { return Ok(()) };
        let tables: Vec<TableRef> =
            sql::written_tables(statement, parsed.tokens_of(first), Flavor::MySql)
                .into_iter()
                .map(|(t, _)| t)
                .collect();
        if tables.is_empty() {
            return Ok(());
        }
        let query = format!(
            "SELECT t.TABLE_NAME, t.ENGINE, e.TRANSACTIONS FROM information_schema.TABLES t \
             LEFT JOIN information_schema.ENGINES e ON e.ENGINE = t.ENGINE WHERE {}",
            stats::wanted("t.TABLE_SCHEMA", "t.TABLE_NAME", &tables)
        );
        for r in self.catalog(&query)? {
            let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
            if !text(1).is_empty() && text(2) != "YES" {
                return Err(DbError::Unsupported(format!(
                    "profiling a write to `{}` as it runs: its engine ({}) can't undo it",
                    text(0),
                    text(1)
                )));
            }
        }
        Ok(())
    }
}

/// A sink that keeps nothing (a profiled write's rows, if any).
struct Discard;

impl RowSink for Discard {
    fn columns(&mut self, _columns: &[database::Column]) {}

    fn row(&mut self, _values: Vec<Value>) -> Flow {
        Flow::Continue
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
