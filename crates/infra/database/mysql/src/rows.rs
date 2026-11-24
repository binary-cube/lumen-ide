//! The rows as they come from the server: a column's type as MySQL writes it, a value typed
//! from its bytes, each row given its key, a query's rows streamed to the sink (killed from
//! another connection when cancelled).

use std::time::Instant;

use database::{Cancel, DbError, Flow, Outcome, RowSink, Value};
use mysql_async::Conn;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::prelude::Queryable;

use crate::{CANCEL_CHECK, Killer, OnStop, console, error_in};

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

/// The text of a value read through the text protocol.
pub(crate) fn text_of(value: Option<mysql_async::Value>) -> Option<String> {
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
pub(crate) async fn stream(
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
