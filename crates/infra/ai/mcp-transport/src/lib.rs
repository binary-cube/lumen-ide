//! The editor's MCP server on the wire (the protocol's "Streamable HTTP", its JSON answers):
//! `POST /mcp` with a JSON-RPC message (or a batch), answered with JSON; `202` for what
//! needs no answer; no event stream (`GET`: `405`, as the protocol allows).
//!
//! Only on the loopback (`127.0.0.1`), each request needing the bearer token the server was
//! started with (compared in constant time), its `Host` the loopback's and its `Origin`, if
//! any, a local page's (DNS rebinding refused). What comes is hostile until checked: headers
//! at most 16 KB, a body at most 4 MB, at most 8 connections at once, each read with a
//! timeout. Connections are accepted on `mcp-server`, each served on its own `mcp-conn`
//! thread (a tool's call may wait for the editor); the handler is called there.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;

/// What answers a message (`None`: nothing to answer, a notification).
pub type Handler = Arc<dyn Fn(&Value) -> Option<Value> + Send + Sync>;

/// The limits on what a client sends.
const HEADERS_MOST: usize = 16 * 1024;
const BODY_MOST: usize = 4 * 1024 * 1024;
const CONNECTIONS_MOST: usize = 8;
/// A connection waiting longer than this for its next request is closed.
const IDLE: Duration = Duration::from_secs(120);
/// How long a request may take to arrive whole once it started (headers and body): a client
/// trickling bytes can't hold a connection for long.
const REQUEST_MOST: Duration = Duration::from_secs(10);
/// The path the protocol is served on.
pub const PATH: &str = "/mcp";

/// A server listening; stopped when dropped.
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").field("port", &self.port).finish_non_exhaustive()
    }
}

impl Server {
    /// Listening on the loopback at `port` (`0`: one the system picks), needing `token`.
    pub fn start(port: u16, token: String, handler: Handler) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let open = Arc::new(AtomicUsize::new(0));
        let token: Arc<str> = token.into();
        std::thread::Builder::new().name("mcp-server".into()).spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if open.fetch_add(1, Ordering::AcqRel) >= CONNECTIONS_MOST {
                    open.fetch_sub(1, Ordering::AcqRel);
                    let _ = respond(&mut &stream, 503, "text/plain", b"busy", false);
                    continue;
                }
                let (handler, token, held, stopped) = (
                    Arc::clone(&handler),
                    Arc::clone(&token),
                    Arc::clone(&open),
                    Arc::clone(&stopped),
                );
                let spawned =
                    std::thread::Builder::new().name("mcp-conn".into()).spawn(move || {
                        serve(stream, port, &token, &handler, &stopped);
                        held.fetch_sub(1, Ordering::AcqRel);
                    });
                if spawned.is_err() {
                    open.fetch_sub(1, Ordering::AcqRel);
                }
            }
        })?;
        Ok(Self { port, stop })
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Its address, as a client is configured with it.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}{PATH}", self.port)
    }
}

impl Drop for Server {
    /// Stopped: the accepting thread woken by one last connection of its own, which it
    /// sees as the end.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// A request as read: its method, path, the headers asked about, its body.
struct Request {
    method: String,
    path: String,
    host: Option<String>,
    origin: Option<String>,
    authorization: Option<String>,
    close: bool,
    body: Vec<u8>,
}

/// Why a request wasn't read.
enum Unread {
    /// The connection ended (or idled out): nothing to answer.
    Gone,
    /// Malformed or too big: answered with this status.
    Refused(u16),
}

/// One request off `reader` (headers and body bounded; whole within [`REQUEST_MOST`] of its
/// first line). `allowed` is asked once the headers are read: a status refuses it before its
/// body is read (nothing of an unknown client's held in memory).
fn read_request(
    reader: &mut BufReader<&TcpStream>,
    allowed: impl Fn(&Request) -> Option<u16>,
) -> Result<Request, Unread> {
    let stream: &TcpStream = reader.get_ref();
    let mut deadline: Option<std::time::Instant> = None;
    // Past the first line, every read waits only for what's left of the deadline.
    let until = |deadline: std::time::Instant| -> Result<(), Unread> {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(Unread::Refused(408));
        }
        stream.set_read_timeout(Some(left)).map_err(|_| Unread::Gone)
    };
    let mut head = Vec::new();
    loop {
        if let Some(deadline) = deadline {
            until(deadline)?;
        }
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take((HEADERS_MOST + 1).saturating_sub(head.len()) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| Unread::Gone)?;
        if n == 0 {
            return Err(if head.is_empty() { Unread::Gone } else { Unread::Refused(400) });
        }
        deadline.get_or_insert_with(|| std::time::Instant::now() + REQUEST_MOST);
        head.extend_from_slice(&line);
        if head.len() > HEADERS_MOST {
            return Err(Unread::Refused(431));
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let head = String::from_utf8(head).map_err(|_| Unread::Refused(400))?;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (Some(method), Some(path)) = (first.next(), first.next()) else {
        return Err(Unread::Refused(400));
    };
    let mut request = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        host: None,
        origin: None,
        authorization: None,
        close: false,
        body: Vec::new(),
    };
    let mut length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_owned();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => request.host = Some(value),
            "origin" => request.origin = Some(value),
            "authorization" => request.authorization = Some(value),
            "connection" => request.close = value.eq_ignore_ascii_case("close"),
            "content-length" => length = value.parse().map_err(|_| Unread::Refused(400))?,
            "transfer-encoding" => return Err(Unread::Refused(501)),
            _ => {}
        }
    }
    if let Some(status) = allowed(&request) {
        return Err(Unread::Refused(status));
    }
    if length > BODY_MOST {
        return Err(Unread::Refused(413));
    }
    if let Some(deadline) = deadline {
        until(deadline)?;
    }
    request.body = vec![0; length];
    reader.read_exact(&mut request.body).map_err(|_| Unread::Gone)?;
    // Between requests, the idle wait again.
    stream.set_read_timeout(Some(IDLE)).map_err(|_| Unread::Gone)?;
    Ok(request)
}

/// Whether `a` and `b` are the same, taking as long whatever differs (a token's compare
/// says nothing of how much of it was right).
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Whether `host` names the loopback at `port` (a page of another site, its name made to
/// point here, says its own).
fn local_host(host: &str, port: u16) -> bool {
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|name| host == format!("{name}:{port}"))
}

/// Whether an `Origin` is a local page's (or a client's: none given).
fn local_origin(origin: Option<&str>) -> bool {
    origin.is_none_or(|o| {
        let rest = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")).unwrap_or("");
        if rest.starts_with("[::1]") {
            return true;
        }
        let host = rest.split([':', '/']).next().unwrap_or("");
        matches!(host, "127.0.0.1" | "localhost")
    })
}

/// Why `request` is refused by its headers alone (not a local client's, no token), if it is.
fn refused(request: &Request, port: u16, token: &str) -> Option<(u16, &'static str)> {
    if !request.host.as_deref().is_some_and(|h| local_host(h, port))
        || !local_origin(request.origin.as_deref())
    {
        return Some((403, "not a local client"));
    }
    let wanted = format!("Bearer {token}");
    if !request.authorization.as_deref().is_some_and(|a| same(a.as_bytes(), wanted.as_bytes())) {
        return Some((401, "a token is needed"));
    }
    None
}

/// What `request` gets: its status, its type, its body.
fn answer(
    request: &Request,
    port: u16,
    token: &str,
    handler: &Handler,
) -> (u16, &'static str, Vec<u8>) {
    let text = |status, body: &str| (status, "text/plain", body.as_bytes().to_vec());
    if let Some((status, why)) = refused(request, port, token) {
        return text(status, why);
    }
    if request.path.split('?').next() != Some(PATH) {
        return text(404, "not here");
    }
    if request.method != "POST" {
        return text(405, "only POST");
    }
    let Ok(message) = serde_json::from_slice::<Value>(&request.body) else {
        return text(400, "not JSON");
    };
    let answers: Vec<Value> = match &message {
        Value::Array(batch) => batch.iter().filter_map(|m| handler(m)).collect(),
        one => handler(one).into_iter().collect(),
    };
    let body = match (message.is_array(), answers.as_slice()) {
        (_, []) => return (202, "text/plain", Vec::new()),
        (false, [one]) => one.to_string(),
        _ => Value::Array(answers).to_string(),
    };
    (200, "application/json", body.into_bytes())
}

fn respond(
    stream: &mut impl Write,
    status: u16,
    kind: &str,
    body: &[u8],
    close: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "Service Unavailable",
    };
    let connection = if close { "close" } else { "keep-alive" };
    // One write: the head and the body together (a client reading once gets it whole).
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: {connection}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    stream.write_all(&response)?;
    stream.flush()
}

/// A connection's requests, one after another, until it closes, idles out or the server
/// stops.
fn serve(stream: TcpStream, port: u16, token: &str, handler: &Handler, stopped: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(IDLE));
    // An answer a client never reads gives up too (one of the few connections' threads).
    let _ = stream.set_write_timeout(Some(IDLE));
    let mut reader = BufReader::new(&stream);
    while !stopped.load(Ordering::Acquire) {
        let read = read_request(&mut reader, |r| refused(r, port, token).map(|(status, _)| status));
        // Stopped while it waited for this one: not answered.
        if stopped.load(Ordering::Acquire) {
            break;
        }
        let (status, kind, body, close) = match read {
            Ok(request) => {
                let (status, kind, body) = answer(&request, port, token, handler);
                (status, kind, body, request.close)
            }
            Err(Unread::Gone) => break,
            Err(Unread::Refused(status)) => (status, "text/plain", Vec::new(), true),
        };
        if respond(&mut &stream, status, kind, &body, close).is_err() || close {
            break;
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
