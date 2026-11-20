//! PostgreSQL for the `database` domain, one driver for every server from 9.2 to the
//! newest: the protocol (v3) is the same, the sign-in is whatever the server asks (trust,
//! cleartext, md5 on old servers, SCRAM-SHA-256 on new ones), and what differs — the
//! catalog, the SQL a version takes — is read from `server_version_num` when it connects
//! ([`database::postgres`]: its features, its dialect). A session owns a small
//! single-threaded runtime of its own (made when it connects: nothing runs before) driving
//! `tokio-postgres`. A query is prepared for its columns' types, then run through the
//! simple protocol so every value comes as text, streamed to the sink and typed by its
//! column; cancelling asks the server through the protocol's cancel request.
//!
//! TLS goes by `sslmode` (libpq's six, `prefer` by default: [`tls`]), through rustls.
//! Tables are read a page at a time, by their primary key when no other order is asked (the
//! next page starts after the last key read, no growing `OFFSET`), and changed by the
//! domain's transactions in the version's dialect. The explorer: schemas › tables and
//! views › columns and indexes.
//!
//! For the console ([`console`]): a query's pages come from a server-side cursor (`DECLARE
//! … NO SCROLL CURSOR`, in the session's transaction, or in a read-only one of its own
//! closed at the next statement), a write's rows past the page are read to its end and
//! dropped (cancelling it would undo it), the server's notices come with the outcome,
//! profiles (and plans, their projection) are read from `EXPLAIN (…, FORMAT JSON)` with the
//! options the server's version takes ([`profile`]: a run in a read-only transaction of its
//! own, a write's undone, a savepoint inside the user's, a local `statement_timeout`), errors
//! keep their place, SQLSTATE, detail and hint, a profile's tables' statistics and the
//! query's history from `pg_stat_statements` are read on the same connection ([`stats`]:
//! under a savepoint inside the user's transaction), transactions begin with their isolation,
//! the schema is `search_path`, and the catalog is read in bulk (columns from
//! `pg_attribute`, routines from `pg_proc` by version). A read-only source's sessions are
//! `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY`.

pub mod catalog;
pub mod console;
pub mod profile;
pub mod stats;
pub mod tls;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use database::fields::{Binding, ConnectionSpec, FieldKind, FieldSpec, FieldTab};
use database::postgres::{self as pg, Postgres as PgDialect};
use database::profile::{Engine, Profile, ProfileDepth, ProfileNote, ProfileOptions, RawFormat};
use database::run::{Limited, Overflow};
use database::sql::{Flavor, StatementKind};
use database::tls::{SSLCERT, SSLKEY, SSLMODE, SSLROOTCERT};
use database::{
    Cancel, Capabilities, Collect, Column, ColumnBrief, ColumnInfo, ConsoleCaps, CursorToken,
    DbError, Dialect, Driver, DriverFactory, Filter, Flow, ForeignKey, Generated, IndexInfo,
    Isolation, Kind, Language, Node, Notice, Outcome, Page, Place, PlanNode, Routine, RoutineKind,
    RowKey, RowSink, RunOptions, RunOutcome, SchemaChanges, Secret, Session, Sorting, SslMode,
    Structure, TableColumns, TableInfo, TableKind, TableRef, Target, TxOptions, Value,
};
use futures_util::StreamExt;
use tokio_postgres::{AsyncMessage, CancelToken, Client, NoTls, SimpleQueryMessage};

use crate::tls::Attempt;

/// The driver's id.
pub const ID: &str = "postgres";

/// The driver as the app lists it (created only when a connection opens).
pub fn factory() -> DriverFactory {
    DriverFactory::new(ID, "PostgreSQL", Kind::Relational, Some(5432), || Arc::new(Postgres))
}

/// How long a connection may take (unless the source's `connect_timeout` says).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a running query looks whether it was cancelled.
const CANCEL_CHECK: Duration = Duration::from_millis(50);
/// The places remembered to start a table's next page after its last key.
const CURSORS: usize = 8;

/// The connection's options its fields set (the others go nowhere: no engine option is
/// passed blindly).
pub const CONNECT_TIMEOUT_OPTION: &str = "connect_timeout";
pub const APPLICATION_NAME: &str = "application_name";

#[derive(Debug)]
pub struct Postgres;

/// TLS for the connection and for its cancel requests.
#[derive(Clone)]
enum Transport {
    Plain,
    Tls(tokio_postgres_rustls::MakeRustlsConnect),
}

/// What the driver can do (every supported version: the same).
pub fn capabilities() -> Capabilities {
    Capabilities {
        transactions: true,
        edit_rows: true,
        schema_changes: SchemaChanges { reorder: false, ..SchemaChanges::ALL },
        schemas: true,
        file_based: false,
        cancel: true,
        returning: true,
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
    ConnectionSpec::server(5432, &["postgresql", "postgres"])
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
        .with(
            FieldSpec::new(
                "application_name",
                "datasources.field_application_name",
                FieldKind::Text,
                option(APPLICATION_NAME),
                FieldTab::Options,
            )
            .with_default("editor"),
        )
}

impl Driver for Postgres {
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
        Some(Arc::new(PgDialect::default()))
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
        let mut config = tokio_postgres::Config::new();
        config
            .host(if target.host.is_empty() { "localhost" } else { &target.host })
            .port(target.port.unwrap_or(5432))
            .user(&target.user)
            .password(secret.expose().as_bytes())
            .connect_timeout(timeout)
            .application_name(
                target.options.get(APPLICATION_NAME).map_or("editor", String::as_str),
            );
        if !target.database.is_empty() {
            config.dbname(&target.database);
        }
        let has_root = tls::file_option(target, SSLROOTCERT).is_some();
        let mut last = None;
        let mut opened = None;
        let notices = Notices::default();
        for attempt in tls::attempts(mode, has_root) {
            match connect_once(&runtime, &mut config, target, attempt, &notices) {
                Ok(done) => {
                    opened = Some(done);
                    break;
                }
                // The server unreachable: no other attempt would reach it.
                Err(e @ DbError::Connect(_)) => {
                    last = Some(e);
                    break;
                }
                // `allow`: plain refused (the server wants TLS), so TLS next.
                Err(e) => last = Some(e),
            }
        }
        let (client, transport) = match opened {
            Some(done) => done,
            None => return Err(last.unwrap_or_else(|| DbError::Connect("no attempt".into()))),
        };
        let show = |name: &str| {
            runtime
                .block_on(client.query_one(&format!("SHOW {name}"), &[]))
                .ok()
                .map(|row| row.get::<_, String>(0))
        };
        let version_name = show("server_version").unwrap_or_default();
        let version = show("server_version_num")
            .and_then(|v| pg::parse_version_num(&v))
            .unwrap_or(pg::OLDEST);
        let server = if version_name.is_empty() {
            "PostgreSQL".to_owned()
        } else {
            // `16.4 (Debian 16.4-1)`: only the number.
            format!("PostgreSQL {}", version_name.split_whitespace().next().unwrap_or_default())
        };
        // A read-only source writes nothing, whatever the app checks before.
        if target.read_only() {
            runtime.block_on(client.batch_execute(console::READ_ONLY_SESSION)).map_err(error)?;
        }
        let block_size = show("block_size")
            .and_then(|b| b.trim().parse::<u32>().ok())
            .filter(|b| *b > 0)
            .unwrap_or(profile::BLOCK_SIZE);
        let cancel = client.cancel_token();
        Ok(Box::new(PgSession {
            runtime,
            client,
            cancel,
            transport,
            server,
            version,
            cursors: Vec::new(),
            notices,
            in_tx: false,
            cursor: None,
            next_cursor: 1,
            block_size,
            statement_stats: None,
        }))
    }
}

/// One connection attempt: the client (its connection driven on the runtime) and how its
/// cancel requests go.
fn connect_once(
    runtime: &tokio::runtime::Runtime,
    config: &mut tokio_postgres::Config,
    target: &Target,
    attempt: Attempt,
    notices: &Notices,
) -> Result<(Client, Transport), DbError> {
    use tokio_postgres::config::SslMode as Wire;
    let (wire, check) = match attempt {
        Attempt::Plain => (Wire::Disable, None),
        Attempt::TlsOrPlain(check) => (Wire::Prefer, Some(check)),
        Attempt::Tls(check) => (Wire::Require, Some(check)),
    };
    config.ssl_mode(wire);
    let transport = match check {
        None => Transport::Plain,
        Some(check) => Transport::Tls(tokio_postgres_rustls::MakeRustlsConnect::new(
            tls::client_config(target, check)?,
        )),
    };
    let client = runtime.block_on(async {
        match &transport {
            Transport::Plain => {
                let (client, connection) = config.connect(NoTls).await.map_err(refused)?;
                tokio::spawn(drive(connection, Arc::clone(notices)));
                Ok::<_, DbError>(client)
            }
            Transport::Tls(tls) => {
                let (client, connection) = config.connect(tls.clone()).await.map_err(refused)?;
                tokio::spawn(drive(connection, Arc::clone(notices)));
                Ok(client)
            }
        }
    })?;
    Ok((client, transport))
}

/// The server's notices since they were last taken (the newest [`console::NOTICES`]).
type Notices = Arc<Mutex<VecDeque<Notice>>>;

/// Drives a connection, keeping the notices it brings (the server's other messages
/// dropped); ends with it.
async fn drive<S, T>(mut connection: tokio_postgres::Connection<S, T>, notices: Notices)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut messages = futures_util::stream::poll_fn(move |cx| connection.poll_message(cx));
    while let Some(message) = messages.next().await {
        match message {
            Ok(AsyncMessage::Notice(notice)) => {
                let mut kept = notices.lock().unwrap_or_else(PoisonError::into_inner);
                if kept.len() == console::NOTICES {
                    kept.pop_front();
                }
                kept.push_back(console::notice(&notice));
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

/// A connection refused, as the domain says it: a database the user may not connect to
/// (42501: `CONNECT` revoked) is refused as a wrong password is, the rest as [`error`].
fn refused(e: tokio_postgres::Error) -> DbError {
    match e.as_db_error() {
        Some(db) if db.code().code() == "42501" => DbError::Auth(db.message().to_owned()),
        _ => error(e),
    }
}

/// A failure, as the domain says it: the server's message when it gave one, with its
/// place, code, detail and hint.
fn error(e: tokio_postgres::Error) -> DbError {
    match e.as_db_error() {
        // 28P01: wrong password; 28000: no such role / not allowed.
        Some(db) if db.code().code().starts_with("28") => DbError::Auth(db.message().to_owned()),
        Some(db) if db.code().code() == "57014" => DbError::Cancelled,
        Some(db) => DbError::sql(console::sql_error(
            db.message(),
            db.code().code(),
            db.position(),
            db.detail(),
            db.hint(),
        )),
        None if e.is_closed() => DbError::Connect("the connection closed".to_owned()),
        None => DbError::Connect(e.to_string()),
    }
}

struct PgSession {
    runtime: tokio::runtime::Runtime,
    client: Client,
    cancel: CancelToken,
    transport: Transport,
    server: String,
    /// `server_version_num`.
    version: u32,
    /// Where a table's next page starts (after the last key read), the newest last.
    cursors: Vec<Cursor>,
    notices: Notices,
    /// A transaction is open (begun by `begin` or by a statement run).
    in_tx: bool,
    /// The console's result read from a cursor (one at a time: closed at the next
    /// statement).
    cursor: Option<OpenCursor>,
    next_cursor: u64,
    /// `SHOW block_size`, read when it connects (a profile's blocks as bytes).
    block_size: u32,
    /// Where `pg_stat_statements` is installed (`Some(None)`: it isn't), read once when a
    /// profile first asks for the query's history.
    statement_stats: Option<Option<String>>,
}

/// A console's result held in a server-side cursor.
#[derive(Debug)]
struct OpenCursor {
    name: String,
    columns: Vec<Column>,
    /// The row read past the last page (to know one was left).
    pending: Option<Vec<Value>>,
    /// In a read-only transaction of its own (committed when it closes), not the user's.
    own_tx: bool,
}

/// How a statement's stream takes a sink's stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnStop {
    /// The server is asked to stop (a query: nothing lost).
    Cancel,
    /// The rest is read and given to the sink all the same (a cursor's page, which a cancel
    /// would abort with its transaction).
    Drain,
}

/// A cursor's page: at most `limit` rows to the sink, one more kept, the set's columns told
/// once.
struct PageSink<'a> {
    sink: &'a mut dyn RowSink,
    announced: bool,
    limit: u64,
    delivered: u64,
    read: u64,
    pending: Option<Vec<Value>>,
    stopped: bool,
}

impl PageSink<'_> {
    fn deliver(&mut self, values: Vec<Value>) {
        if self.stopped {
            return;
        }
        if self.delivered < self.limit {
            self.delivered += 1;
            if self.sink.row(values) == Flow::Stop {
                self.stopped = true;
            }
        } else {
            self.pending = Some(values);
        }
    }
}

impl RowSink for PageSink<'_> {
    fn columns(&mut self, columns: &[Column]) {
        if !self.announced {
            self.announced = true;
            self.sink.columns(columns);
        }
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        self.read += 1;
        self.deliver(values);
        Flow::Continue
    }
}

/// Whether `e` is a write refused in a read-only transaction.
fn read_only_refusal(e: &DbError) -> bool {
    e.sql_error().and_then(|s| s.sqlstate.as_deref()) == Some(console::READ_ONLY_TRANSACTION)
}

/// A table read in its key's order: the page starting at `next` starts after `last`.
#[derive(Debug, Clone)]
struct Cursor {
    table: TableRef,
    filter: Option<Filter>,
    next: u64,
    last: Vec<Value>,
}

/// A value of a column of type `type_name`, from its text.
pub fn value(type_name: &str, text: Option<&str>) -> Value {
    let Some(text) = text else { return Value::Null };
    let parsed = match type_name {
        "bool" => Some(Value::Bool(text == "t")),
        // `\x…`: the bytes in hex (the default output since 9.0).
        "bytea" => text.strip_prefix("\\x").and_then(hex_bytes).map(Value::Bytes),
        "int2" | "int4" | "int8" | "oid" => text.parse().ok().map(Value::Int),
        "float4" | "float8" => text.parse().ok().map(Value::Float),
        "json" | "jsonb" => Some(Value::Json(text.to_owned())),
        "text" | "varchar" | "bpchar" | "name" | "char" | "citext" => {
            Some(Value::Text(text.to_owned()))
        }
        _ => None,
    };
    parsed.unwrap_or_else(|| Value::Other(text.to_owned()))
}

/// The bytes `hex` spells; `None` if it isn't hex.
fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()).collect()
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
    fn columns(&mut self, columns: &[Column]) {
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

impl PgSession {
    fn dialect(&self) -> PgDialect {
        PgDialect::new(self.version)
    }

    /// A catalog query's rows, each value as text (none: `None`), `params` text too.
    fn catalog(&self, query: &str, params: &[&str]) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();
        let rows = self.runtime.block_on(self.client.query(query, &params)).map_err(error)?;
        Ok(rows
            .iter()
            .map(|row| (0..row.len()).map(|i| row.get::<_, Option<String>>(i)).collect())
            .collect())
    }

    /// The tables and views of every user schema, or of `schema` alone.
    fn list_tables(&self, schema: Option<&str>) -> Result<Vec<TableInfo>, DbError> {
        let sql = catalog::tables(self.version, schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        Ok(self
            .catalog(&sql, &params)?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                let kind = match text(2).as_str() {
                    "v" | "m" => TableKind::View,
                    _ => TableKind::Table,
                };
                let tuples: f64 = text(3).parse().unwrap_or(-1.0);
                let pages: i64 = text(4).parse().unwrap_or(0);
                // Never analyzed: the statistics collector's live rows (a table's, a
                // materialized view's), as cheap.
                let live = matches!(text(2).as_str(), "r" | "m")
                    .then(|| text(5).parse::<u64>().ok())
                    .flatten();
                TableInfo {
                    table: TableRef { schema: Some(text(0)), name: text(1) },
                    kind,
                    rows: pg::estimated_rows(self.version, tuples, pages).or(live),
                }
            })
            .collect())
    }

    /// `table`'s structure from the catalog (as its version keeps it).
    fn read_structure(&self, table: &TableRef) -> Result<Structure, DbError> {
        let name = self.dialect().table_name(table)?;
        let text = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten();
        let mut columns = Vec::new();
        let mut numbers = Vec::new();
        for r in self.catalog(&catalog::columns(self.version), &[&name])? {
            let declared = text(&r, 1).unwrap_or_default();
            let mut column = ColumnInfo::new(text(&r, 0).unwrap_or_default(), declared);
            column.nullable = text(&r, 2).as_deref() != Some("true");
            let default = text(&r, 3);
            let identity = text(&r, 4).unwrap_or_default();
            let generated = text(&r, 5).unwrap_or_default();
            if generated == "s" || generated == "v" {
                column.generated = Some(Generated {
                    expression: default.clone().unwrap_or_default(),
                    stored: generated == "s",
                });
            } else {
                column.auto_increment = matches!(identity.as_str(), "a" | "d")
                    || default.as_deref().is_some_and(|d| d.starts_with("nextval("));
                column.default = default;
            }
            column.collation = text(&r, 6);
            numbers.push(text(&r, 7).and_then(|n| n.parse::<i64>().ok()).unwrap_or(0));
            column.values = text(&r, 8).map(|labels| catalog::enum_labels(&labels));
            columns.push(column);
        }
        if columns.is_empty() {
            return Err(DbError::Query(format!("no table {table}")));
        }
        // The indexes and their columns, in order.
        let mut keyed: Vec<(String, Vec<String>)> = Vec::new();
        for r in self.catalog(&catalog::index_columns(self.version), &[&name])? {
            let index = text(&r, 0).unwrap_or_default();
            let column = catalog::unquote(&text(&r, 2).unwrap_or_default());
            match keyed.iter_mut().find(|(n, _)| *n == index) {
                Some((_, list)) => list.push(column),
                None => keyed.push((index, vec![column])),
            }
        }
        let mut indexes = Vec::new();
        for r in self.catalog(&catalog::indexes(), &[&name])? {
            let index = text(&r, 0).unwrap_or_default();
            let cols = keyed.iter().find(|(n, _)| *n == index).map(|(_, c)| c.clone());
            let cols = cols.unwrap_or_default();
            let primary = text(&r, 2).as_deref() == Some("true");
            if primary {
                for (place, key) in cols.iter().enumerate() {
                    if let Some(c) = columns.iter_mut().find(|c| c.name == *key) {
                        c.in_key = true;
                        c.key_position = u16::try_from(place + 1).ok();
                    }
                }
            }
            let options = catalog::numbers(&text(&r, 5).unwrap_or_default());
            let descending =
                (0..cols.len()).map(|i| options.get(i).is_some_and(|o| o & 1 == 1)).collect();
            indexes.push(IndexInfo {
                name: index,
                columns: cols,
                unique: text(&r, 1).as_deref() == Some("true"),
                implicit: primary || text(&r, 3).is_some_and(|n| n != "0"),
                descending,
                predicate: text(&r, 4),
            });
        }
        let mut foreign_keys = Vec::new();
        for r in self.catalog(&catalog::foreign_keys(), &[&name])? {
            let here = catalog::numbers(&text(&r, 5).unwrap_or_default());
            let there = catalog::numbers(&text(&r, 6).unwrap_or_default());
            let target_oid = text(&r, 7).unwrap_or_default();
            let named: Vec<(i64, String)> = self
                .catalog(&catalog::column_names(), &[&target_oid])?
                .into_iter()
                .filter_map(|c| Some((text(&c, 0)?.parse().ok()?, text(&c, 1)?)))
                .collect();
            let local =
                |n: &i64| numbers.iter().position(|x| x == n).map(|i| columns[i].name.clone());
            let remote = |n: &i64| named.iter().find(|(x, _)| x == n).map(|(_, c)| c.clone());
            foreign_keys.push(ForeignKey {
                name: text(&r, 0),
                columns: here.iter().filter_map(local).collect(),
                table: TableRef { schema: text(&r, 3), name: text(&r, 4).unwrap_or_default() },
                references: there.iter().filter_map(remote).collect(),
                on_update: catalog::action(&text(&r, 1).unwrap_or_default()),
                on_delete: catalog::action(&text(&r, 2).unwrap_or_default()),
            });
        }
        Ok(Structure {
            table: table.clone(),
            columns,
            indexes,
            foreign_keys,
            rowid: false,
            lines: false,
        })
    }

    /// A query's rows as strings (the explorer's lists).
    fn rows(&self, query: &str, params: &[&str]) -> Result<Vec<Vec<String>>, DbError> {
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();
        let rows = self.runtime.block_on(self.client.query(query, &params)).map_err(error)?;
        Ok(rows
            .iter()
            .map(|row| {
                (0..row.len())
                    .map(|i| row.get::<_, Option<String>>(i).unwrap_or_default())
                    .collect()
            })
            .collect())
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
}

impl Session for PgSession {
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
    /// page after one read starting after its last key (`(k1, k2) > (…)`, which every
    /// version compares), so a far page costs no more than the first.
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
        let outcome = self.execute(&sql, &mut keyed, cancel)?;
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

    /// From the catalog (`pg_namespace`, `pg_class`, `pg_attribute`), as [`Self::tables`] and
    /// [`Self::structure`] read it — never `information_schema`, which hides what the user
    /// has no privilege on (another role's schemas and tables) and materialized views.
    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        match path {
            [] => Ok(self
                .rows(&catalog::schemas(), &[])?
                .into_iter()
                .map(|r| Self::node("schema", &r[0], String::new(), path, true))
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
        self.release_cursor();
        // The columns' types (a single statement can be prepared; several can't: untyped).
        let types = self.types(query, cancel)?;
        // A write may move rows: the pages' places are read again.
        if !query.trim_start().get(..6).is_some_and(|w| w.eq_ignore_ascii_case("select")) {
            self.cursors.clear();
        }
        let outcome = self.stream(query, types, sink, cancel, OnStop::Cancel)?;
        self.in_tx = database::run::transaction_after(query, self.flavor(), self.in_tx);
        Ok(outcome)
    }

    fn console(&self) -> Option<ConsoleCaps> {
        Some(ConsoleCaps { profile: profile::caps(self.version), ..console::CONSOLE })
    }

    /// A query from a cursor when asked (its page fetched, one row more kept to know the
    /// rest); otherwise streamed, a write's rows past the page read to its end.
    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        self.release_cursor();
        self.take_notices();
        let read = console::statement(statement, self.version);
        let mut outcome = None;
        if options.keep_cursor && read.cursorable() {
            outcome = self.run_cursor(&read, options, sink, cancel)?;
        }
        let mut outcome = match outcome {
            Some(outcome) => outcome,
            None => self.run_plain(statement, &read, options, sink, cancel)?,
        };
        self.in_tx = database::run::transaction_after(statement, self.flavor(), self.in_tx);
        outcome.notices = self.take_notices();
        Ok(outcome)
    }

    fn fetch(
        &mut self,
        cursor: &CursorToken,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        if self.cursor.as_ref().is_none_or(|c| c.name != cursor.0) {
            return Err(DbError::Query("that result's cursor was closed".to_owned()));
        }
        self.take_notices();
        let mut outcome = self.fetch_page(rows, sink, cancel).map_err(|(e, _)| e)?;
        outcome.notices = self.take_notices();
        Ok(outcome)
    }

    fn close_cursor(&mut self, cursor: &CursorToken) -> Result<(), DbError> {
        if self.cursor.as_ref().is_some_and(|c| c.name == cursor.0) {
            self.release_cursor();
        }
        Ok(())
    }

    /// The profile's projection: `EXPLAIN (FORMAT JSON)`, or as it runs (`analyze`: a write
    /// undone).
    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        let depth =
            if analyze { ProfileDepth::Actual { timing: true } } else { ProfileDepth::Estimated };
        let options = ProfileOptions { settings: false, ..ProfileOptions::of(depth) };
        Ok(self.profile(statement, &options, cancel)?.plan_nodes())
    }

    /// `EXPLAIN (…, FORMAT JSON)` with what the version takes ([`profile::options`]). Run, it
    /// goes in a transaction of its own — read-only for a read (a function that writes,
    /// called from a `SELECT`, fails instead) — or a savepoint inside the user's, rolled back
    /// at the end with its `SET LOCAL statement_timeout`: nothing it did stays.
    fn profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        self.release_cursor();
        let read = console::statement(statement, self.version);
        let runs = options.depth.runs();
        if runs && let Some(words) = database::profile::actual_refused(read.body, self.flavor()) {
            return Err(DbError::Unsupported(format!(
                "profiling {words} as it runs: it can't run inside a transaction"
            )));
        }
        let writes = read.kind.writes();
        // The query's history (14+: by the server's `Query Identifier`, which `VERBOSE` gives).
        let history = options.statement_history && self.version >= 140_000;
        let extension = if history { self.statement_stats_schema() } else { None };
        let asked = ProfileOptions { verbose: options.verbose || extension.is_some(), ..*options };
        let prefix = profile::options(self.version, &asked, writes);
        let sql = format!("{prefix}{}", read.body);
        let mut io_timing = None;
        if runs {
            if self.in_tx {
                self.simple(&format!("SAVEPOINT {}", profile::SAVEPOINT))?;
            } else {
                self.simple(if writes || read.locks { "BEGIN" } else { "BEGIN READ ONLY" })?;
            }
            let prepared = (|| {
                if let Some(timeout) = options.timeout {
                    self.simple(&format!(
                        "SET LOCAL statement_timeout = {}",
                        timeout.as_millis().max(1)
                    ))?;
                }
                if options.buffers {
                    io_timing = self.show("track_io_timing");
                }
                Ok::<(), DbError>(())
            })();
            if let Err(e) = prepared {
                self.undo_profile();
                return Err(e);
            }
        }
        let mut rows = Collect::default();
        let result = self.stream(&sql, None, &mut rows, cancel, OnStop::Cancel);
        if runs {
            self.undo_profile();
        }
        result.map_err(|e| console::shifted(e, prefix.chars().count(), read.leading))?;
        let json: String = rows.rows.iter().filter_map(|r| r.first()).map(Value::display).collect();
        let mut p = profile::parse(&json, self.block_size)?;
        p.engine = Engine {
            driver: ID.to_owned(),
            name: "PostgreSQL".to_owned(),
            version: self.server.strip_prefix("PostgreSQL ").map(str::to_owned),
            version_num: Some(self.version),
        };
        p.depth = options.depth;
        let caps = profile::caps(self.version);
        let has = |f: &dyn Fn(&database::profile::ProfileNode) -> bool| p.nodes.iter().any(f);
        p.collected = database::ProfileCaps {
            estimated: true,
            actual: runs,
            node_timing: options.depth == (ProfileDepth::Actual { timing: true }),
            buffers: options.buffers && (runs || self.version >= 130_000),
            io_timing: has(&|n| n.io.is_some_and(|i| i.read_ms.is_some() || i.write_ms.is_some())),
            wal: options.wal && runs && writes && caps.wal,
            memory: has(&|n| n.memory.is_some()),
            spill: has(&|n| n.spill.is_some()),
            parallel: has(&|n| n.parallel.is_some()),
            settings: options.settings && caps.settings,
            verbose_output: options.verbose,
            jit: p.statement.jit.is_some(),
            triggers: runs,
            undo_writes: runs && writes,
            ..database::ProfileCaps::NONE
        };
        if runs && writes {
            p.note(ProfileNote::RolledBack);
        }
        if runs && options.buffers && io_timing.as_deref().is_some_and(|v| v == "off") {
            p.note(ProfileNote::IoTimingOff);
        }
        if options.depth == (ProfileDepth::Actual { timing: true }) {
            p.note(ProfileNote::InstrumentationOverhead);
        }
        if !p.statement.settings.is_empty() {
            p.note(ProfileNote::PlannerSettingsChanged);
        }
        if extension.is_some() && !options.verbose {
            // Asked only for the query's identifier: the columns weren't.
            for node in &mut p.nodes {
                node.output.clear();
            }
        }
        if history {
            match &extension {
                Some(schema) => {
                    p.statement.history = p
                        .statement
                        .query_id
                        .filter(|id| *id != 0)
                        .and_then(|id| self.read_history(schema, id, &mut p));
                    p.collected.statement_history = p.statement.history.is_some();
                }
                None => p.note(ProfileNote::NoStatementStats),
            }
        }
        if runs && !writes {
            p.statement.rows_returned =
                p.roots.first().and_then(|r| p.node(*r).total_rows()).map(|r| r.round() as u64);
        }
        p.keep_raw(RawFormat::Json, &json, options.max_raw_bytes);
        Ok(p)
    }

    /// One query for the tables (`pg_class`, `pg_stat_user_tables`, their sizes), one for
    /// their indexes ([`stats`]); inside the user's transaction under a savepoint of its own.
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
        self.release_cursor();
        let version = self.version;
        let params = stats::params(tables);
        let refs: Vec<&str> = params.iter().map(String::as_str).collect();
        let (rows, indexes) = self.guarded(|s| {
            let rows = s.catalog(&stats::tables_sql(version, tables.len()), &refs)?;
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            let indexes = s.catalog(&stats::indexes_sql(version, tables.len()), &refs)?;
            Ok((rows, indexes))
        })?;
        Ok(stats::assemble(&rows, &indexes))
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        if let Some(kind @ (Isolation::Deferred | Isolation::Immediate | Isolation::Exclusive)) =
            options.isolation
        {
            return Err(DbError::Unsupported(format!("{} in PostgreSQL", kind.id())));
        }
        self.release_cursor();
        self.simple(&database::run::begin_statement("BEGIN", options))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.release_cursor();
        self.cursors.clear();
        // Failed or not, the transaction is over (a failed one rolled back).
        self.in_tx = false;
        self.simple("COMMIT")
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        self.release_cursor();
        self.cursors.clear();
        self.in_tx = false;
        self.simple("ROLLBACK")
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    /// `search_path`: `schema`, then `public`.
    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        self.release_cursor();
        let quoted = self.dialect().quote_identifier(schema)?;
        self.simple(&console::search_path(schema, &quoted))
    }

    fn current(&mut self) -> Result<Place, DbError> {
        let rows = self.catalog("SELECT current_database()::text, current_schema()::text", &[])?;
        let row = rows.into_iter().next().unwrap_or_default();
        let mut row = row.into_iter();
        Ok(Place { database: row.next().flatten(), schema: row.next().flatten() })
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(self
            .catalog(catalog::databases(), &[])?
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }

    /// One query over `pg_attribute`, `pg_class` and `pg_namespace` (as 9.2 reads them).
    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = catalog::catalog_columns(self.version, schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        let mut all: Vec<TableColumns> = Vec::new();
        for r in self.catalog(&sql, &params)? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let table = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let column = ColumnBrief {
                name: text(2).unwrap_or_default(),
                type_name: text(3).unwrap_or_default(),
                nullable: text(4).as_deref() == Some("true"),
                default: text(5),
                comment: text(6),
                key_position: text(7).and_then(|k| k.parse().ok()),
                values: text(8).map(|labels| catalog::enum_labels(&labels)),
            };
            match all.last_mut() {
                Some((last, columns)) if *last == table => columns.push(column),
                _ => all.push((table, vec![column])),
            }
        }
        Ok(all)
    }

    /// From `pg_constraint`, every key at once (see [`catalog::catalog_keys`]).
    fn catalog_relations(&mut self, cancel: &Cancel) -> Result<Vec<TableRef>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        Ok(self
            .catalog(&catalog::relations(self.version), &[])?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                TableRef { schema: text(0), name: text(1).unwrap_or_default() }
            })
            .collect())
    }

    fn catalog_keys(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = catalog::catalog_keys(schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        let mut all: Vec<database::TableKeys> = Vec::new();
        let mut last_key: Option<(TableRef, String)> = None;
        for r in self.catalog(&sql, &params)? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let table = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let name = text(2).unwrap_or_default();
            if all.last().is_none_or(|(t, _)| *t != table) {
                all.push((table.clone(), Vec::new()));
            }
            let Some((_, keys)) = all.last_mut() else { continue };
            let same = last_key.as_ref().is_some_and(|(t, n)| *t == table && *n == name);
            if !same {
                keys.push(database::ForeignKey {
                    name: Some(name.clone()),
                    columns: Vec::new(),
                    table: TableRef { schema: text(3), name: text(4).unwrap_or_default() },
                    references: Vec::new(),
                    on_update: catalog::action(&text(8).unwrap_or_default()),
                    on_delete: catalog::action(&text(9).unwrap_or_default()),
                });
                last_key = Some((table, name));
            }
            if let Some(key) = keys.last_mut() {
                key.columns.extend(text(5));
                key.references.extend(text(6));
            }
        }
        Ok(all)
    }

    /// From `pg_proc` (`prokind` from 11, `proisagg` / `proiswindow` before).
    fn routines(&mut self, schema: Option<&str>, cancel: &Cancel) -> Result<Vec<Routine>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = catalog::routines(self.version, schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        Ok(self
            .catalog(&sql, &params)?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                let variadic = text(6).as_deref() == Some("true");
                Routine {
                    schema: text(0),
                    name: text(1).unwrap_or_default(),
                    arguments: text(2).unwrap_or_default(),
                    result: text(3),
                    kind: match text(4).as_deref() {
                        Some("p") => RoutineKind::Procedure,
                        Some("a") => RoutineKind::Aggregate,
                        Some("w") => RoutineKind::Window,
                        _ => RoutineKind::Function,
                    },
                    arity: if variadic { None } else { text(5).and_then(|n| n.parse().ok()) },
                    builtin: text(7).as_deref() == Some("true"),
                }
            })
            .collect())
    }
}

impl PgSession {
    fn flavor(&self) -> Flavor {
        Flavor::Postgres { version: Some(self.version) }
    }

    /// Statements run for their effect alone.
    fn simple(&self, sql: &str) -> Result<(), DbError> {
        self.runtime.block_on(self.client.batch_execute(sql)).map_err(error)
    }

    /// A setting's value (`SHOW name`), `None` if it can't be read.
    fn show(&self, name: &str) -> Option<String> {
        let mut rows = Collect::default();
        self.stream(&format!("SHOW {name}"), None, &mut rows, &Cancel::default(), OnStop::Cancel)
            .ok()?;
        rows.rows.first().and_then(|r| r.first()).map(Value::display)
    }

    /// A profile's run undone: its savepoint inside the user's transaction, or its own
    /// transaction (a failure leaves nothing more to undo).
    fn undo_profile(&mut self) {
        if self.in_tx {
            let point = profile::SAVEPOINT;
            let _ = self.simple(&format!("ROLLBACK TO SAVEPOINT {point}"));
            let _ = self.simple(&format!("RELEASE SAVEPOINT {point}"));
        } else {
            let _ = self.simple("ROLLBACK");
        }
    }

    /// `work` run as is, or inside the user's transaction under [`stats::SAVEPOINT`]
    /// (undone if it fails, so an auxiliary query never aborts the user's transaction).
    fn guarded<T>(&mut self, work: impl FnOnce(&Self) -> Result<T, DbError>) -> Result<T, DbError> {
        if !self.in_tx {
            return work(self);
        }
        let point = stats::SAVEPOINT;
        self.simple(&format!("SAVEPOINT {point}"))?;
        let result = work(self);
        if result.is_err() {
            let _ = self.simple(&format!("ROLLBACK TO SAVEPOINT {point}"));
        }
        let _ = self.simple(&format!("RELEASE SAVEPOINT {point}"));
        result
    }

    /// Where `pg_stat_statements` is installed (`None`: it isn't, or it can't be told),
    /// read once.
    fn statement_stats_schema(&mut self) -> Option<String> {
        if let Some(known) = &self.statement_stats {
            return known.clone();
        }
        let found = self.guarded(|s| s.catalog(stats::EXTENSION_SQL, &[]));
        let schema = match found {
            Ok(rows) => rows.into_iter().next().and_then(|r| r.get(1).cloned().flatten()),
            Err(e) => {
                tracing::debug!(error = %e, "pg_stat_statements wasn't looked for");
                None
            }
        };
        self.statement_stats = Some(schema.clone());
        schema
    }

    /// The server's history of query `id` (`pg_stat_statements` in `schema`); a failure
    /// leaves it unknown (the extension not loaded noted as missing).
    fn read_history(
        &mut self,
        schema: &str,
        id: i64,
        profile: &mut Profile,
    ) -> Option<database::profile::StatementStats> {
        let sql = stats::history_sql(self.version, schema)?;
        let id = id.to_string();
        match self.guarded(|s| s.catalog(&sql, &[id.as_str()])) {
            Ok(rows) => rows.first().and_then(|r| stats::history_row(r)),
            Err(e) => {
                let text = e.to_string();
                if text.contains("shared_preload_libraries") {
                    profile.note(ProfileNote::NoStatementStats);
                }
                tracing::debug!(%text, "the query's history wasn't read");
                None
            }
        }
    }

    fn take_notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect()
    }

    /// The columns' types of `query` (one statement, prepared; `None`: it can't be).
    fn types(&self, query: &str, cancel: &Cancel) -> Result<Option<Vec<Column>>, DbError> {
        let (client, token, transport) = (&self.client, &self.cancel, &self.transport);
        // Preparing waits for the table's locks too: cancelled as a run is.
        let prepared = self.runtime.block_on(async move {
            let prepare = client.prepare(query);
            futures_util::pin_mut!(prepare);
            let mut tick = tokio::time::interval(CANCEL_CHECK);
            let mut cancelled = false;
            loop {
                tokio::select! {
                    prepared = &mut prepare => return prepared.ok(),
                    _ = tick.tick() => {
                        if cancel.is_cancelled() && !cancelled {
                            cancelled = true;
                            let _ = match transport {
                                Transport::Plain => token.cancel_query(NoTls).await,
                                Transport::Tls(tls) => token.cancel_query(tls.clone()).await,
                            };
                        }
                    }
                }
            }
        });
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        Ok(prepared.map(|s| {
            s.columns()
                .iter()
                .map(|c| Column {
                    name: c.name().to_owned(),
                    type_name: c.type_().name().to_owned(),
                })
                .collect()
        }))
    }

    /// The console's cursor closed (its own transaction with it); a failure leaves it gone
    /// all the same (an aborted transaction closed it).
    fn release_cursor(&mut self) {
        if let Some(cursor) = self.cursor.take() {
            let _ = self.simple(&console::close(&cursor.name));
            if cursor.own_tx {
                let _ = self.simple("COMMIT");
            }
        }
    }

    /// `read` from a cursor: declared (in a read-only transaction of its own outside the
    /// user's), its first page fetched. `None`: it wasn't for a cursor after all (a write
    /// refused in the read-only transaction, before any row): run it plainly.
    fn run_cursor(
        &mut self,
        read: &console::Statement<'_>,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Option<RunOutcome>, DbError> {
        let own_tx = !self.in_tx;
        let types = self.types(read.body, cancel)?;
        if own_tx {
            self.simple("BEGIN READ ONLY")?;
        }
        let name = console::cursor_name(self.next_cursor);
        self.next_cursor += 1;
        let (declare, prefix) = console::declare(&name, read.body);
        if let Err(e) = self.simple(&declare) {
            if own_tx {
                let _ = self.simple("ROLLBACK");
                if read_only_refusal(&e) {
                    return Ok(None);
                }
            }
            return Err(console::shifted(e, prefix, read.leading));
        }
        self.cursor =
            Some(OpenCursor { name, columns: types.unwrap_or_default(), pending: None, own_tx });
        match self.fetch_page(options.fetch_limit, sink, cancel) {
            Ok(outcome) => Ok(Some(outcome)),
            Err((e, 0)) if own_tx && read_only_refusal(&e) => Ok(None),
            Err((e, _)) => Err(e),
        }
    }

    /// The cursor's next `rows` rows to `sink` (the one kept from before first), one more
    /// read and kept to know the rest; closed once none are left. Failing: the error and
    /// the rows given before (the cursor gone, its own transaction rolled back).
    fn fetch_page(
        &mut self,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, (DbError, u64)> {
        let Some(cursor) = self.cursor.as_mut() else {
            return Err((DbError::Query("no cursor open".to_owned()), 0));
        };
        let mut page = PageSink {
            sink,
            announced: false,
            limit: u64::from(rows.max(1)),
            delivered: 0,
            read: 0,
            pending: None,
            stopped: false,
        };
        if !cursor.columns.is_empty() {
            page.columns(&cursor.columns);
        }
        if let Some(row) = cursor.pending.take() {
            page.deliver(row);
        }
        let need = page.limit - page.delivered + 1;
        let (name, types) = (cursor.name.clone(), cursor.columns.clone());
        let types = (!types.is_empty()).then_some(types);
        let result =
            self.stream(&console::fetch(&name, need), types, &mut page, cancel, OnStop::Drain);
        let (delivered, read, pending, stopped) =
            (page.delivered, page.read, page.pending, page.stopped);
        if let Err(e) = result {
            if let Some(cursor) = self.cursor.take()
                && cursor.own_tx
            {
                let _ = self.simple("ROLLBACK");
            }
            return Err((e, delivered));
        }
        let left = read >= need && !stopped;
        if left {
            if let Some(cursor) = self.cursor.as_mut() {
                cursor.pending = pending;
            }
        } else {
            self.release_cursor();
        }
        Ok(RunOutcome {
            affected: None,
            rows: delivered,
            truncated: left,
            more: left.then_some(CursorToken(name)),
            stopped,
            notices: Vec::new(),
        })
    }

    /// `statement` streamed as it is (no cursor): a single query stopped one row past the
    /// page, anything else read to its end, the rows past the page dropped.
    fn run_plain(
        &mut self,
        statement: &str,
        read: &console::Statement<'_>,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let query = read.kind == StatementKind::Query && !read.several;
        let types = if read.several { None } else { self.types(statement, cancel)? };
        if !query {
            self.cursors.clear();
        }
        let overflow = if query { Overflow::Stop } else { Overflow::Drop };
        let mut limited = Limited::new(sink, options.fetch_limit, overflow);
        let outcome = self.stream(statement, types, &mut limited, cancel, OnStop::Cancel)?;
        Ok(RunOutcome {
            affected: outcome.affected,
            rows: limited.delivered,
            truncated: limited.truncated,
            more: None,
            stopped: limited.stopped,
            notices: Vec::new(),
        })
    }

    /// Runs `query` through the simple protocol (every value as text, typed by `types` when
    /// they fit the set), its rows to `sink`; `cancel` asks the server to stop, and so does
    /// the sink's stop with [`OnStop::Cancel`].
    fn stream(
        &self,
        query: &str,
        types: Option<Vec<Column>>,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
        on_stop: OnStop,
    ) -> Result<Outcome, DbError> {
        let (client, token, transport) = (&self.client, &self.cancel, &self.transport);
        self.runtime.block_on(async move {
            let stream = client.simple_query_raw(query).await.map_err(error)?;
            futures_util::pin_mut!(stream);
            let mut outcome = Outcome::default();
            let mut columns: Option<Vec<Column>> = None;
            let mut done_set = false;
            let mut tick = tokio::time::interval(CANCEL_CHECK);
            let mut cancelled = false;
            loop {
                let message = tokio::select! {
                    message = stream.next() => message,
                    _ = tick.tick() => {
                        if cancel.is_cancelled() && !cancelled {
                            cancelled = true;
                            let _ = match transport {
                                Transport::Plain => token.cancel_query(NoTls).await,
                                Transport::Tls(tls) => token.cancel_query(tls.clone()).await,
                            };
                        }
                        continue;
                    }
                };
                match message {
                    None => break,
                    Some(Err(e)) => {
                        let e = error(e);
                        return if cancelled || e == DbError::Cancelled {
                            Err(DbError::Cancelled)
                        } else {
                            Err(e)
                        };
                    }
                    Some(Ok(SimpleQueryMessage::Row(row))) => {
                        // A new result set: its columns first (typed when prepared).
                        if columns.is_none() || done_set {
                            let named: Vec<Column> = row
                                .columns()
                                .iter()
                                .map(|c| Column {
                                    name: c.name().to_owned(),
                                    type_name: String::new(),
                                })
                                .collect();
                            let typed = types
                                .clone()
                                .filter(|t| t.len() == named.len() && columns.is_none());
                            let set = typed.unwrap_or(named);
                            sink.columns(&set);
                            columns = Some(set);
                            done_set = false;
                        }
                        let set = columns.as_deref().unwrap_or_default();
                        let values = (0..row.len())
                            .map(|i| {
                                value(set.get(i).map_or("", |c| c.type_name.as_str()), row.get(i))
                            })
                            .collect();
                        outcome.rows += 1;
                        if sink.row(values) == Flow::Stop && on_stop == OnStop::Cancel {
                            // Enough: the server stops working on the rest.
                            outcome.stopped = true;
                            let _ = match transport {
                                Transport::Plain => token.cancel_query(NoTls).await,
                                Transport::Tls(tls) => token.cancel_query(tls.clone()).await,
                            };
                            break;
                        }
                    }
                    Some(Ok(SimpleQueryMessage::CommandComplete(count))) => {
                        outcome.affected = Some(count);
                        done_set = true;
                    }
                    Some(Ok(_)) => {}
                }
            }
            // A query without rows (DDL, an UPDATE): its columns all the same (none).
            if columns.is_none() {
                sink.columns(&types.unwrap_or_default());
            }
            Ok(outcome)
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
