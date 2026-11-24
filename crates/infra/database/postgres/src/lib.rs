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

use crate::connect::Notices;

pub mod catalog;
pub mod console;
pub mod profile;
pub mod stats;
pub mod tls;

mod connect;
mod reading;
mod running;
mod session;

use std::sync::Arc;
use std::time::Duration;

use database::fields::{Binding, ConnectionSpec, FieldKind, FieldSpec, FieldTab};
use database::tls::{SSLCERT, SSLKEY, SSLMODE, SSLROOTCERT};
use database::{
    Capabilities, Column, DbError, DriverFactory, Filter, Flow, Kind, RowKey, RowSink,
    SchemaChanges, Sorting, SslMode, TableRef, Value,
};
use tokio_postgres::{CancelToken, Client};

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

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
