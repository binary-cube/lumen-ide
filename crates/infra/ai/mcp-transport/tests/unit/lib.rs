use super::*;
use serde_json::json;

/// A request's status and body, sent raw to the server at `port`.
fn send(port: u16, raw: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    let mut reader = BufReader::new(&stream);
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let code = status.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, String::from_utf8(body).unwrap())
}

fn post(port: u16, headers: &str, body: &str) -> (u16, String) {
    send(
        port,
        &format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// An echoing server: a request's `id` and `method` back; a notification, nothing.
fn server() -> Server {
    let handler: Handler = Arc::new(|m: &Value| {
        let id = m.get("id")?;
        Some(json!({"id": id, "result": m["method"]}))
    });
    Server::start(0, "secret-1".into(), handler).unwrap()
}

/// With its token, a request is answered as JSON; a notification gets `202`; a batch, its
/// answers in a list.
#[test]
fn a_client_with_the_token_is_answered() {
    let server = server();
    let auth = "Authorization: Bearer secret-1\r\n";
    let (status, body) = post(server.port(), auth, r#"{"id":1,"method":"ping"}"#);
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"id": 1, "result": "ping"}));
    assert_eq!(post(server.port(), auth, r#"{"method":"notifications/initialized"}"#).0, 202);
    let (status, body) = post(
        server.port(),
        auth,
        r#"[{"id":1,"method":"a"},{"method":"n"},{"id":2,"method":"b"}]"#,
    );
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap().as_array().unwrap().len(), 2);
    assert!(server.url().ends_with("/mcp") && server.url().starts_with("http://127.0.0.1:"));
}

/// Refused: no token or another; a page of another site (its Origin, or a Host not ours);
/// another path; anything but POST; a body too big; not JSON.
#[test]
fn what_isn_t_a_local_client_with_the_token_is_refused() {
    let server = server();
    let port = server.port();
    let auth = "Authorization: Bearer secret-1\r\n";
    assert_eq!(post(port, "", "{}").0, 401);
    assert_eq!(post(port, "Authorization: Bearer secret-2\r\n", "{}").0, 401);
    assert_eq!(post(port, &format!("{auth}Origin: https://evil.example\r\n"), "{}").0, 403);
    assert_eq!(
        post(port, &format!("{auth}Origin: http://localhost:3000\r\n"), r#"{"id":1,"method":"x"}"#)
            .0,
        200
    );
    let rebound = format!(
        "POST /mcp HTTP/1.1\r\nHost: evil.example:{port}\r\n{auth}Content-Length: 2\r\n\r\n{{}}"
    );
    assert_eq!(send(port, &rebound).0, 403);
    let other = format!(
        "POST /other HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Length: 2\r\n\r\n{{}}"
    );
    assert_eq!(send(port, &other).0, 404);
    let get = format!("GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}\r\n");
    assert_eq!(send(port, &get).0, 405);
    let big = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Length: 999999999\r\n\r\n"
    );
    assert_eq!(send(port, &big).0, 413);
    assert_eq!(post(port, auth, "not json").0, 400);
}

/// Requests one after another on one connection (kept alive), then closed.
#[test]
fn a_connection_serves_several_requests() {
    let server = server();
    let port = server.port();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    for id in 1..=2 {
        let body = format!(r#"{{"id":{id},"method":"m"}}"#);
        let raw = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: Bearer secret-1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(raw.as_bytes()).unwrap();
        // Read until this answer's body is in (it may come in pieces).
        let mut got = Vec::new();
        let mut buf = [0u8; 512];
        while !String::from_utf8_lossy(&got).contains(&format!("\"id\":{id}")) {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "closed early");
            got.extend_from_slice(&buf[..n]);
        }
        let text = String::from_utf8_lossy(&got);
        assert!(
            text.starts_with("HTTP/1.1 200") && text.contains(&format!("\"id\":{id}")),
            "{text}"
        );
    }
}

#[test]
fn the_token_is_compared_whole() {
    assert!(same(b"abc", b"abc"));
    assert!(!same(b"abc", b"abd") && !same(b"abc", b"ab"));
    assert!(local_origin(None) && local_origin(Some("http://[::1]:5")));
    assert!(!local_origin(Some("http://127.0.0.1.evil.example")));
}

/// A client without the token is refused by its headers: its body (announced, never sent)
/// isn't waited for nor held.
#[test]
fn a_client_without_the_token_is_refused_before_its_body() {
    let server = server();
    let port = server.port();
    let raw =
        format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 4000000\r\n\r\n");
    let started = std::time::Instant::now();
    let (status, _) = send(port, &raw);
    assert_eq!(status, 401);
    assert!(started.elapsed() < Duration::from_secs(2), "answered without the body");
}
