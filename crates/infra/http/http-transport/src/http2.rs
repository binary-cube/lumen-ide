//! HTTP/2 (plan 77), for a request whose line asks for it (`HTTP/2`, `HTTP/2 (Prior
//! Knowledge)`): one exchange sent with `h2` on a runtime of its own, on the run's thread
//! (nothing kept between requests), over TLS (ALPN `h2`, the environment's
//! `SSLConfiguration` applied) or plain (prior knowledge, h2c). What it answers is read as
//! ureq's answer is ([`crate::Answer`]): redirects, cookies, Digest, the body's limits and
//! its file stay the transport's. `HTTP/2` that the server doesn't take, on plain `http://`
//! or through a proxy: HTTP/1.1 (`None`).

use std::io::Read;
use std::sync::Arc;
use std::time::Instant;

use http_requests::model::Header;
use http_requests::transport::{Http2, SendOptions, TransportError};

use crate::{Answer, Exchange};

/// The connection, run beside the exchange.
type Running = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), h2::Error>> + Send>>;

/// `what` within the request's time (`deadline`).
async fn within<T>(
    deadline: Option<tokio::time::Instant>,
    what: impl std::future::Future<Output = Result<T, TransportError>>,
) -> Result<T, TransportError> {
    match deadline {
        Some(at) => tokio::time::timeout_at(at, what).await.map_err(|_| TransportError::Timeout)?,
        None => what.await,
    }
}

/// How much of a body is sent at a time.
const CHUNK: usize = 16 * 1024;

/// The headers HTTP/2 forbids (the connection's own; the host becomes `:authority`).
const CONNECTION_HEADERS: &[&str] =
    &["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "host"];

/// `exchange` sent over HTTP/2 when its options ask for it; `None`: as HTTP/1.1 (not asked,
/// not taken by the server, plain `http://` without prior knowledge, a proxy in the way).
pub(crate) fn exchange(
    exchange: &Exchange<'_>,
    headers: &[(String, String)],
    options: &SendOptions,
    started: Instant,
) -> Result<Option<Answer>, TransportError> {
    if options.http2 == Http2::Off {
        return Ok(None);
    }
    let uri: http::Uri = exchange
        .url
        .parse()
        .map_err(|e: http::uri::InvalidUri| TransportError::Url(e.to_string()))?;
    let secure = uri.scheme_str() == Some("https");
    let prior = options.http2 == Http2::PriorKnowledge;
    if (!secure && !prior) || ureq::Proxy::try_from_env().is_some() {
        return Ok(None);
    }
    let host = uri.host().ok_or_else(|| TransportError::Url(exchange.url.to_owned()))?;
    let host = host.trim_start_matches('[').trim_end_matches(']').to_owned();
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let tls = if secure { Some(tls_config(options, prior)?) } else { None };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|e| TransportError::Io(e.to_string()))?;
    let deadline = options.timeout.map(|t| tokio::time::Instant::from_std(started + t));
    let body = exchange.body.filter(|parts| !parts.is_empty());
    let answer = runtime.block_on(async {
        let connect = async {
            let stream = tokio::net::TcpStream::connect((host.as_str(), port));
            let stream = match options.connect_timeout {
                Some(t) => {
                    tokio::time::timeout(t, stream).await.map_err(|_| TransportError::Timeout)?
                }
                None => stream.await,
            };
            stream.map_err(|e| TransportError::Connect(e.to_string()))
        };
        let tcp = within(deadline, connect).await?;
        let _ = tcp.set_nodelay(true);
        let (send, connection) = match tls {
            Some(config) => {
                let name = rustls::pki_types::ServerName::try_from(host.clone())
                    .map_err(|e| TransportError::Url(e.to_string()))?;
                let connector = tokio_rustls::TlsConnector::from(config);
                let stream = within(deadline, async {
                    connector
                        .connect(name, tcp)
                        .await
                        .map_err(|e| TransportError::Tls(e.to_string()))
                })
                .await?;
                // Offered, not taken: HTTP/1.1, by the other transport.
                if stream.get_ref().1.alpn_protocol() != Some(b"h2") {
                    if prior {
                        return Err(TransportError::Connect(
                            "the server doesn't speak HTTP/2".into(),
                        ));
                    }
                    return Ok(None);
                }
                let (send, connection) = h2::client::handshake(stream).await.map_err(h2_error)?;
                (send, Box::pin(connection) as Running)
            }
            None => {
                let (send, connection) = h2::client::handshake(tcp).await.map_err(h2_error)?;
                (send, Box::pin(connection) as Running)
            }
        };
        // The connection runs beside the exchange (while the runtime is driven: by the
        // body's reads too).
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let mut send = within(deadline, async { send.ready().await.map_err(h2_error) }).await?;
        let reader = body.map(crate::body_reader).transpose()?;
        let length = reader.as_ref().and_then(|(_, length)| *length);
        let request = request(exchange, &uri, headers, length)?;
        let (response, mut stream) =
            send.send_request(request, reader.is_none()).map_err(h2_error)?;
        if let Some((mut reader, _)) = reader {
            // The stream open first (until the server's settings come it waits, and room
            // asked for meanwhile is never given): then its body as its window lets it.
            within(deadline, async { send.ready().await.map(drop).map_err(h2_error) }).await?;
            let mut chunk = vec![0u8; CHUNK];
            loop {
                let n = reader.read(&mut chunk).map_err(|e| TransportError::Io(e.to_string()))?;
                if n == 0 {
                    stream.send_data(bytes::Bytes::new(), true).map_err(h2_error)?;
                    break;
                }
                // Sent as the server's window lets it (a large file never held whole).
                let mut sent = 0;
                while sent < n {
                    stream.reserve_capacity(n - sent);
                    // Room already given (at once, by the reservation itself) is used;
                    // `poll_capacity` only tells of room given after it's asked.
                    let room = match stream.capacity() {
                        0 => {
                            within(deadline, async {
                                std::future::poll_fn(|cx| stream.poll_capacity(cx))
                                    .await
                                    .ok_or_else(|| TransportError::Io("the stream closed".into()))?
                                    .map_err(h2_error)
                            })
                            .await?
                        }
                        room => room,
                    };
                    let take = room.min(n - sent);
                    stream
                        .send_data(bytes::Bytes::copy_from_slice(&chunk[sent..sent + take]), false)
                        .map_err(h2_error)?;
                    sent += take;
                }
            }
        }
        let response = within(deadline, async { response.await.map_err(h2_error) }).await?;
        Ok(Some(response))
    })?;
    let Some(response) = answer else { return Ok(None) };
    let (parts, body) = response.into_parts();
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| Header {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    Ok(Some(Answer {
        status: parts.status.as_u16(),
        reason: parts.status.canonical_reason().unwrap_or_default().to_owned(),
        version: "HTTP/2.0".to_owned(),
        headers,
        body: Box::new(Body { runtime, body, pending: bytes::Bytes::new(), deadline }),
    }))
}

/// The request as HTTP/2 writes it: its headers lowercased, the connection's own left out.
/// Its body's length told when it's known and not written.
fn request(
    exchange: &Exchange<'_>,
    uri: &http::Uri,
    headers: &[(String, String)],
    length: Option<u64>,
) -> Result<http::Request<()>, TransportError> {
    let mut builder = http::Request::builder()
        .method(exchange.method)
        .uri(uri.clone())
        .version(http::Version::HTTP_2);
    let written = headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-length"));
    if let (Some(length), false) = (length, written) {
        builder = builder.header("content-length", length.to_string());
    }
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if CONNECTION_HEADERS.contains(&lower.as_str()) || (lower == "te" && value != "trailers") {
            continue;
        }
        builder = builder.header(lower, value.as_str());
    }
    builder.body(()).map_err(|e| TransportError::Url(e.to_string()))
}

/// The TLS of an HTTP/2 exchange: as the environment says (the server's certificate, the
/// client's), offering `h2` (and HTTP/1.1, unless it's known to speak HTTP/2).
fn tls_config(
    options: &SendOptions,
    prior: bool,
) -> Result<Arc<rustls::ClientConfig>, TransportError> {
    let mut config = crate::client_config(options)?;
    config.alpn_protocols =
        if prior { vec![b"h2".to_vec()] } else { vec![b"h2".to_vec(), b"http/1.1".to_vec()] };
    Ok(Arc::new(config))
}

fn h2_error(e: h2::Error) -> TransportError {
    if e.is_io() {
        TransportError::Connect(e.to_string())
    } else {
        TransportError::Io(e.to_string())
    }
}

/// An HTTP/2 answer's body, read as it comes: the runtime driven for each piece (the
/// connection with it), within the request's time.
struct Body {
    runtime: tokio::runtime::Runtime,
    body: h2::RecvStream,
    pending: bytes::Bytes,
    deadline: Option<tokio::time::Instant>,
}

impl Read for Body {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            let (body, deadline) = (&mut self.body, self.deadline);
            let next = self.runtime.block_on(async move {
                match deadline {
                    Some(at) => tokio::time::timeout_at(at, body.data()).await.ok(),
                    None => Some(body.data().await),
                }
            });
            match next {
                None => return Err(std::io::ErrorKind::TimedOut.into()),
                Some(None) => return Ok(0),
                Some(Some(Err(e))) => return Err(std::io::Error::other(e.to_string())),
                Some(Some(Ok(bytes))) => {
                    let _ = self.body.flow_control().release_capacity(bytes.len());
                    self.pending = bytes;
                }
            }
        }
        let n = out.len().min(self.pending.len());
        out[..n].copy_from_slice(&self.pending[..n]);
        let _ = self.pending.split_to(n);
        Ok(n)
    }
}
