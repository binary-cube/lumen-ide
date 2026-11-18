//! The port a request is sent through (its adapter is infra's: `http-transport`): what's
//! sent ([`Outgoing`]), how ([`SendOptions`]), what comes back ([`Incoming`]: its status,
//! headers, body — in memory, or in a file when large —, the redirects it went through, how
//! long it took), why not ([`TransportError`]). Called on a background thread; a
//! [`kernel::work::Cancel`] stops it between its reads.

use std::path::PathBuf;
use std::time::Duration;

use crate::cookies::CookieJar;
use crate::model::Header;

/// A request ready to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Vec<OutgoingPart>,
}

/// A piece of what's sent: bytes, or a file read as it's sent (never held whole).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingPart {
    Bytes(Vec<u8>),
    File(PathBuf),
}

/// How a request is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOptions {
    /// Redirects followed (each one seen in [`Incoming::hops`]), at most `max_redirects`.
    pub follow_redirects: bool,
    pub max_redirects: u32,
    /// For the whole exchange; for the connection alone.
    pub timeout: Option<Duration>,
    pub connect_timeout: Option<Duration>,
    /// The server's certificate checked (off only when the user asks, and said so).
    pub verify_tls: bool,
    /// A body larger than this goes to a file in `spill_dir` (no folder: an error past it).
    pub memory_limit: u64,
    pub spill_dir: Option<PathBuf>,
    /// The most a response may be (bytes).
    pub max_bytes: u64,
    /// The client's certificate the server may ask for (the environment's
    /// `SSLConfiguration`).
    pub client_certificate: Option<ClientCertificate>,
    /// HTTP/2, as the request line asks (`HTTP/2`, `HTTP/2 (Prior Knowledge)`).
    pub http2: Http2,
}

/// How a request asks for HTTP/2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Http2 {
    /// HTTP/1.1 (nothing written, `HTTP/1.1`).
    #[default]
    Off,
    /// `HTTP/2`: offered over TLS (ALPN), HTTP/1.1 if the server doesn't take it, and on
    /// plain `http://` (its upgrade is deprecated, RFC 9113).
    Negotiated,
    /// `HTTP/2 (Prior Knowledge)`: HTTP/2 from the first byte, plain `http://` too (h2c).
    PriorKnowledge,
}

impl Http2 {
    /// What the request line's version asks for.
    pub fn written(version: Option<&str>) -> Self {
        let Some(version) = version.map(str::trim) else { return Self::Off };
        let lower = version.to_ascii_lowercase();
        match lower.strip_prefix("http/") {
            Some(rest) if rest.starts_with('2') && lower.contains("prior knowledge") => {
                Self::PriorKnowledge
            }
            Some(rest) if rest.starts_with('2') => Self::Negotiated,
            _ => Self::Off,
        }
    }
}

/// A client certificate as PEM: its chain, its key (both may be in one file: then the same).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCertificate {
    pub chain: Vec<u8>,
    pub key: Vec<u8>,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            follow_redirects: true,
            max_redirects: 10,
            timeout: Some(Duration::from_secs(60)),
            connect_timeout: Some(Duration::from_secs(15)),
            verify_tls: true,
            memory_limit: 8 * 1024 * 1024,
            spill_dir: None,
            max_bytes: 1024 * 1024 * 1024,
            client_certificate: None,
            http2: Http2::Off,
        }
    }
}

/// A response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub status: u16,
    pub reason: String,
    /// `HTTP/1.1`, `HTTP/2`.
    pub version: String,
    pub headers: Vec<Header>,
    pub body: IncomingBody,
    /// The URL it came from (after the redirects).
    pub url: String,
    /// The redirects followed, in order.
    pub hops: Vec<Hop>,
    pub timings: Timings,
}

impl Incoming {
    /// Its header `name` (any case), the first one.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| &*h.value)
    }

    /// Its media type, lowercase, without parameters (`application/json`).
    pub fn media_type(&self) -> Option<String> {
        let value = self.header("content-type")?;
        Some(value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase())
    }
}

/// A response's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingBody {
    Memory(Vec<u8>),
    /// Too large to hold: in this file (the module's to keep or remove).
    File {
        path: PathBuf,
        len: u64,
    },
}

impl IncomingBody {
    pub fn len(&self) -> u64 {
        match self {
            Self::Memory(bytes) => bytes.len() as u64,
            Self::File { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A redirect followed: what answered, and where it sent the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    pub status: u16,
    pub url: String,
    pub location: String,
}

/// How long a request took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timings {
    /// Until the response's headers came (the connection, TLS, the server's work).
    pub headers: Duration,
    /// Until its body was all read.
    pub total: Duration,
}

/// Why a request got no response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Not a URL it can send to.
    Url(String),
    /// No connection: the name not found, refused, unreachable.
    Connect(String),
    /// The server's certificate isn't trusted, or TLS failed.
    Tls(String),
    Timeout,
    /// Past [`SendOptions::max_bytes`] (or the memory limit, with no folder to spill to).
    TooLarge,
    /// More redirects than allowed.
    TooManyRedirects,
    Cancelled,
    /// A file to send couldn't be read, the connection broke…
    Io(String),
    /// Not something this transport does.
    Unsupported(String),
}

/// A message a WebSocket request sends (its `===` blocks): its text, and whether a
/// message from the server is waited for before it goes (`=== wait-for-server`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsMessage {
    pub text: String,
    pub wait_for_server: bool,
}

/// A WebSocket request's body read into its messages: the text before the first `===` is
/// one (if any), then each block after a `===` line.
pub fn ws_messages(body: &str) -> Vec<WsMessage> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut wait = false;
    let push = |text: &mut String, wait: bool, out: &mut Vec<WsMessage>| {
        let message = text.trim_end_matches(['\n', '\r']).to_owned();
        if !message.trim().is_empty() {
            out.push(WsMessage { text: message, wait_for_server: wait });
        }
        text.clear();
    };
    for line in body.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("===") {
            push(&mut current, wait, &mut out);
            wait = rest.trim() == "wait-for-server";
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    push(&mut current, wait, &mut out);
    out
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "{what} isn't supported"),
            Self::Url(why) => write!(f, "invalid URL: {why}"),
            Self::Connect(why) => write!(f, "can't connect: {why}"),
            Self::Tls(why) => write!(f, "TLS: {why}"),
            Self::Timeout => write!(f, "timed out"),
            Self::TooLarge => write!(f, "the response is too large"),
            Self::TooManyRedirects => write!(f, "too many redirects"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Io(why) => write!(f, "{why}"),
        }
    }
}

/// What sends requests.
pub trait HttpTransport: Send + Sync + std::fmt::Debug {
    /// `request` sent with `options`; `cookies` given and taken from (`None`: none — the
    /// adapter tells them the time).
    fn send(
        &self,
        request: &Outgoing,
        options: &SendOptions,
        cookies: Option<&mut CookieJar>,
        cancel: &kernel::work::Cancel,
    ) -> Result<Incoming, TransportError>;

    /// [`Self::send`], each piece of the body told to `piece` as it comes (a stream of
    /// events shown while it lasts).
    fn send_streaming(
        &self,
        request: &Outgoing,
        options: &SendOptions,
        cookies: Option<&mut CookieJar>,
        cancel: &kernel::work::Cancel,
        piece: &dyn Fn(&[u8]),
    ) -> Result<Incoming, TransportError> {
        let _ = piece;
        self.send(request, options, cookies, cancel)
    }

    /// A WebSocket session at `url` (its handshake's `headers`, its TLS as `options` say:
    /// the server's certificate checked or not, the client's): `messages` sent (each after a
    /// message of the server when it waits for one), every message either way told to
    /// `event` (`true`: received), until the server closes or `cancel`. The transcript.
    fn websocket(
        &self,
        url: &str,
        headers: &[crate::model::Header],
        messages: &[WsMessage],
        options: &SendOptions,
        cancel: &kernel::work::Cancel,
        event: &dyn Fn(bool, &str),
    ) -> Result<Vec<(bool, String)>, TransportError> {
        let _ = (url, headers, messages, options, cancel, event);
        Err(TransportError::Unsupported("WebSocket".into()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/transport.rs"]
mod tests;
