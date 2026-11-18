//! How messages cross the pipe: each a JSON-RPC 2.0 object in a frame of its own, as LSP
//! frames them — `Content-Length: <bytes>\r\n\r\n<UTF-8 JSON>` (other headers are allowed
//! and ignored). A request has an `id` and is answered once, by `result` or `error`; a
//! notification (`cancel`, `exit`) has none and isn't answered.

use std::io::{self, BufRead, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use database::{DbError, DriverFailure};

/// The largest frame read (a page of large values fits; more is a broken peer).
pub const MAX_FRAME: usize = 256 * 1024 * 1024;

/// The methods.
pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const CONNECT: &str = "connect";
    pub const CHILDREN: &str = "children";
    pub const TABLES: &str = "tables";
    pub const STRUCTURE: &str = "structure";
    pub const READ: &str = "read";
    pub const COUNT: &str = "count";
    pub const EXECUTE: &str = "execute";
    pub const APPLY: &str = "apply";
    pub const ALTER: &str = "alter";
    pub const PLAN_ALTER: &str = "plan_alter";
    pub const REBUILD_SOURCE: &str = "rebuild_source";
    // The console's (protocol 2).
    pub const RUN: &str = "run";
    pub const FETCH: &str = "fetch";
    pub const CLOSE_CURSOR: &str = "close_cursor";
    pub const EXPLAIN: &str = "explain";
    pub const BEGIN: &str = "begin";
    pub const COMMIT: &str = "commit";
    pub const ROLLBACK: &str = "rollback";
    pub const SET_SCHEMA: &str = "set_schema";
    pub const CURRENT: &str = "current";
    pub const DATABASES: &str = "databases";
    pub const CATALOG_COLUMNS: &str = "catalog_columns";
    pub const ROUTINES: &str = "routines";
    // Profiles (protocol 3).
    pub const PROFILE: &str = "profile";
    pub const TABLE_STATS: &str = "table_stats";
    /// The driver's notification (protocol 2): rows of a `run` or `fetch` as they come.
    pub const ROWS: &str = "$/rows";
    /// The methods a driver of the protocol's first version doesn't know.
    pub const SINCE_2: &[&str] = &[
        RUN,
        FETCH,
        CLOSE_CURSOR,
        EXPLAIN,
        BEGIN,
        COMMIT,
        ROLLBACK,
        SET_SCHEMA,
        CURRENT,
        DATABASES,
        CATALOG_COLUMNS,
        ROUTINES,
    ];
    /// The methods a driver of the protocol's second version doesn't know.
    pub const SINCE_3: &[&str] = &[PROFILE, TABLE_STATS];
    /// Notification: stop the request `{"id": n}` (it answers `cancelled`).
    pub const CANCEL: &str = "cancel";
    /// Notification: the session is over; close it and end (as when stdin closes).
    pub const EXIT: &str = "exit";
}

/// The errors' codes: JSON-RPC's own, then the domain's.
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    pub const CONNECT: i64 = 1;
    pub const AUTH: i64 = 2;
    pub const QUERY: i64 = 3;
    pub const CANCELLED: i64 = 4;
    pub const UNSUPPORTED: i64 = 5;
    pub const CONFLICT: i64 = 6;
    pub const READ_ONLY: i64 = 7;
    pub const IN_USE: i64 = 8;
    /// No protocol version in common: `data` is the driver's `{"min": .., "max": ..}`.
    pub const PROTOCOL: i64 = 9;
    /// A request before `connect` (or a second `connect`).
    pub const NOT_CONNECTED: i64 = 10;
}

/// An error answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ErrorObject {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    /// A domain error as the driver answers it (a refused statement's details in `data`
    /// from protocol 2 on: `protocol`, the version spoken).
    pub fn from_db(error: &DbError, protocol: u32) -> Self {
        if let DbError::Sql(e) = error {
            let mut object = Self::new(code::QUERY, e.message.clone());
            if protocol >= 2 {
                object.data = serde_json::to_value(crate::dto::ErrorData::from(e.as_ref())).ok();
            } else if let Some(detail) = &e.detail {
                object.message = format!("{} ({detail})", e.message);
            }
            return object;
        }
        let (code, message) = match error {
            DbError::Connect(m) => (code::CONNECT, m.clone()),
            DbError::Auth(m) => (code::AUTH, m.clone()),
            DbError::Query(m) => (code::QUERY, m.clone()),
            DbError::Sql(e) => (code::QUERY, e.to_string()),
            DbError::Cancelled => (code::CANCELLED, "cancelled".to_owned()),
            DbError::Unsupported(m) => (code::UNSUPPORTED, m.clone()),
            DbError::Conflict(m) => (code::CONFLICT, m.clone()),
            DbError::ReadOnly(m) => (code::READ_ONLY, m.clone()),
            DbError::InUse(m) => (code::IN_USE, m.clone()),
            DbError::Driver(failure) => (code::INTERNAL, failure.to_string()),
        };
        Self::new(code, message)
    }

    /// The domain error it says, for driver `driver` (`ours`: the editor's versions).
    pub fn to_db(&self, driver: &str, ours: (u32, u32)) -> DbError {
        let m = self.message.clone();
        match self.code {
            code::CONNECT | code::NOT_CONNECTED => DbError::Connect(m),
            code::AUTH => DbError::Auth(m),
            code::QUERY => match self
                .data
                .clone()
                .and_then(|d| serde_json::from_value::<crate::dto::ErrorData>(d).ok())
                .filter(|d| *d != crate::dto::ErrorData::default())
            {
                Some(data) => DbError::sql(data.to_sql(m)),
                None => DbError::Query(m),
            },
            code::CANCELLED => DbError::Cancelled,
            code::UNSUPPORTED | code::METHOD_NOT_FOUND => DbError::Unsupported(m),
            code::CONFLICT => DbError::Conflict(m),
            code::READ_ONLY => DbError::ReadOnly(m),
            code::IN_USE => DbError::InUse(m),
            code::PROTOCOL => {
                let theirs = self
                    .data
                    .clone()
                    .and_then(|d| serde_json::from_value::<crate::dto::Versions>(d).ok())
                    .map_or((0, 0), crate::dto::Versions::pair);
                DbError::Driver(DriverFailure::Protocol { driver: driver.to_owned(), theirs, ours })
            }
            _ => DbError::Query(format!("the driver failed: {m}")),
        }
    }
}

/// A message read: a request, a notification or an answer, its parts borrowed from the
/// frame's bytes (so a password in them is never copied before they're wiped).
#[derive(Debug, Deserialize)]
pub struct Incoming<'a> {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default, borrow)]
    pub method: Option<&'a str>,
    #[serde(default, borrow)]
    pub params: Option<&'a RawValue>,
    #[serde(default, borrow)]
    pub result: Option<&'a RawValue>,
    #[serde(default)]
    pub error: Option<ErrorObject>,
}

#[derive(Serialize)]
struct Request<'a, P> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    method: &'a str,
    params: &'a P,
}

#[derive(Serialize)]
struct Answer<'a, R> {
    jsonrpc: &'static str,
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a ErrorObject>,
}

/// A request (`id`) or a notification (`None`), as JSON.
pub fn request<P: Serialize>(id: Option<u64>, method: &str, params: &P) -> Vec<u8> {
    serde_json::to_vec(&Request { jsonrpc: "2.0", id, method, params }).unwrap_or_default()
}

/// The answer to request `id`.
pub fn answer<R: Serialize>(id: Option<u64>, result: Result<&R, &ErrorObject>) -> Vec<u8> {
    let (result, error) = match result {
        Ok(r) => (Some(r), None),
        Err(e) => (None, Some(e)),
    };
    let body = serde_json::to_vec(&Answer { jsonrpc: "2.0", id, result, error });
    body.unwrap_or_else(|e| {
        // A result that can't be written (a float JSON can't hold was taken care of: this
        // is a bug) is still answered.
        let failed = ErrorObject::new(code::INTERNAL, format!("an answer not written: {e}"));
        serde_json::to_vec(&Answer::<()> { jsonrpc: "2.0", id, result: None, error: Some(&failed) })
            .unwrap_or_default()
    })
}

/// Writes `body` as one frame, flushed.
pub fn write_frame(out: &mut dyn Write, body: &[u8]) -> io::Result<()> {
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(body)?;
    out.flush()
}

/// The next frame's body; `None`: the stream ended between frames.
pub fn read_frame(input: &mut dyn BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length: Option<usize> = None;
    let mut line = Vec::new();
    let mut first = true;
    loop {
        line.clear();
        let read = Read::take(&mut *input, 8 * 1024).read_until(b'\n', &mut line)?;
        if read == 0 {
            return if first {
                Ok(None)
            } else {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "a frame's headers cut short"))
            };
        }
        first = false;
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            break;
        }
        let Some((name, value)) = text.split_once(':') else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a frame's header"));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            let n = value.trim().parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "a frame's length isn't a number")
            })?;
            length = Some(n);
        }
    }
    let Some(length) = length else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a frame without its length"));
    };
    if length > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a frame too large"));
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Floats as JSON numbers; those JSON can't hold as text: `"NaN"`, `"Infinity"`,
/// `"-Infinity"`.
pub mod float {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f64, s: S) -> Result<S::Ok, S::Error> {
        if value.is_nan() {
            s.serialize_str("NaN")
        } else if value.is_infinite() {
            s.serialize_str(if *value > 0.0 { "Infinity" } else { "-Infinity" })
        } else {
            s.serialize_f64(*value)
        }
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Written {
        Number(f64),
        Text(String),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        match Written::deserialize(d)? {
            Written::Number(n) => Ok(n),
            Written::Text(t) => match t.as_str() {
                "NaN" => Ok(f64::NAN),
                "Infinity" => Ok(f64::INFINITY),
                "-Infinity" => Ok(f64::NEG_INFINITY),
                other => Err(serde::de::Error::custom(format!("`{other}` isn't a float"))),
            },
        }
    }
}

/// Bytes as standard base64 (with `=` padding).
pub mod base64 {
    use serde::{Deserialize, Deserializer, Serializer};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for at in 0..4 {
                if at <= chunk.len() {
                    out.push(char::from(ALPHABET[((n >> (18 - 6 * at)) & 63) as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub fn decode(text: &str) -> Result<Vec<u8>, String> {
        let digit = |c: u8| -> Result<u32, String> {
            ALPHABET
                .iter()
                .position(|&a| a == c)
                .map(|p| p as u32)
                .ok_or_else(|| format!("`{}` isn't base64", char::from(c)))
        };
        let text = text.trim_end_matches('=').as_bytes();
        if text.len() % 4 == 1 {
            return Err("base64 cut short".to_owned());
        }
        let mut out = Vec::with_capacity(text.len() * 3 / 4);
        for chunk in text.chunks(4) {
            let mut n = 0u32;
            for (at, &c) in chunk.iter().enumerate() {
                n |= digit(c)? << (18 - 6 * at);
            }
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..chunk.len() - 1]);
        }
        Ok(out)
    }

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        decode(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "../tests/unit/wire.rs"]
mod tests;
