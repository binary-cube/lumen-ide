//! A request file run end to end against a server of the test's own (on the loopback): the
//! environment's variables in the request, the response kept and saved for the editor; a
//! variable missing: nothing sent.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// A server answering every request with `{"ok":true}`, telling the request lines it got.
fn server() -> (String, std::sync::mpsc::Receiver<String>) {
    server_with(|_| ("{\"ok\":true}".to_owned(), String::new()))
}

/// A server answering each request (its line and headers) with a JSON body and headers.
fn server_with(
    answer: impl Fn(&str) -> (String, String) + Send + 'static,
) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    // The test's server, on a thread of its own.
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut length = 0;
            let mut seen = line.trim().to_owned();
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                seen.push('\n');
                seen.push_str(h.trim());
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let (body, headers) = answer(&seen);
            let _ = tx.send(seen);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (base, rx)
}

fn wait(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(10), "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_request_runs_with_its_environment_and_its_response_is_kept() {
    let (base, seen) = server();
    let root = std::env::temp_dir().join(format!("http-client-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(http_requests::env::PUBLIC),
        format!(r#"{{"dev": {{"base": "{base}"}}}}"#),
    )
    .unwrap();
    let file = root.join("api.http");
    let text = "@path = users\n\n### Users\nGET {{base}}/{{path}}?id={{$randomInt}}\n\n###\nGET {{base}}/{{missing}}\n";
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    runs.choose_environment(&file, Some("dev".into()));
    assert!(job::run_at(&runs, &file, text, text.find("GET").unwrap()));
    let id = runs.runs()[0].id;
    wait("done", || !matches!(runs.run(id).unwrap().state, runs::RunState::Running));
    let got = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(got.starts_with("GET /users?id="), "{got}");
    let runs::RunState::Done(outcome) = runs.run(id).unwrap().state else { panic!("done") };
    assert_eq!(outcome.response.status, 200);
    assert_eq!(runs.runs()[0].label, "Users");
    let saved = outcome.saved.clone().expect("saved for the editor");
    assert!(saved.starts_with(root.join(kernel::brand::PROJECT_DIR).join("http/responses")));
    assert_eq!(saved.extension().and_then(|e| e.to_str()), Some("json"));
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "{\"ok\":true}");
    // A variable nothing has: said, nothing sent.
    let second = text.rfind("GET").unwrap();
    assert!(job::run_at(&runs, &file, text, second));
    let id = runs.runs().last().unwrap().id;
    wait("failed", || !matches!(runs.run(id).unwrap().state, runs::RunState::Running));
    let runs::RunState::Failed(why) = runs.run(id).unwrap().state else { panic!("failed") };
    assert!(why.contains("missing"), "{why}");
    assert!(seen.recv_timeout(Duration::from_millis(200)).is_err(), "nothing sent");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_request_files_get_the_bar() {
    assert!(is_request_file(std::path::Path::new("a.http")));
    assert!(is_request_file(std::path::Path::new("a.rest")));
    assert!(!is_request_file(std::path::Path::new("a.json")));
}

#[test]
fn a_request_using_another_s_response_runs_it_first_and_takes_the_value() {
    let (base, seen) = server_with(|request| {
        if request.starts_with("POST /login") {
            ("{\"data\": {\"token\": \"t0k\"}}".to_owned(), "X-Session: s1\r\n".to_owned())
        } else {
            ("{}".to_owned(), String::new())
        }
    });
    let root = std::env::temp_dir().join(format!("http-chain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("chain.http");
    let text = format!(
        "# @name login\nPOST {base}/login\n\n###\nGET {base}/me\nAuthorization: Bearer {{{{login.response.body.$.data.token}}}}\nX-S: {{{{login.response.headers.X-Session}}}}\n"
    );
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    assert!(job::run_at(&runs, &file, &text, text.find("GET").unwrap()));
    assert_eq!(runs.runs().len(), 2, "login first, then itself");
    wait("both done", || runs.runs().iter().all(|r| !matches!(r.state, runs::RunState::Running)));
    let first = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(first.starts_with("POST /login"), "{first}");
    // (Header names go lowercase on the wire.)
    let second = second.to_ascii_lowercase();
    assert!(second.contains("authorization: bearer t0k"), "{second}");
    assert!(second.contains("x-s: s1"), "{second}");
    // Run again: login's response is kept, it isn't sent again.
    assert!(job::run_at(&runs, &file, &text, text.find("GET").unwrap()));
    assert_eq!(runs.runs().len(), 3);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scripts_keep_a_token_test_the_response_and_set_a_request_s_variables() {
    let (base, seen) = server_with(|request| {
        if request.starts_with("POST /login") {
            ("{\"token\": \"t0k\"}".to_owned(), String::new())
        } else {
            ("{}".to_owned(), String::new())
        }
    });
    let root = std::env::temp_dir().join(format!("http-scripts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("scripts.http");
    let text = format!(
        "POST {base}/login\n\n> {{%\n  client.global.set(\"auth\", response.body.token);\n  client.test(\"logged in\", () => client.assert(response.status === 200, \"no\"));\n  client.log(\"token\", response.body.token);\n%}}\n\n###\n< {{%\n  request.variables.set(\"page\", 2);\n%}}\nGET {base}/items?page={{{{page}}}}\nAuthorization: Bearer {{{{auth}}}}\n"
    );
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    let done =
        |runs: &runs::Runs| runs.runs().iter().all(|r| !matches!(r.state, runs::RunState::Running));
    assert!(job::run_at(&runs, &file, &text, text.find("POST").unwrap()));
    wait("logged in", || done(&runs));
    let runs::RunState::Done(outcome) = runs.runs()[0].state.clone() else {
        panic!("done: {:?}", runs.runs()[0].state)
    };
    let script = outcome.script.as_ref().expect("its script ran");
    assert!(script.passed(), "{script:?}");
    assert_eq!(script.logs, ["token t0k"]);
    assert_eq!(runs.globals().get("auth").map(String::as_str), Some("t0k"));
    let _ = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    // The next request: its own variable, the kept token.
    assert!(job::run_at(&runs, &file, &text, text.rfind("GET").unwrap()));
    wait("listed", || done(&runs));
    let sent = seen.recv_timeout(Duration::from_secs(5)).unwrap().to_ascii_lowercase();
    assert!(sent.starts_with("get /items?page=2"), "{sent}");
    assert!(sent.contains("authorization: bearer t0k"), "{sent}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_response_is_written_where_the_request_says() {
    let (base, _seen) = server();
    let root = std::env::temp_dir().join(format!("http-save-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("save.http");
    let text = format!(
        "GET {base}/a\n\n>> ./out/{{{{name}}}}.json\n\n###\nGET {base}/b\n\n>>! ./out/fixed.json\n"
    );
    let text = format!("@name = got\n\n{text}");
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    let done =
        |runs: &runs::Runs| runs.runs().iter().all(|r| !matches!(r.state, runs::RunState::Running));
    for _ in 0..2 {
        assert!(job::run_at(&runs, &file, &text, text.find("GET").unwrap()));
        wait("saved", || done(&runs));
        assert!(job::run_at(&runs, &file, &text, text.rfind("GET").unwrap()));
        wait("fixed", || done(&runs));
    }
    assert_eq!(std::fs::read_to_string(root.join("out/got.json")).unwrap(), "{\"ok\":true}");
    assert!(root.join("out/got-1.json").is_file(), ">> beside what's there");
    assert!(
        root.join("out/fixed.json").is_file() && !root.join("out/fixed-1.json").exists(),
        ">>! over it"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_collection_sends_one_request_per_element_and_global_headers_follow() {
    let (base, seen) = server();
    let root = std::env::temp_dir().join(format!("http-iterate-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(http_requests::env::PUBLIC), r#"{"dev": {"ids": [1, 2, 3]}}"#)
        .unwrap();
    let file = root.join("each.http");
    let text = format!(
        "< {{%\n  client.global.headers.set(\"X-Trace\", \"t\");\n%}}\nGET {base}/items/{{{{ids}}}}\n\n> {{%\n  client.log(request.iteration());\n%}}\n"
    );
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    runs.choose_environment(&file, Some("dev".into()));
    assert!(job::run_at(&runs, &file, &text, text.find("GET").unwrap()));
    wait("all three", || {
        runs.runs().len() == 3
            && runs.runs().iter().all(|r| !matches!(r.state, runs::RunState::Running))
    });
    let got: Vec<String> =
        (0..3).map(|_| seen.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
    assert!(got[0].starts_with("GET /items/1") && got[2].starts_with("GET /items/3"), "{got:?}");
    assert!(got.iter().all(|r| r.to_ascii_lowercase().contains("x-trace: t")), "the global header");
    let logs: Vec<String> = runs
        .runs()
        .iter()
        .filter_map(|r| match &r.state {
            runs::RunState::Done(o) => o.script.as_ref().map(|s| s.logs.join("")),
            _ => None,
        })
        .collect();
    assert_eq!(logs, ["0", "1", "2"]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_xml_response_is_read_by_xpath_in_the_next_request() {
    let (base, seen) = server_with(|request| {
        if request.starts_with("POST /login") {
            ("<login><token>x1</token></login>".to_owned(), String::new())
        } else {
            ("{}".to_owned(), String::new())
        }
    });
    let root = std::env::temp_dir().join(format!("http-xpath-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("xml.http");
    let text = format!(
        "# @name login\nPOST {base}/login\n\n###\nGET {base}/me?t={{{{login.response.body.//token}}}}\n"
    );
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    assert!(job::run_at(&runs, &file, &text, text.find("GET").unwrap()));
    wait("both", || {
        runs.runs().len() == 2
            && runs.runs().iter().all(|r| !matches!(r.state, runs::RunState::Running))
    });
    let _ = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(second.starts_with("GET /me?t=x1"), "{second}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_oauth_token_is_had_and_sent_as_the_request_says() {
    let (base, seen) = server_with(|request| {
        if request.starts_with("POST /token") {
            (r#"{"access_token": "tok-1", "expires_in": 3600}"#.to_owned(), String::new())
        } else {
            ("{}".to_owned(), String::new())
        }
    });
    let root = std::env::temp_dir().join(format!("http-oauth-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(http_requests::env::PUBLIC),
        format!(r#"{{"dev": {{"Security": {{"Auth": {{"m": {{"Type": "OAuth2", "Grant Type": "Client Credentials", "Token URL": "{base}/token", "Client ID": "c", "Client Secret": "s"}}}}}}}}}}"#),
    )
    .unwrap();
    let file = root.join("oauth.http");
    let text = format!("GET {base}/me\nAuthorization: Bearer {{{{$auth.token(\"m\")}}}}\n");
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    runs.choose_environment(&file, Some("dev".into()));
    assert!(job::run_at(&runs, &file, &text, 0));
    wait("done", || runs.runs().iter().all(|r| !matches!(r.state, runs::RunState::Running)));
    let token_ask = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(token_ask.starts_with("POST /token"), "{token_ask}");
    let sent = seen.recv_timeout(Duration::from_secs(5)).unwrap().to_ascii_lowercase();
    assert!(sent.contains("authorization: bearer tok-1"), "{sent}");
    // Hidden where it's shown: a secret.
    let runs::RunState::Done(outcome) = runs.runs()[0].state.clone() else { panic!("done") };
    assert!(outcome.request.secret);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_stream_is_seen_while_it_comes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    #[allow(clippy::disallowed_methods)]
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                break;
            }
        }
        let mut stream = stream;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        );
        let _ = write!(stream, "data: one\n\n");
        let _ = stream.flush();
        std::thread::sleep(Duration::from_millis(300));
        let _ = write!(stream, "data: two\n\n");
    });
    let root = std::env::temp_dir().join(format!("http-stream-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("sse.http");
    let text = format!("GET {base}/events\nAccept: text/event-stream\n");
    let runs = runs::Runs::of(Some(kernel::ProjectId::new()), Some(&root));
    assert!(job::run_at(&runs, &file, &text, 0));
    let id = runs.runs()[0].id;
    wait("the first event while it goes", || runs.live(id).is_some_and(|l| l.contains("one")));
    assert!(matches!(runs.run(id).unwrap().state, runs::RunState::Running), "still coming");
    wait("done", || !matches!(runs.run(id).unwrap().state, runs::RunState::Running));
    assert_eq!(runs.live(id), None, "let go once it ended");
    let _ = std::fs::remove_dir_all(root);
}
