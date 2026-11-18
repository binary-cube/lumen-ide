//! OAuth tokens had: Client Credentials from a token endpoint of the test's own, kept and
//! used again; Authorization Code by a "browser" following the sign-in's redirect; Mock.

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A token endpoint answering each form with `answer`, counting what it got.
fn token_server(answer: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            let _ = std::io::Read::read_exact(&mut reader, &mut body);
            seen.fetch_add(1, Ordering::SeqCst);
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (base, count)
}

fn config(security: serde_json::Value, id: &str) -> AuthConfig {
    oauth::config(&security, id, &|t| t.to_owned()).unwrap()
}

#[test]
fn client_credentials_are_asked_once_and_kept() {
    let (base, count) = token_server(r#"{"access_token": "a1", "expires_in": 3600}"#);
    let security = serde_json::json!({"Auth": {"m": {"Type": "OAuth2", "Grant Type": "Client Credentials",
        "Token URL": format!("{base}/token"), "Client ID": "c", "Client Secret": "s"}}});
    let transport = http_transport::UreqTransport::default();
    let cancel = Cancel::new();
    let ctx =
        Context { transport: &transport, now: 1000, open: &|_| {}, tell: &|_| {}, cancel: &cancel };
    let tokens = Tokens::default();
    let cfg = config(security, "m");
    assert_eq!(token(&tokens, Some("dev"), "m", &cfg, false, &ctx).unwrap(), "a1");
    assert_eq!(token(&tokens, Some("dev"), "m", &cfg, false, &ctx).unwrap(), "a1");
    assert_eq!(count.load(Ordering::SeqCst), 1, "kept, not asked again");
}

#[test]
fn authorization_code_follows_the_sign_in() {
    let (base, _) = token_server(r#"{"access_token": "user-token", "id_token": "idt"}"#);
    let security = serde_json::json!({"Auth": {"u": {"Type": "OAuth2", "Grant Type": "Authorization Code",
        "Auth URL": "https://auth.example.com/authorize", "Token URL": format!("{base}/token"),
        "Client ID": "c", "PKCE": true}}});
    let transport = http_transport::UreqTransport::default();
    let cancel = Cancel::new();
    // The "browser": signs in at once, sent back to the redirect with a code and the state.
    let open = |url: &str| {
        let query = url.split_once('?').unwrap().1;
        let get = |key: &str| {
            query.split('&').find_map(|p| p.strip_prefix(&format!("{key}="))).unwrap().to_owned()
        };
        let redirect = get("redirect_uri").replace("%3A", ":").replace("%2F", "/");
        let state = get("state");
        #[allow(clippy::disallowed_methods)]
        std::thread::spawn(move || {
            let ask = |query: &str| {
                let target = format!("{redirect}?{query}");
                let rest = target.strip_prefix("http://").unwrap();
                let (host, path) = rest.split_once('/').unwrap();
                let mut stream = std::net::TcpStream::connect(host).unwrap();
                let _ = write!(stream, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n");
                let mut answer = String::new();
                let _ = std::io::Read::read_to_string(&mut stream, &mut answer);
                answer
            };
            // A stray page (its icon, another's answer) first: not found, the sign-in goes on.
            assert!(ask("code=stolen").starts_with("HTTP/1.1 404"));
            assert!(ask("code=stolen&state=other").starts_with("HTTP/1.1 404"));
            assert!(ask(&format!("code=c0de&state={state}")).starts_with("HTTP/1.1 200"));
        });
    };
    let ctx =
        Context { transport: &transport, now: 0, open: &open, tell: &|_| {}, cancel: &cancel };
    let cfg = config(security, "u");
    let tokens = Tokens::default();
    assert_eq!(token(&tokens, None, "u", &cfg, false, &ctx).unwrap(), "user-token");
    assert_eq!(token(&tokens, None, "u", &cfg, true, &ctx).unwrap(), "idt", "its ID token");
}

#[test]
fn implicit_gets_its_token_from_the_fragment() {
    let security = serde_json::json!({"Auth": {"i": {"Type": "OAuth2", "Grant Type": "Implicit",
        "Auth URL": "https://auth.example.com/authorize", "Client ID": "c", "PKCE": true}}});
    let transport = http_transport::UreqTransport::default();
    let cancel = Cancel::new();
    // The "browser": sent back with the token in the fragment (which it never sends), it
    // gets our page, then (as the page's script would) asks again with it as a query.
    let open = |url: &str| {
        assert!(url.contains("response_type=token") && !url.contains("code_challenge"), "{url}");
        let query = url.split_once('?').unwrap().1;
        let get = |key: &str| {
            query.split('&').find_map(|p| p.strip_prefix(&format!("{key}="))).unwrap().to_owned()
        };
        let redirect = get("redirect_uri").replace("%3A", ":").replace("%2F", "/");
        let state = get("state");
        #[allow(clippy::disallowed_methods)]
        std::thread::spawn(move || {
            let ask = |target: &str| {
                let rest = target.strip_prefix("http://").unwrap();
                let (host, path) = rest.split_once('/').unwrap();
                let mut stream = std::net::TcpStream::connect(host).unwrap();
                let _ = write!(stream, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n");
                let mut answer = String::new();
                let _ = std::io::Read::read_to_string(&mut stream, &mut answer);
                answer
            };
            let first = ask(&redirect);
            assert!(first.contains("location.replace"), "the page bringing the fragment back");
            let stray = ask(&format!("{redirect}?access_token=stolen&state=other"));
            assert!(stray.starts_with("HTTP/1.1 404"));
            let back = ask(&format!("{redirect}?access_token=imp&expires_in=60&state={state}"));
            assert!(back.starts_with("HTTP/1.1 200"));
        });
    };
    let ctx =
        Context { transport: &transport, now: 0, open: &open, tell: &|_| {}, cancel: &cancel };
    let cfg = config(security, "i");
    assert_eq!(token(&Tokens::default(), None, "i", &cfg, false, &ctx).unwrap(), "imp");
}

#[test]
fn a_mock_s_token_is_as_written() {
    let security = serde_json::json!({"Auth": {"f": {"Type": "Mock", "Token": "t0k"}}});
    let transport = http_transport::UreqTransport::default();
    let cancel = Cancel::new();
    let ctx =
        Context { transport: &transport, now: 0, open: &|_| {}, tell: &|_| {}, cancel: &cancel };
    assert_eq!(
        token(&Tokens::default(), None, "f", &config(security, "f"), false, &ctx).unwrap(),
        "t0k"
    );
}

#[test]
fn only_web_pages_are_opened() {
    assert!(web_page("https://e.com/device") && web_page("HTTP://e.com"));
    assert!(!web_page("file:///etc/passwd") && !web_page("javascript:alert(1)") && !web_page(""));
}

#[test]
fn one_sign_in_at_a_time_per_configuration() {
    let (base, count) = token_server(r#"{"access_token": "a1", "expires_in": 3600}"#);
    let security = serde_json::json!({"Auth": {"m": {"Type": "OAuth2", "Grant Type": "Client Credentials",
        "Token URL": format!("{base}/token"), "Client ID": "c", "Client Secret": "s"}}});
    let cfg = config(security, "m");
    let tokens = Tokens::default();
    #[allow(clippy::disallowed_methods)]
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                let transport = http_transport::UreqTransport::default();
                let cancel = Cancel::new();
                let ctx = Context {
                    transport: &transport,
                    now: 1000,
                    open: &|_| {},
                    tell: &|_| {},
                    cancel: &cancel,
                };
                assert_eq!(token(&tokens, None, "m", &cfg, false, &ctx).unwrap(), "a1");
            });
        }
    });
    assert_eq!(count.load(Ordering::SeqCst), 1, "the others waited for the first's token");
}
