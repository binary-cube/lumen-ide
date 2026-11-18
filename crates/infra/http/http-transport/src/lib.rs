//! The HTTP requests' transport (plan 77): [`http_requests::transport::HttpTransport`]
//! implemented with `ureq`.
//!
//! - Any method (non-standard ones too), the headers as written, a body of bytes and files
//!   read as they're sent (its length told when it's known).
//! - Redirects followed here, one by one, each kept ([`Hop`]): the cookies applied at each,
//!   the user's credentials (`Authorization`, `Proxy-Authorization`, `Cookie`) not sent on
//!   to another host nor from https to http, `303` (and `301` / `302` after a `POST`) a
//!   `GET` without the body, `307` / `308` the same method and body.
//! - The project's cookies sent and kept (`Cookie` written by the user goes first).
//! - TLS verified unless asked not to; the timeouts asked for; the system's proxy from the
//!   environment (`HTTPS_PROXY`…).
//! - The body read in pieces, a cancel looked at between them; past the memory limit it goes
//!   to a file; past the most allowed, refused.
//! - The time until the headers and until the end.
//!
//! Agents are kept by their settings (the TLS roots aren't loaded again for each request),
//! the few used last.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http_requests::cookies::CookieJar;
use http_requests::model::Header;
use http_requests::transport::{
    Hop, HttpTransport, Incoming, IncomingBody, Outgoing, OutgoingPart, SendOptions, Timings,
    TransportError,
};
use http_requests::url::Url;
use kernel::work::Cancel;

mod http2;

/// One exchange's answer, by either protocol: its status, version and headers, its body to
/// read.
struct Answer {
    status: u16,
    reason: String,
    version: String,
    headers: Vec<Header>,
    body: Box<dyn Read + Send>,
}

/// What an agent is made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AgentKey {
    verify_tls: bool,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    /// The client certificate's fingerprint (its bytes hashed), if one is sent.
    certificate: Option<u64>,
}

/// The transport (one for the app: its agents shared by every project).
#[derive(Debug, Default)]
pub struct UreqTransport {
    /// The agents made, the most recently used last (each with its own connections).
    agents: Mutex<Vec<(AgentKey, ureq::Agent)>>,
}

/// The most agents kept: one per `@timeout` / `@connection-timeout` / TLS choice in use, the
/// one used least recently let go past it (with its connections).
const AGENTS: usize = 4;

/// How much is read at a time (a cancel is looked at between).
const CHUNK: usize = 64 * 1024;

impl UreqTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn agent(&self, options: &SendOptions) -> ureq::Agent {
        let key = AgentKey {
            verify_tls: options.verify_tls,
            timeout: options.timeout,
            connect_timeout: options.connect_timeout,
            certificate: options.client_certificate.as_ref().map(|c| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                c.chain.hash(&mut hasher);
                c.key.hash(&mut hasher);
                hasher.finish()
            }),
        };
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        let agent = match agents.iter().position(|(k, _)| *k == key) {
            Some(at) => agents.remove(at).1,
            None => {
                let tls = ureq::tls::TlsConfig::builder()
                    .disable_verification(!key.verify_tls)
                    .client_cert(options.client_certificate.as_ref().and_then(client_cert))
                    .build();
                ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .max_redirects(0)
                    .allow_non_standard_methods(true)
                    .timeout_global(key.timeout)
                    .timeout_connect(key.connect_timeout)
                    .tls_config(tls)
                    .proxy(ureq::Proxy::try_from_env())
                    .user_agent(kernel::brand::named("{app}"))
                    .build()
                    .new_agent()
            }
        };
        agents.push((key, agent.clone()));
        let extra = agents.len().saturating_sub(AGENTS);
        agents.drain(..extra);
        agent
    }

    /// How many agents it keeps (tests).
    #[cfg(test)]
    fn agents(&self) -> usize {
        self.agents.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

/// Seconds since the epoch (the cookies' clock).
#[allow(clippy::disallowed_methods)] // The adapter's clock, for the cookies' expiry.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The body's bytes, read again for each send (a redirect keeping it).
fn body_reader(
    parts: &[OutgoingPart],
) -> Result<(Box<dyn Read + Send>, Option<u64>), TransportError> {
    let mut readers: Vec<Box<dyn Read + Send>> = Vec::new();
    let mut length = Some(0u64);
    for part in parts {
        match part {
            OutgoingPart::Bytes(bytes) => {
                length = length.map(|l| l + bytes.len() as u64);
                readers.push(Box::new(std::io::Cursor::new(bytes.clone())));
            }
            OutgoingPart::File(path) => {
                let file = std::fs::File::open(path)
                    .map_err(|e| TransportError::Io(format!("{}: {e}", path.display())))?;
                length = match (length, file.metadata()) {
                    (Some(l), Ok(meta)) => Some(l + meta.len()),
                    _ => None,
                };
                readers.push(Box::new(file));
            }
        }
    }
    let chained = readers
        .into_iter()
        .fold(Box::new(std::io::empty()) as Box<dyn Read + Send>, |a, b| Box::new(a.chain(b)));
    Ok((chained, length))
}

/// A `ureq` failure, as the port says it.
fn error(e: ureq::Error) -> TransportError {
    match e {
        ureq::Error::BadUri(why) => TransportError::Url(why),
        ureq::Error::Http(why) => TransportError::Url(why.to_string()),
        ureq::Error::Timeout(_) => TransportError::Timeout,
        ureq::Error::HostNotFound => TransportError::Connect("host not found".to_owned()),
        ureq::Error::ConnectionFailed => TransportError::Connect("connection failed".to_owned()),
        ureq::Error::TooManyRedirects => TransportError::TooManyRedirects,
        ureq::Error::BodyExceedsLimit(_) => TransportError::TooLarge,
        ureq::Error::Tls(why) => TransportError::Tls(why.to_owned()),
        ureq::Error::Rustls(why) => TransportError::Tls(why.to_string()),
        ureq::Error::Io(why) if why.kind() == std::io::ErrorKind::TimedOut => {
            TransportError::Timeout
        }
        ureq::Error::Io(why) if why.kind() == std::io::ErrorKind::ConnectionRefused => {
            TransportError::Connect(why.to_string())
        }
        other => TransportError::Io(other.to_string()),
    }
}

/// The method a redirect's next request goes with, and whether the body goes too.
fn after_redirect(status: u16, method: &str) -> (String, bool) {
    match status {
        307 | 308 => (method.to_owned(), true),
        303 => ("GET".to_owned(), false),
        301 | 302 if method.eq_ignore_ascii_case("POST") => ("GET".to_owned(), false),
        _ => (method.to_owned(), true),
    }
}

/// One exchange: `method` to `url` with `headers` and (`with_body`) the body.
struct Exchange<'a> {
    method: &'a str,
    url: &'a str,
    headers: &'a [Header],
    body: Option<&'a [OutgoingPart]>,
}

/// The longest a WebSocket session stays open.
const WS_LONGEST: Duration = Duration::from_secs(30 * 60);
/// How long a WebSocket has to connect (each address) and to finish its handshake.
const WS_CONNECT: Duration = Duration::from_secs(10);
/// The most a session's transcript keeps (its messages, their bytes): the rest is still
/// shown as it comes (`event`), only not kept.
const WS_MESSAGES: usize = 10_000;
const WS_BYTES: usize = 16 * 1024 * 1024;

/// A WebSocket session (see [`HttpTransport::websocket`]): read with a short timeout, so a
/// stop is seen at once.
fn websocket(
    url: &str,
    headers: &[Header],
    messages: &[http_requests::transport::WsMessage],
    tls: Option<Arc<rustls::ClientConfig>>,
    cancel: &Cancel,
    event: &dyn Fn(bool, &str),
) -> Result<Vec<(bool, String)>, TransportError> {
    use tungstenite::client::IntoClientRequest;
    use tungstenite::{Error, Message, stream::MaybeTlsStream};
    let mut request = url.into_client_request().map_err(|e| TransportError::Url(e.to_string()))?;
    for header in headers {
        let name = tungstenite::http::HeaderName::from_bytes(header.name.as_bytes());
        let value = tungstenite::http::HeaderValue::from_str(&header.value);
        if let (Ok(name), Ok(value)) = (name, value) {
            request.headers_mut().insert(name, value);
        }
    }
    // Connected by us (a time to connect, a stop between addresses), the handshake by it.
    let uri = request.uri();
    let host = uri.host().ok_or_else(|| TransportError::Url(url.to_owned()))?;
    let secure = uri.scheme_str() == Some("wss");
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let host = host.trim_start_matches('[').trim_end_matches(']').to_owned();
    let addresses = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
        .map_err(|e| TransportError::Connect(e.to_string()))?;
    let mut stream = None;
    let mut last = String::from("no address");
    for address in addresses {
        if cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        match std::net::TcpStream::connect_timeout(&address, WS_CONNECT) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = e.to_string(),
        }
    }
    let stream = stream.ok_or(TransportError::Connect(last))?;
    let _ = stream.set_read_timeout(Some(WS_CONNECT));
    let _ = stream.set_write_timeout(Some(WS_CONNECT));
    let connector = tls.map(tungstenite::Connector::Rustls);
    let (mut socket, _) = tungstenite::client_tls_with_config(request, stream, None, connector)
        .map_err(|e| match e {
            tungstenite::HandshakeError::Failure(Error::Tls(e)) => {
                TransportError::Tls(e.to_string())
            }
            tungstenite::HandshakeError::Failure(Error::Url(e)) => {
                TransportError::Url(e.to_string())
            }
            other => TransportError::Connect(other.to_string()),
        })?;
    let tick = Some(Duration::from_millis(200));
    let _ = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(tick),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(tick),
        _ => Ok(()),
    };
    let started = Instant::now();
    let mut transcript = Vec::new();
    let mut kept_bytes = 0usize;
    let mut keep = |transcript: &mut Vec<(bool, String)>, from_server: bool, text: &str| {
        kept_bytes = kept_bytes.saturating_add(text.len());
        if transcript.len() < WS_MESSAGES && kept_bytes <= WS_BYTES {
            transcript.push((from_server, text.to_owned()));
        }
    };
    let mut next = 0;
    let mut heard = false;
    loop {
        if cancel.is_cancelled() || started.elapsed() > WS_LONGEST {
            let _ = socket.close(None);
            let _ = socket.flush();
            break;
        }
        // What can go now: each message, one waiting for the server only once it spoke.
        while let Some(message) = messages.get(next) {
            if message.wait_for_server && !heard {
                break;
            }
            socket
                .send(Message::text(message.text.clone()))
                .map_err(|e| TransportError::Io(e.to_string()))?;
            event(false, &message.text);
            keep(&mut transcript, false, &message.text);
            heard = false;
            next += 1;
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                event(true, text.as_str());
                keep(&mut transcript, true, text.as_str());
                heard = true;
            }
            Ok(Message::Binary(bytes)) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                event(true, &text);
                keep(&mut transcript, true, &text);
                heard = true;
            }
            Ok(Message::Close(_)) | Err(Error::ConnectionClosed | Error::AlreadyClosed) => break,
            Ok(_) => {}
            Err(Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(TransportError::Io(e.to_string())),
        }
    }
    Ok(transcript)
}

/// A client certificate read from its PEM (its chain, its key; `None`: none could be).
fn client_cert(
    certificate: &http_requests::transport::ClientCertificate,
) -> Option<ureq::tls::ClientCert> {
    let mut chain = Vec::new();
    let mut key = None;
    for pem in [&certificate.chain, &certificate.key] {
        for item in ureq::tls::parse_pem(pem).flatten() {
            match item {
                ureq::tls::PemItem::Certificate(c) => chain.push(c),
                ureq::tls::PemItem::PrivateKey(k) if key.is_none() => key = Some(k),
                _ => {}
            }
        }
    }
    Some(ureq::tls::ClientCert::new_with_certs(&chain, key?))
}

/// A WebSocket's TLS when the environment's `SSLConfiguration` asks for one of its own
/// (the server's certificate not checked, the client's sent); `None`: the default (the web's
/// roots). Made with the same crypto as the requests' (ring).
fn ws_tls(options: &SendOptions) -> Result<Option<Arc<rustls::ClientConfig>>, TransportError> {
    if options.verify_tls && options.client_certificate.is_none() {
        return Ok(None);
    }
    client_config(options).map(|c| Some(Arc::new(c)))
}

/// A TLS client as the environment's `SSLConfiguration` says: the web's roots, or any
/// server's certificate taken; the client's certificate sent, if given.
fn client_config(options: &SendOptions) -> Result<rustls::ClientConfig, TransportError> {
    let tls = |e: rustls::Error| TransportError::Tls(e.to_string());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(tls)?;
    let builder = if options.verify_tls {
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        builder.with_root_certificates(roots)
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(AnyServer(provider)))
    };
    let config = match &options.client_certificate {
        Some(certificate) => {
            use rustls::pki_types::pem::PemObject;
            use rustls::pki_types::{CertificateDer, PrivateKeyDer};
            let chain: Vec<CertificateDer<'static>> =
                CertificateDer::pem_slice_iter(&certificate.chain).flatten().collect();
            let key = PrivateKeyDer::from_pem_slice(&certificate.key)
                .map_err(|e| TransportError::Tls(e.to_string()))?;
            builder.with_client_auth_cert(chain, key).map_err(tls)?
        }
        None => builder.with_no_client_auth(),
    };
    Ok(config)
}

/// Any server's certificate taken (`verifyHostCertificate: false`, as asked): the handshake's
/// signatures are still checked, only who signed the certificate isn't.
#[derive(Debug)]
struct AnyServer(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AnyServer {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// The URI a Digest answer names: the URL's path and query.
fn digest_uri(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let start = rest.find('/').unwrap_or(rest.len());
    let path = &rest[start..];
    let path = path.split('#').next().unwrap_or(path);
    if path.is_empty() { "/".to_owned() } else { path.to_owned() }
}

/// A header carrying the user's credentials (dropped on a redirect to another host).
fn is_credential(name: &str) -> bool {
    ["authorization", "proxy-authorization", "cookie"].iter().any(|c| name.eq_ignore_ascii_case(c))
}

/// The headers an exchange sends: as written, the cookies the user wrote first and the
/// jar's after, in one `Cookie`.
fn request_headers(exchange: &Exchange<'_>, cookie: Option<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut user_cookie = None;
    for header in exchange.headers {
        if header.name.eq_ignore_ascii_case("cookie") {
            user_cookie = Some(header.value.clone());
            continue;
        }
        out.push((header.name.clone(), header.value.clone()));
    }
    let cookie = match (user_cookie, cookie) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    };
    out.extend(cookie.map(|c| ("Cookie".to_owned(), c)));
    out
}

impl UreqTransport {
    /// One exchange: over HTTP/2 when asked and taken ([`http2`]), else ureq's HTTP/1.1.
    fn exchange(
        &self,
        agent: &ureq::Agent,
        exchange: &Exchange<'_>,
        cookie: Option<String>,
        (options, started): (&SendOptions, Instant),
    ) -> Result<Answer, TransportError> {
        let headers = request_headers(exchange, cookie);
        if let Some(answer) = http2::exchange(exchange, &headers, options, started)? {
            return Ok(answer);
        }
        let response = self.exchange_http1(agent, exchange, &headers)?;
        let reason = response.status().canonical_reason().unwrap_or_default().to_owned();
        let answer_headers = headers_of(&response);
        let (parts, body) = response.into_parts();
        Ok(Answer {
            status: parts.status.as_u16(),
            reason,
            version: format!("{:?}", parts.version),
            headers: answer_headers,
            body: Box::new(body.into_with_config().limit(u64::MAX).reader()),
        })
    }

    fn exchange_http1(
        &self,
        agent: &ureq::Agent,
        exchange: &Exchange<'_>,
        headers: &[(String, String)],
    ) -> Result<ureq::http::Response<ureq::Body>, TransportError> {
        let mut builder = ureq::http::Request::builder().method(exchange.method).uri(exchange.url);
        let mut has_length = false;
        for (name, value) in headers {
            has_length |= name.eq_ignore_ascii_case("content-length");
            builder = builder.header(name.as_str(), value.as_str());
        }
        let response = match exchange.body.filter(|parts| !parts.is_empty()) {
            Some(parts) => {
                let (reader, length) = body_reader(parts)?;
                if let (Some(length), false) = (length, has_length) {
                    builder = builder.header("Content-Length", length.to_string());
                }
                let request = builder
                    .body(ureq::SendBody::from_owned_reader(reader))
                    .map_err(|e| TransportError::Url(e.to_string()))?;
                agent.run(request)
            }
            None => {
                let request = builder.body(()).map_err(|e| TransportError::Url(e.to_string()))?;
                agent.run(request)
            }
        };
        response.map_err(error)
    }
}

/// The response's body read: in memory up to the limit, then in a file; past the most
/// allowed, refused; a cancel looked at between reads.
/// A body being written to a file: the file removed unless it's kept (an error, a cancel,
/// a panic midway leave nothing behind).
struct Spill {
    path: PathBuf,
    out: std::fs::File,
    kept: bool,
}

impl Spill {
    fn create(dir: &Path) -> Result<Self, TransportError> {
        std::fs::create_dir_all(dir).map_err(|e| TransportError::Io(e.to_string()))?;
        let path = dir.join(format!("{}.body", kernel::id::uuid_text(kernel::id::new_uuid())));
        let out = std::fs::File::create(&path).map_err(|e| TransportError::Io(e.to_string()))?;
        Ok(Self { path, out, kept: false })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.out.write_all(bytes).map_err(|e| TransportError::Io(e.to_string()))
    }

    fn keep(mut self) -> PathBuf {
        self.kept = true;
        std::mem::take(&mut self.path)
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        if !self.kept {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn read_body(
    reader: &mut dyn Read,
    options: &SendOptions,
    cancel: &Cancel,
    piece: &dyn Fn(&[u8]),
) -> Result<IncomingBody, TransportError> {
    let mut memory = Vec::new();
    let mut file: Option<Spill> = None;
    let mut total = 0u64;
    let mut chunk = vec![0u8; CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        let n = reader.read(&mut chunk).map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => TransportError::Timeout,
            _ => TransportError::Io(e.to_string()),
        })?;
        if n == 0 {
            break;
        }
        piece(&chunk[..n]);
        total += n as u64;
        if total > options.max_bytes {
            return Err(TransportError::TooLarge);
        }
        match &mut file {
            Some(spill) => spill.write(&chunk[..n])?,
            None if total > options.memory_limit => {
                let Some(dir) = &options.spill_dir else { return Err(TransportError::TooLarge) };
                let mut spill = Spill::create(dir)?;
                spill.write(&memory)?;
                spill.write(&chunk[..n])?;
                memory = Vec::new();
                file = Some(spill);
            }
            None => memory.extend_from_slice(&chunk[..n]),
        }
    }
    Ok(match file {
        Some(spill) => IncomingBody::File { path: spill.keep(), len: total },
        None => IncomingBody::Memory(memory),
    })
}

fn headers_of(response: &ureq::http::Response<ureq::Body>) -> Vec<Header> {
    response
        .headers()
        .iter()
        .map(|(name, value)| Header {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect()
}

impl HttpTransport for UreqTransport {
    fn send(
        &self,
        request: &Outgoing,
        options: &SendOptions,
        cookies: Option<&mut CookieJar>,
        cancel: &Cancel,
    ) -> Result<Incoming, TransportError> {
        self.send_streaming(request, options, cookies, cancel, &|_| {})
    }

    fn websocket(
        &self,
        url: &str,
        headers: &[Header],
        messages: &[http_requests::transport::WsMessage],
        options: &SendOptions,
        cancel: &Cancel,
        event: &dyn Fn(bool, &str),
    ) -> Result<Vec<(bool, String)>, TransportError> {
        let tls = ws_tls(options)?;
        websocket(url, headers, messages, tls, cancel, event)
    }

    fn send_streaming(
        &self,
        request: &Outgoing,
        options: &SendOptions,
        mut cookies: Option<&mut CookieJar>,
        cancel: &Cancel,
        piece: &dyn Fn(&[u8]),
    ) -> Result<Incoming, TransportError> {
        let agent = self.agent(options);
        let started = Instant::now();
        let mut method = request.method.clone();
        let mut url = request.url.clone();
        let mut with_body = true;
        let mut headers = request.headers.clone();
        // `Authorization: Digest user password`: answered when the server challenges it.
        let mut digest = headers
            .iter()
            .position(|h| h.name.eq_ignore_ascii_case("authorization"))
            .and_then(|at| {
                let credentials = http_requests::digest::credentials(&headers[at].value)?;
                headers.remove(at);
                Some(credentials)
            });
        let mut digest_tried = false;
        let first = Url::parse(&url);
        let first_secure = first.as_ref().is_some_and(Url::is_secure);
        let first_host = first.map(|u| u.host);
        let mut hops = Vec::new();
        loop {
            if cancel.is_cancelled() {
                return Err(TransportError::Cancelled);
            }
            let parsed = Url::parse(&url).ok_or_else(|| TransportError::Url(url.clone()))?;
            let cookie = cookies.as_deref().and_then(|jar| jar.header_for(&parsed, now()));
            let exchange = Exchange {
                method: &method,
                url: &url,
                headers: &headers,
                body: with_body.then_some(request.body.as_slice()),
            };
            let mut response = self.exchange(&agent, &exchange, cookie, (options, started))?;
            let at_headers = started.elapsed();
            let status = response.status;
            let response_headers = std::mem::take(&mut response.headers);
            if let Some(jar) = cookies.as_deref_mut() {
                for header in
                    response_headers.iter().filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
                {
                    jar.store(&parsed, &header.value, now());
                }
            }
            let location = response_headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("location"))
                .map(|h| h.value.clone());
            // Challenged for the Digest credentials it has: asked again, once, answering it.
            if status == 401
                && !digest_tried
                && let Some((user, password)) = &digest
                && let Some(challenge) = response_headers
                    .iter()
                    .filter(|h| h.name.eq_ignore_ascii_case("www-authenticate"))
                    .find_map(|h| http_requests::digest::challenge(&h.value))
            {
                digest_tried = true;
                let uri = digest_uri(&url);
                let cnonce = format!("{:016x}", kernel::id::new_uuid() as u64);
                let answer = http_requests::digest::authorization(
                    &challenge,
                    (user, password),
                    (&method, &uri),
                    &cnonce,
                    1,
                );
                headers.retain(|h| !h.name.eq_ignore_ascii_case("authorization"));
                headers.push(Header { name: "Authorization".into(), value: answer });
                continue;
            }
            let redirect = matches!(status, 301 | 302 | 303 | 307 | 308);
            if let (true, true, Some(location)) = (options.follow_redirects, redirect, location) {
                if hops.len() as u32 >= options.max_redirects {
                    return Err(TransportError::TooManyRedirects);
                }
                let next = parsed.join(&location);
                hops.push(Hop { status, url: url.clone(), location: next.clone() });
                let (next_method, keep_body) = after_redirect(status, &method);
                method = next_method;
                with_body &= keep_body;
                if !keep_body {
                    headers.retain(|h| {
                        !h.name.eq_ignore_ascii_case("content-type")
                            && !h.name.eq_ignore_ascii_case("content-length")
                    });
                }
                // Credentials written by the user (a password, a session) stay with the host
                // they were written for, and never go from https to plain http.
                let to = Url::parse(&next);
                let other_host = to.as_ref().map(|u| &u.host) != first_host.as_ref();
                let downgraded = first_secure && !to.as_ref().is_some_and(Url::is_secure);
                if other_host || downgraded {
                    headers.retain(|h| !is_credential(&h.name));
                    // The Digest password too: no other host is answered with it.
                    digest = None;
                }
                url = next;
                continue;
            }
            let body = read_body(&mut response.body, options, cancel, piece)?;
            return Ok(Incoming {
                status,
                reason: response.reason,
                version: response.version,
                headers: response_headers,
                body,
                url,
                hops,
                timings: Timings { headers: at_headers, total: started.elapsed() },
            });
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
