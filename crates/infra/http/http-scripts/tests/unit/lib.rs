//! The HTTP client script API run by QuickJS: globals kept, the request's variables set, tests
//! and asserts, logs; a script that throws, never ends or reaches for what it hasn't got.

use super::*;
use http_requests::Header;

fn request() -> ScriptRequest {
    ScriptRequest {
        method: "GET".into(),
        url: "https://e.com/{{id}}".into(),
        headers: vec![Header { name: "Accept".into(), value: "application/json".into() }],
        body: String::new(),
    }
}

fn response(body: &str) -> ScriptResponse {
    ScriptResponse {
        status: 200,
        headers: vec![Header { name: "X-Session".into(), value: "s1".into() }],
        body: body.into(),
        content_type: Some("application/json".into()),
        charset: Some("utf-8".into()),
    }
}

#[test]
fn a_response_handler_keeps_globals_and_tests_the_response() {
    let mut run = ScriptRun::default();
    run.globals.insert("kept".into(), "1".into());
    let script = r#"
        client.global.set("token", response.body.token);
        client.global.set("session", response.headers.valueOf("x-session"));
        client.test("status is 200", function () {
            client.assert(response.status === 200, "not 200");
        });
        client.test("has an id", function () {
            client.assert(response.body.id !== undefined, "no id");
        });
        client.log("got", response.body.token, response.contentType.mimeType);
    "#;
    QuickJs::new().run(
        script,
        Phase::After,
        &request(),
        Some(&response(r#"{"token": "t0"}"#)),
        &mut run,
    );
    assert_eq!(run.error, None);
    assert_eq!(run.globals.get("token").map(String::as_str), Some("t0"));
    assert_eq!(run.globals.get("session").map(String::as_str), Some("s1"));
    assert_eq!(run.globals.get("kept").map(String::as_str), Some("1"), "what was there stays");
    assert_eq!(run.tests.len(), 2);
    assert_eq!(run.tests[0].failure, None);
    assert_eq!(run.tests[1].failure.as_deref(), Some("no id"));
    assert!(!run.passed());
    assert_eq!(run.logs, ["got t0 application/json"]);
}

#[test]
fn a_pre_request_script_sets_the_request_s_variables() {
    let mut run = ScriptRun::default();
    let script = r#"
        request.variables.set("id", 42);
        request.variables.set("method", request.method + " " + request.url.getRaw());
        client.log(request.headers.findByName("accept").getRawValue());
    "#;
    QuickJs::new().run(script, Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error, None);
    assert_eq!(run.variables.get("id").map(String::as_str), Some("42"));
    assert_eq!(run.variables.get("method").map(String::as_str), Some("GET https://e.com/{{id}}"));
    assert_eq!(run.logs, ["application/json"]);
}

#[test]
fn what_goes_wrong_is_said_and_stopped() {
    let mut run = ScriptRun::default();
    QuickJs::new().run("throw new Error('boom')", Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error.as_deref(), Some("boom"));
    let mut run = ScriptRun::default();
    QuickJs::new().run("this is not js", Phase::Before, &request(), None, &mut run);
    assert!(run.error.is_some());
    // Nothing of the outside to reach.
    let mut run = ScriptRun::default();
    QuickJs::new().run(
        "client.log(typeof require, typeof fetch, typeof os, typeof std)",
        Phase::Before,
        &request(),
        None,
        &mut run,
    );
    assert_eq!(run.logs, ["undefined undefined undefined undefined"]);
    // `client.exit()` ends it, quietly.
    let mut run = ScriptRun::default();
    QuickJs::new().run(
        "client.log('a'); client.exit(); client.log('b')",
        Phase::Before,
        &request(),
        None,
        &mut run,
    );
    assert_eq!((run.error, run.logs), (None, vec!["a".to_owned()]));
}

#[test]
fn a_script_that_never_ends_is_stopped() {
    let mut run = ScriptRun::default();
    let started = Instant::now();
    QuickJs::new().run("while (true) {}", Phase::Before, &request(), None, &mut run);
    assert!(run.error.as_deref().is_some_and(|e| e.contains("stopped")), "{:?}", run.error);
    assert!(started.elapsed() < LONGEST + Duration::from_secs(3));
}

#[test]
fn a_script_that_throws_keeps_what_it_logged_and_tested() {
    let mut run = ScriptRun::default();
    let script = "client.log('a'); client.test('t', () => {}); null.x;";
    QuickJs::new().run(script, Phase::Before, &request(), None, &mut run);
    assert!(run.error.is_some());
    assert_eq!(run.logs, ["a"]);
    assert_eq!(run.tests.len(), 1);
}

#[test]
fn a_promise_s_work_is_done_before_the_results_are_read() {
    let mut run = ScriptRun::default();
    let script = "Promise.resolve(2).then((n) => client.global.set('n', n));";
    QuickJs::new().run(script, Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error, None);
    assert_eq!(run.globals.get("n").map(String::as_str), Some("2"), "its `then` ran");
}

#[test]
fn a_response_s_cookies_are_read() {
    let mut response = response("{}");
    response
        .headers
        .push(Header { name: "Set-Cookie".into(), value: "sid=a=b; Path=/; HttpOnly".into() });
    let mut run = ScriptRun::default();
    let script =
        "const c = response.cookiesByName('sid')[0]; client.log(c.value, c.path, c.httponly);";
    QuickJs::new().run(script, Phase::After, &request(), Some(&response), &mut run);
    assert_eq!(run.error, None);
    assert_eq!(run.logs, ["a=b / true"]);
}

#[test]
fn crypto_hashes_and_hmacs_as_phpstorm_s() {
    let mut run = ScriptRun::default();
    let script = r#"
        client.log(crypto.sha256().updateWithText("abc").digest().toHex());
        client.log(crypto.hmac.sha256().withTextSecret("key").updateWithText("The quick brown fox jumps over the lazy dog").digest().toHex());
        client.log(crypto.md5().updateWithText("abc").digest().toBase64());
        client.log(crypto.sha1().updateWithHex("616263").digest().toHex());
        client.log(crypto.sha256().updateWithText("é").digest().toBase64(true));
    "#;
    QuickJs::new().run(script, Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error, None);
    assert_eq!(run.logs[0], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(run.logs[1], "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
    assert_eq!(run.logs[2], "kAFQmDzST7DWlj99KOF/cg==");
    assert_eq!(run.logs[3], "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert!(!run.logs[4].contains('=') && !run.logs[4].contains('+'), "URL-safe");
}

#[test]
fn a_jwt_is_signed_as_jwt_io_s_example_and_verified() {
    let mut run = ScriptRun::default();
    let script = r#"
        const token = jwt.sign({sub: "1234567890", name: "John Doe", iat: 1516239022}, "your-256-bit-secret");
        client.log(token.split(".")[2]);
        client.log(jwt.verify(token, "your-256-bit-secret").name);
        try { jwt.verify(token, "wrong"); } catch (e) { client.log(e.message); }
        client.log(jwt.decode(token).header.alg);
    "#;
    QuickJs::new().run(script, Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error, None);
    assert_eq!(
        run.logs,
        ["SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c", "John Doe", "invalid signature", "HS256"]
    );
}

#[test]
fn awaits_timers_variables_and_global_headers() {
    let mut run = ScriptRun::default();
    run.globals.insert("g".into(), "1".into());
    let script = r#"
        setTimeout(() => client.log("later"), 20);
        const t = setTimeout(() => client.log("never"), 10);
        clearTimeout(t);
        setTimeout(() => client.log("sooner"), 5);
        await sleep(10);
        client.log("after sleep");
        client.log(client.variables.get("g"), client.variables.file.get("base"), request.iteration());
        client.global.headers.set("X-Trace", "abc");
        client.global.headers.clear("X-Old");
    "#;
    let engine = QuickJs::new().with_file([("base".to_owned(), "https://e.com".to_owned())].into());
    let started = Instant::now();
    engine.run(script, Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error, None);
    assert_eq!(run.logs, ["after sleep", "1 https://e.com 0", "sooner", "later"]);
    assert!(started.elapsed() >= Duration::from_millis(10), "it waited");
    assert_eq!(run.headers.get("X-Trace"), Some(&Some("abc".to_owned())));
    assert_eq!(run.headers.get("X-Old"), Some(&None));
}

#[test]
fn a_stream_s_messages_and_lines_are_told_one_by_one() {
    let mut events = response("data: {\"n\": 1}\n\ndata: two\n\ndata: {\"n\": 3}\n\n");
    events.content_type = Some("text/event-stream".into());
    let mut run = ScriptRun::default();
    let script = r#"
        response.body.onEachMessage((m, stop) => {
            client.log(typeof m === "object" ? m.n : m);
            if (m === "two") stop();
        }, () => client.log("done"));
        response.body.onEachLine((l) => client.global.set("last", l));
    "#;
    QuickJs::new().run(script, Phase::After, &request(), Some(&events), &mut run);
    assert_eq!(run.error, None);
    assert_eq!(run.logs, ["1", "two", "done"], "stopped after `two`");
    assert_eq!(run.globals.get("last").map(String::as_str), Some("data: {\"n\": 3}"));
}

#[test]
fn jwt_signed_with_a_key_and_checked_with_its_public_one() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unit/data");
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    let script = format!(
        r#"
        const key = {key:?};
        const pub = {public:?};
        const token = jwt.sign({{ name: "Ana" }}, key, {{ algorithm: "RS256", keyid: "k1" }});
        client.log(jwt.decode(token).header.alg + " " + jwt.decode(token).header.kid);
        client.log(jwt.verify(token, pub).name);
        try {{ jwt.verify(token, pub, {{ algorithms: ["ES256"] }}); }} catch (e) {{ client.log(e.message); }}
        const hs = jwt.sign({{ a: 1 }}, "secret");
        try {{ jwt.verify(hs, pub); }} catch (e) {{ client.log(e.message); }}
        try {{ jwt.sign({{}}, "not pem", {{ algorithm: "ES256" }}); }} catch (e) {{ client.log(e.message); }}
        "#,
        key = read("rsa.key"),
        public = read("rsa.pub"),
    );
    let mut run = ScriptRun::default();
    QuickJs::new().run(&script, Phase::Before, &request(), None, &mut run);
    assert_eq!(run.error, None);
    assert_eq!(
        run.logs,
        [
            "RS256 k1",
            "Ana",
            "RS256 isn't allowed",
            "HS256 needs a shared secret, not a key",
            "ES256 needs a key in PEM",
        ]
    );
}
