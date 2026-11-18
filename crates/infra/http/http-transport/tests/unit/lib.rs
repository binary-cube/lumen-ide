//! The transport against a server of the test's own (on the loopback, no network): what it
//! sends, the redirects it follows and how, the cookies, a large body spilled, the errors.

use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc;

/// A request as the test's server got it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A server answering each request with what `answer` makes of it; what it got, in order.
fn server(answer: impl Fn(&Seen) -> Vec<u8> + Send + 'static) -> (String, mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let mut words = line.split_whitespace();
            let (method, path) =
                (words.next().unwrap_or("").to_owned(), words.next().unwrap_or("").to_owned());
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                if let Some((n, v)) = h.split_once(':') {
                    headers.push((n.trim().to_owned(), v.trim().to_owned()));
                }
            }
            let length = headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let seen = Seen { method, path, headers, body };
            let reply = answer(&seen);
            let _ = tx.send(seen);
            let mut stream = stream;
            let _ = stream.write_all(&reply);
        }
    });
    (base, rx)
}

fn reply(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
    let mut out =
        format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for h in headers {
        out.push_str(h);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

fn header(name: &str, value: &str) -> Header {
    Header { name: name.into(), value: value.into() }
}

fn get(url: &str) -> Outgoing {
    Outgoing { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Vec::new() }
}

#[test]
fn a_request_goes_as_written_and_an_error_status_is_a_response() {
    let (base, seen) = server(|s| {
        if s.path == "/missing" {
            reply("404 Not Found", &["Content-Type: text/plain"], b"nope")
        } else {
            reply(
                "201 Created",
                &["Content-Type: application/json; charset=utf-8", "X-Id: 7"],
                b"{\"ok\":true}",
            )
        }
    });
    let transport = UreqTransport::default();
    let request = Outgoing {
        method: "POST".into(),
        url: format!("{base}/users?x=1"),
        headers: vec![header("Content-Type", "application/json"), header("X-Trace", "abc")],
        body: vec![OutgoingPart::Bytes(b"{\"name\":\"ana\"}".to_vec())],
    };
    let response = transport.send(&request, &SendOptions::default(), None, &Cancel::new()).unwrap();
    let got = seen.recv().unwrap();
    assert_eq!((&*got.method, &*got.path), ("POST", "/users?x=1"));
    assert_eq!(got.header("x-trace"), Some("abc"));
    assert_eq!(got.header("content-length"), Some("14"), "its length told");
    assert_eq!(got.body, b"{\"name\":\"ana\"}");
    assert_eq!((response.status, &*response.reason), (201, "Created"));
    assert_eq!(response.version, "HTTP/1.1");
    assert_eq!(response.header("x-id"), Some("7"));
    assert_eq!(response.media_type().as_deref(), Some("application/json"));
    assert_eq!(response.body, IncomingBody::Memory(b"{\"ok\":true}".to_vec()));
    assert!(response.timings.total >= response.timings.headers);
    // An error status: a response like any other.
    let missing = transport
        .send(&get(&format!("{base}/missing")), &SendOptions::default(), None, &Cancel::new())
        .unwrap();
    assert_eq!(missing.status, 404);
}

#[test]
fn redirects_are_followed_one_by_one_as_browsers_do() {
    let (base, seen) = server(|s| match s.path.as_str() {
        "/login" => {
            reply("302 Found", &["Location: /step", "Set-Cookie: session=abc; Path=/"], b"")
        }
        "/step" => reply("307 Temporary Redirect", &["Location: /done"], b""),
        _ => reply("200 OK", &[], b"done"),
    });
    let transport = UreqTransport::default();
    let request = Outgoing {
        method: "POST".into(),
        url: format!("{base}/login"),
        headers: vec![header("Content-Type", "text/plain"), header("Authorization", "Bearer t")],
        body: vec![OutgoingPart::Bytes(b"secret".to_vec())],
    };
    let mut jar = CookieJar::default();
    let response =
        transport.send(&request, &SendOptions::default(), Some(&mut jar), &Cancel::new()).unwrap();
    let got: Vec<Seen> = seen.try_iter().collect();
    assert_eq!(
        got.iter().map(|s| (s.method.as_str(), s.path.as_str())).collect::<Vec<_>>(),
        [("POST", "/login"), ("GET", "/step"), ("GET", "/done"),],
        "302 after a POST: a GET; 307: the same method"
    );
    assert!(
        got[1].body.is_empty() && got[1].header("content-type").is_none(),
        "the body stays behind"
    );
    assert_eq!(got[1].header("cookie"), Some("session=abc"), "the cookie set on the way");
    assert_eq!(got[1].header("authorization"), Some("Bearer t"), "the same host: kept");
    assert_eq!(response.hops.len(), 2);
    assert_eq!((response.hops[0].status, response.hops[1].status), (302, 307));
    assert!(response.url.ends_with("/done"));
    assert_eq!(jar.cookies().len(), 1);
    // Not followed when asked: the redirect is the response.
    let options = SendOptions { follow_redirects: false, ..SendOptions::default() };
    let first =
        transport.send(&get(&format!("{base}/login")), &options, None, &Cancel::new()).unwrap();
    assert_eq!((first.status, first.hops.len()), (302, 0));
    // Too many.
    let options = SendOptions { max_redirects: 1, ..SendOptions::default() };
    let err = transport.send(&get(&format!("{base}/login")), &options, None, &Cancel::new());
    assert_eq!(err, Err(TransportError::TooManyRedirects));
}

#[test]
fn a_file_is_sent_as_read_and_a_large_body_goes_to_a_file() {
    let big = vec![b'x'; 300_000];
    let big_reply = big.clone();
    let (base, seen) = server(move |s| {
        if s.path == "/big" {
            reply("200 OK", &[], &big_reply)
        } else {
            reply("200 OK", &[], b"ok")
        }
    });
    let dir = std::env::temp_dir().join(format!("http-transport-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("part.txt");
    std::fs::write(&file, "from a file").unwrap();
    let transport = UreqTransport::default();
    let request = Outgoing {
        method: "PUT".into(),
        url: format!("{base}/upload"),
        headers: Vec::new(),
        body: vec![OutgoingPart::Bytes(b"head;".to_vec()), OutgoingPart::File(file)],
    };
    transport.send(&request, &SendOptions::default(), None, &Cancel::new()).unwrap();
    let got = seen.recv().unwrap();
    assert_eq!(got.body, b"head;from a file");
    let spill = dir.join("spill");
    let options =
        SendOptions { memory_limit: 100_000, spill_dir: Some(spill), ..SendOptions::default() };
    let response =
        transport.send(&get(&format!("{base}/big")), &options, None, &Cancel::new()).unwrap();
    let IncomingBody::File { path, len } = &response.body else {
        panic!("spilled: {:?}", response.body.len())
    };
    assert_eq!(*len, 300_000);
    assert_eq!(std::fs::read(path).unwrap(), big);
    let too_small = SendOptions { max_bytes: 1000, ..options };
    let err = transport.send(&get(&format!("{base}/big")), &too_small, None, &Cancel::new());
    assert_eq!(err, Err(TransportError::TooLarge));
    let missing = Outgoing { body: vec![OutgoingPart::File(dir.join("nope"))], ..request };
    assert!(matches!(
        transport.send(&missing, &SendOptions::default(), None, &Cancel::new()),
        Err(TransportError::Io(_))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_server_and_a_cancel_are_said() {
    // A port nothing listens on: refused.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let transport = UreqTransport::default();
    let err = transport.send(
        &get(&format!("http://127.0.0.1:{port}/")),
        &SendOptions::default(),
        None,
        &Cancel::new(),
    );
    assert!(matches!(err, Err(TransportError::Connect(_)) | Err(TransportError::Io(_))), "{err:?}");
    let cancel = Cancel::new();
    cancel.cancel();
    let err = transport.send(&get("http://127.0.0.1:9/"), &SendOptions::default(), None, &cancel);
    assert_eq!(err, Err(TransportError::Cancelled));
    assert!(matches!(
        transport.send(&get("not a url"), &SendOptions::default(), None, &Cancel::new()),
        Err(TransportError::Url(_))
    ));
}

#[test]
fn a_redirect_s_method_and_body() {
    assert_eq!(after_redirect(303, "PUT"), ("GET".to_owned(), false));
    assert_eq!(after_redirect(301, "POST"), ("GET".to_owned(), false));
    assert_eq!(after_redirect(301, "PUT"), ("PUT".to_owned(), true));
    assert_eq!(after_redirect(308, "POST"), ("POST".to_owned(), true));
}

#[test]
fn the_user_s_credentials_stay_with_their_host_on_a_redirect() {
    let (other, seen_there) = server(|_| reply("200 OK", &[], b"there"));
    // The same server by another name: another host.
    let other = other.replace("127.0.0.1", "localhost");
    let location = format!("Location: {other}/landed");
    let (base, _) = server(move |_| reply("302 Found", &[location.as_str()], b""));
    let request = Outgoing {
        method: "GET".into(),
        url: format!("{base}/start"),
        headers: vec![
            header("Authorization", "Bearer t"),
            header("Proxy-Authorization", "Basic p"),
            header("Cookie", "session=mine"),
            header("Accept", "text/plain"),
        ],
        body: Vec::new(),
    };
    let response = UreqTransport::default()
        .send(&request, &SendOptions::default(), None, &Cancel::new())
        .unwrap();
    assert_eq!(response.status, 200);
    let got: Vec<Seen> = seen_there.try_iter().collect();
    assert_eq!(got.len(), 1);
    for name in ["authorization", "proxy-authorization", "cookie"] {
        assert_eq!(got[0].header(name), None, "{name} left behind");
    }
    assert_eq!(got[0].header("accept"), Some("text/plain"), "the rest goes on");
}

#[test]
fn only_a_few_agents_are_kept() {
    let transport = UreqTransport::default();
    for seconds in 1..=10 {
        let options =
            SendOptions { timeout: Some(Duration::from_secs(seconds)), ..SendOptions::default() };
        let _ = transport.agent(&options);
    }
    assert_eq!(transport.agents(), AGENTS);
    let again = SendOptions { timeout: Some(Duration::from_secs(10)), ..SendOptions::default() };
    let _ = transport.agent(&again);
    assert_eq!(transport.agents(), AGENTS, "one in use: not made again");
}

#[test]
fn a_digest_challenge_is_answered_once() {
    let (base, seen) = server(|s| match s.header("authorization") {
        Some(auth) if auth.starts_with("Digest username=\"ana\"") => reply("200 OK", &[], b"in"),
        _ => reply(
            "401 Unauthorized",
            &["WWW-Authenticate: Digest realm=\"r\", nonce=\"n1\", qop=\"auth\", opaque=\"o\""],
            b"",
        ),
    });
    let request = Outgoing {
        method: "GET".into(),
        url: format!("{base}/private?x=1"),
        headers: vec![header("Authorization", "Digest ana p4ss")],
        body: Vec::new(),
    };
    let response = UreqTransport::default()
        .send(&request, &SendOptions::default(), None, &Cancel::new())
        .unwrap();
    assert_eq!(response.status, 200);
    let got: Vec<Seen> = seen.try_iter().collect();
    assert_eq!(got.len(), 2, "challenged, then answered");
    assert_eq!(got[0].header("authorization"), None, "the password never sent as written");
    let answer = got[1].header("authorization").unwrap();
    assert!(answer.contains("uri=\"/private?x=1\"") && answer.contains("qop=auth"), "{answer}");
}

#[test]
fn digest_credentials_never_answer_another_host() {
    let (other, seen_there) = server(|_| {
        reply("401 Unauthorized", &["WWW-Authenticate: Digest realm=\"r\", nonce=\"n\""], b"")
    });
    let other = other.replace("127.0.0.1", "localhost");
    let location = format!("Location: {other}/elsewhere");
    let (base, _) = server(move |_| reply("302 Found", &[location.as_str()], b""));
    let request = Outgoing {
        method: "GET".into(),
        url: format!("{base}/start"),
        headers: vec![header("Authorization", "Digest ana p4ss")],
        body: Vec::new(),
    };
    let response = UreqTransport::default()
        .send(&request, &SendOptions::default(), None, &Cancel::new())
        .unwrap();
    assert_eq!(response.status, 401, "not answered");
    let got: Vec<Seen> = seen_there.try_iter().collect();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].header("authorization"), None);
}

#[test]
fn a_websocket_session_sends_waits_and_hears() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        // Echo two messages, then close.
        for _ in 0..2 {
            if let Ok(message) = socket.read() {
                let _ = socket.send(message);
            }
        }
        let _ = socket.close(None);
        while socket.read().is_ok() {}
    });
    let messages = vec![
        http_requests::transport::WsMessage { text: "a".into(), wait_for_server: false },
        http_requests::transport::WsMessage { text: "b".into(), wait_for_server: true },
    ];
    let seen = std::sync::Mutex::new(Vec::new());
    let transcript = UreqTransport::default()
        .websocket(
            &url,
            &[],
            &messages,
            &SendOptions::default(),
            &Cancel::new(),
            &|received, text| {
                seen.lock().unwrap().push((received, text.to_owned()));
            },
        )
        .unwrap();
    let expected = vec![
        (false, "a".to_owned()),
        (true, "a".to_owned()),
        (false, "b".to_owned()),
        (true, "b".to_owned()),
    ];
    assert_eq!(transcript, expected, "`b` sent only once the server spoke");
    assert_eq!(*seen.lock().unwrap(), expected, "each told as it came");
}

/// A WSS server on the loopback with a self-signed certificate (made for these tests only:
/// `tests/unit/data/localhost.*`), echoing one message.
fn wss_server() -> String {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unit/data");
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(data.join("localhost.crt")).unwrap().flatten().collect();
    let key = PrivateKeyDer::from_pem_file(data.join("localhost.key")).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("wss://localhost:{}/ws", listener.local_addr().unwrap().port());
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        for stream in listener.incoming().take(2).flatten() {
            let Ok(tls) = rustls::ServerConnection::new(Arc::clone(&config)) else { continue };
            let stream = rustls::StreamOwned::new(tls, stream);
            let Ok(mut socket) = tungstenite::accept(stream) else { continue };
            if let Ok(message) = socket.read() {
                let _ = socket.send(message);
            }
            let _ = socket.close(None);
            while socket.read().is_ok() {}
        }
    });
    url
}

#[test]
fn a_websocket_follows_the_environment_s_tls() {
    let url = wss_server();
    let messages =
        vec![http_requests::transport::WsMessage { text: "hi".into(), wait_for_server: false }];
    let transport = UreqTransport::default();
    // Checked (the default): a self-signed certificate is refused.
    let checked = transport.websocket(
        &url,
        &[],
        &messages,
        &SendOptions::default(),
        &Cancel::new(),
        &|_, _| {},
    );
    assert!(
        matches!(checked, Err(TransportError::Tls(_) | TransportError::Connect(_))),
        "{checked:?}"
    );
    // `verifyHostCertificate: false`: taken.
    let options = SendOptions { verify_tls: false, ..SendOptions::default() };
    let transcript =
        transport.websocket(&url, &[], &messages, &options, &Cancel::new(), &|_, _| {}).unwrap();
    assert_eq!(transcript, vec![(false, "hi".to_owned()), (true, "hi".to_owned())]);
}

#[test]
fn a_websocket_s_client_certificate_is_read_as_pem() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unit/data");
    let certificate = http_requests::transport::ClientCertificate {
        chain: std::fs::read(data.join("localhost.crt")).unwrap(),
        key: std::fs::read(data.join("localhost.key")).unwrap(),
    };
    let options = SendOptions { client_certificate: Some(certificate), ..SendOptions::default() };
    assert!(ws_tls(&options).unwrap().is_some());
    let broken =
        http_requests::transport::ClientCertificate { chain: b"x".to_vec(), key: b"y".to_vec() };
    let options = SendOptions { client_certificate: Some(broken), ..SendOptions::default() };
    assert!(matches!(ws_tls(&options), Err(TransportError::Tls(_))));
    assert!(ws_tls(&SendOptions::default()).unwrap().is_none(), "the default: tungstenite's own");
}

/// An HTTP/2 server of the test's own on the loopback (TLS offering only `h2` when `tls`,
/// else h2c): each request answered with what it saw (method, path, body, cookie), and a
/// cookie set.
fn h2_server(tls: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let acceptor = tls.then(|| {
        use rustls::pki_types::pem::PemObject;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unit/data");
        let chain: Vec<CertificateDer<'static>> =
            CertificateDer::pem_file_iter(data.join("localhost.crt")).unwrap().flatten().collect();
        let key = PrivateKeyDer::from_pem_file(data.join("localhost.key")).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap();
        config.alpn_protocols = vec![b"h2".to_vec()];
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
    });
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((tcp, _)) = listener.accept().await else { continue };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor {
                        Some(acceptor) => {
                            let Ok(stream) = acceptor.accept(tcp).await else { return };
                            serve_h2(stream).await;
                        }
                        None => serve_h2(tcp).await,
                    }
                });
            }
        });
    });
    let host = if tls { "localhost" } else { "127.0.0.1" };
    format!("{}://{host}:{port}", if tls { "https" } else { "http" })
}

async fn serve_h2<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(io: T) {
    let Ok(mut connection) = ::h2::server::handshake(io).await else { return };
    // Each request answered on its own task: the connection reads its frames only while
    // it's asked for the next request.
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        tokio::spawn(async move {
            let (parts, mut body) = request.into_parts();
            let mut got = Vec::new();
            while let Some(Ok(piece)) = body.data().await {
                let _ = body.flow_control().release_capacity(piece.len());
                got.extend_from_slice(&piece);
            }
            let cookie = parts.headers.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
            let text = format!(
                "{} {} {} cookie={cookie}",
                parts.method,
                parts.uri.path(),
                String::from_utf8_lossy(&got)
            );
            let response = http::Response::builder()
                .status(200)
                .header("set-cookie", "s=1; Path=/")
                .body(())
                .unwrap();
            let Ok(mut send) = respond.send_response(response, false) else { return };
            let _ = send.send_data(bytes::Bytes::from(text), true);
        });
    }
}

fn text_of(response: &Incoming) -> String {
    match &response.body {
        IncomingBody::Memory(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        IncomingBody::File { .. } => String::new(),
    }
}

#[test]
fn http2_with_prior_knowledge_speaks_it_from_the_first_byte() {
    let base = h2_server(false);
    let request = Outgoing {
        method: "POST".into(),
        url: format!("{base}/items"),
        headers: vec![header("Cookie", "a=1"), header("Connection", "keep-alive")],
        body: vec![OutgoingPart::Bytes(b"hello".to_vec())],
    };
    let options = SendOptions {
        http2: http_requests::transport::Http2::PriorKnowledge,
        ..SendOptions::default()
    };
    let mut jar = CookieJar::default();
    let response =
        UreqTransport::default().send(&request, &options, Some(&mut jar), &Cancel::new()).unwrap();
    assert_eq!((response.status, response.version.as_str()), (200, "HTTP/2.0"));
    assert_eq!(
        text_of(&response),
        "POST /items hello cookie=a=1",
        "the connection's header left out"
    );
    // Its cookie kept, sent with the next one.
    let again = Outgoing { method: "GET".into(), body: Vec::new(), headers: Vec::new(), ..request };
    let response =
        UreqTransport::default().send(&again, &options, Some(&mut jar), &Cancel::new()).unwrap();
    assert_eq!(text_of(&response), "GET /items  cookie=s=1");
}

#[test]
fn http2_is_negotiated_over_tls_and_plain_http_stays_on_http1() {
    let base = h2_server(true);
    let request = Outgoing {
        method: "GET".into(),
        url: format!("{base}/a"),
        headers: Vec::new(),
        body: Vec::new(),
    };
    let options = SendOptions {
        http2: http_requests::transport::Http2::Negotiated,
        verify_tls: false,
        ..SendOptions::default()
    };
    let response = UreqTransport::default().send(&request, &options, None, &Cancel::new()).unwrap();
    assert_eq!((response.status, response.version.as_str()), (200, "HTTP/2.0"));
    // Plain http with `HTTP/2` (no prior knowledge): HTTP/1.1 as before.
    let (plain, _) = server(|_| reply("200 OK", &[], b"one"));
    let request = Outgoing { url: format!("{plain}/b"), ..request };
    let response = UreqTransport::default().send(&request, &options, None, &Cancel::new()).unwrap();
    assert_eq!((response.version.as_str(), text_of(&response).as_str()), ("HTTP/1.1", "one"));
}

#[test]
fn an_http2_body_larger_than_its_window_goes_whole() {
    let base = h2_server(false);
    let big = "a".repeat(300 * 1024);
    let request = Outgoing {
        method: "PUT".into(),
        url: format!("{base}/big"),
        headers: Vec::new(),
        body: vec![OutgoingPart::Bytes(big.clone().into_bytes())],
    };
    let options = SendOptions {
        http2: http_requests::transport::Http2::PriorKnowledge,
        ..SendOptions::default()
    };
    let response = UreqTransport::default().send(&request, &options, None, &Cancel::new()).unwrap();
    assert_eq!(text_of(&response), format!("PUT /big {big} cookie="));
}
