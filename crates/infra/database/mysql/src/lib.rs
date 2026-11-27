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

mod connect;
mod profiling;
mod reading;
mod rows;
mod session;

pub use rows::{type_name, value};

use std::sync::Arc;
use std::time::Duration;

use database::fields::{Binding, ConnectionSpec, FieldKind, FieldSpec, FieldTab};
use database::mysql::ServerVersion;
use database::tls::{SSLCERT, SSLKEY, SSLMODE, SSLROOTCERT};
use database::{
    Capabilities, DbError, DriverFactory, Filter, Kind, SchemaChanges, Sorting, SslMode, TableRef,
    Value,
};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts};

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
        schema_changes: SchemaChanges { reorder: false, schemas: false, ..SchemaChanges::ALL },
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

/// A routine by its schema and name.
type RoutineKey = (Option<String>, String);

/// A catalog query's rows, each value as text (`None`: null).
type TextRows = Vec<Vec<Option<String>>>;

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
