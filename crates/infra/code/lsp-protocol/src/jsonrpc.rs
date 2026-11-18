//! JSON-RPC 2.0 over a byte stream, as LSP frames it: each message a `Content-Length`
//! header, an empty line, then that many bytes of JSON.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

/// The largest message read (a runaway server can't fill the memory): 64 MB.
pub const MAX_MESSAGE: usize = 64 * 1024 * 1024;

/// A request's identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Id {
    Number(i64),
    Text(String),
}

impl Id {
    pub fn to_value(&self) -> Value {
        match self {
            Self::Number(n) => json!(n),
            Self::Text(s) => json!(s),
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().map(Self::Number),
            Value::String(s) => Some(Self::Text(s.clone())),
            _ => None,
        }
    }
}

/// An error a response carries.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
}

/// The error codes used here.
pub mod codes {
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    /// LSP: the request was cancelled (`$/cancelRequest`).
    pub const REQUEST_CANCELLED: i64 = -32800;
    /// LSP: the content changed since the request was made.
    pub const CONTENT_MODIFIED: i64 = -32801;
}

/// A message read.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request { id: Id, method: String, params: Value },
    Response { id: Id, result: Result<Value, ResponseError> },
    Notification { method: String, params: Value },
}

impl Message {
    /// The message a JSON value is, `None` if it's none.
    pub fn from_value(mut value: Value) -> Option<Self> {
        let method = value.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = value.get("id").and_then(Id::from_value);
        let params = value.get_mut("params").map(Value::take).unwrap_or(Value::Null);
        match (method, id) {
            (Some(method), Some(id)) => Some(Self::Request { id, method, params }),
            (Some(method), None) => Some(Self::Notification { method, params }),
            (None, Some(id)) => {
                let result = match value.get("error") {
                    Some(error) if !error.is_null() => Err(ResponseError {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    }),
                    _ => Ok(value.get_mut("result").map(Value::take).unwrap_or(Value::Null)),
                };
                Some(Self::Response { id, result })
            }
            (None, None) => None,
        }
    }

    /// The message as JSON.
    pub fn to_value(&self) -> Value {
        match self {
            Self::Request { id, method, params } => {
                json!({"jsonrpc": "2.0", "id": id.to_value(), "method": method, "params": params})
            }
            // Without parameters: none said (JSON-RPC's `params` is an object or an array).
            Self::Notification { method, params } if params.is_null() => {
                json!({"jsonrpc": "2.0", "method": method})
            }
            Self::Notification { method, params } => {
                json!({"jsonrpc": "2.0", "method": method, "params": params})
            }
            Self::Response { id, result: Ok(result) } => {
                json!({"jsonrpc": "2.0", "id": id.to_value(), "result": result})
            }
            Self::Response { id, result: Err(e) } => json!({
                "jsonrpc": "2.0",
                "id": id.to_value(),
                "error": {"code": e.code, "message": e.message},
            }),
        }
    }
}

/// Reads the next message's bytes (its JSON); `None` at the end of the stream.
pub fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length: Option<usize> = None;
    let mut line = String::new();
    let mut any = false;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            // The end: cleanly between messages, or cut in a header.
            return if any {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "a header cut short"))
            } else {
                Ok(None)
            };
        }
        any = true;
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            if length.is_some() {
                break;
            }
            // Blank lines before a header (some servers print one): skipped.
            any = false;
            continue;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok();
            if length.is_none() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "a bad Content-Length"));
            }
        }
    }
    let length = length.unwrap_or(0);
    if length > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a message too large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Reads the next message; `None` at the end of the stream. A frame that isn't a message
/// (not JSON, neither a request, a response nor a notification) is `Err(InvalidData)`.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Message>> {
    let Some(body) = read_frame(reader)? else { return Ok(None) };
    let value: Value =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Message::from_value(value)
        .map(Some)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a JSON-RPC message"))
}

/// Writes a message (`value`'s JSON), framed, and flushes it. Returns its size.
pub fn write_message(writer: &mut impl Write, value: &Value) -> io::Result<usize> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(body.len())
}

#[cfg(test)]
#[path = "../tests/unit/jsonrpc.rs"]
mod tests;
