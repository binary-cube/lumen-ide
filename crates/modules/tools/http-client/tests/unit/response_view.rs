//! A response's view: running, then its body by lines; its menu's copies.

use super::*;
use http_requests::transport::{Incoming, IncomingBody, Timings};
use http_requests::vars::ResolvedRequest;

fn outcome(body: &[u8]) -> Arc<Outcome> {
    Arc::new(Outcome {
        request: ResolvedRequest {
            method: "GET".into(),
            url: "https://e.com/a".into(),
            headers: Vec::new(),
            body: Vec::new(),
            secret: false,
            secrets: Vec::new(),
        },
        response: Incoming {
            status: 200,
            reason: "OK".into(),
            version: "HTTP/1.1".into(),
            headers: vec![http_requests::Header {
                name: "Content-Type".into(),
                value: "application/json".into(),
            }],
            body: IncomingBody::Memory(body.to_vec()),
            url: "https://e.com/a".into(),
            hops: Vec::new(),
            timings: Timings::default(),
        },
        saved: None,
        kept: None,
        json: None,
        script: None,
    })
}

/// Polled until `ready` (what's made on a worker arrives by a poll).
fn wait(view: &mut ResponseView, ready: impl Fn(&ResponseView) -> bool) {
    let started = std::time::Instant::now();
    while !ready(view) && started.elapsed() < std::time::Duration::from_secs(10) {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn lines_are_one_text_and_a_long_one_is_cut_when_drawn() {
    let lines = Lines::new("a\r\nbb\n\nc".to_owned());
    assert_eq!(lines.len(), 4);
    assert_eq!((lines.get(0), lines.get(1), lines.get(2), lines.get(3)), ("a", "bb", "", "c"));
    assert_eq!(Lines::new(String::new()).len(), 0);
    assert_eq!(Lines::new("x\n".to_owned()).len(), 1, "as `str::lines`: no empty last one");
    let long = Lines::new("é".repeat(LINE_SHOWN + 10));
    assert_eq!(long.shown(0).chars().count(), LINE_SHOWN);
}

#[test]
fn it_follows_its_run_and_shows_the_body_readable() {
    let runs = Runs::of(Some(kernel::ProjectId::new()), None);
    let id = runs.begin(crate::runs::Run {
        id: kernel::id::new_uuid(),
        number: 0,
        file: "/p/a.http".into(),
        label: "a".into(),
        method: "GET".into(),
        line: 0..5,
        line_number: 0,
        hash: 0,
        state: RunState::Running,
        started: std::time::Instant::now(),
        cancel: kernel::work::Cancel::new(),
    });
    let mut view = ResponseView::new(Arc::clone(&runs), id, Part::Body);
    assert!(matches!(view.shown, Shown::Running));
    assert!(!view.poll(), "nothing new");
    runs.end(id, RunState::Done(outcome(br#"{"a":1}"#)));
    assert!(view.poll(), "the run ended");
    let shown = |view: &ResponseView| match &view.shown {
        Shown::Done { body: Some(body), .. } => Some(body.lines.text.clone()),
        _ => None,
    };
    assert_eq!(shown(&view), None, "made readable on a worker, not here");
    wait(&mut view, |v| shown(v).is_some());
    assert_eq!(shown(&view).as_deref(), Some("{\n  \"a\": 1\n}"));
    let menu = view.context_menu(Point::new(0.0, 0.0)).unwrap();
    assert_eq!(menu.len(), 3, "nothing saved: no Open in Editor (Copy Body, cURL, Guzzle)");
    // The body as it came, not as shown.
    assert!(view.command("copy-body"));
    wait(&mut view, |v| v.action.is_some());
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(r#"{"a":1}"#.into())))
    );
    assert!(view.command("copy-curl"));
    let Some(ViewerAction::Request(AppRequest::CopyText(curl))) = view.take_action() else {
        panic!("copied")
    };
    assert!(curl.starts_with("curl -X GET"));
}

#[test]
fn headers_and_raw_hide_what_carries_a_secret() {
    let mut outcome = Arc::into_inner(outcome(b"{}")).unwrap();
    outcome.request.secret = true;
    outcome.request.secrets = vec!["t0k".into(), "k3y-v4lue".into()];
    outcome.request.url = "https://e.com/a?key=k3y-v4lue".into();
    outcome.request.headers = vec![
        http_requests::Header { name: "Authorization".into(), value: "Bearer t0k".into() },
        http_requests::Header { name: "X-Auth-Token".into(), value: "k3y-v4lue".into() },
        http_requests::Header { name: "Accept".into(), value: "*/*".into() },
    ];
    outcome.request.body = vec![http_requests::BodyPart::Text("{\"key\": \"k3y-v4lue\"}".into())];
    let (headers, _) = part_lines(Part::Headers, &outcome);
    assert!(headers.contains(&"Authorization: ••••••".to_owned()), "{headers:?}");
    assert!(headers.contains(&"Accept: */*".to_owned()));
    assert!(headers.iter().any(|l| l.contains("200 OK")));
    let (raw, extension) = part_lines(Part::Raw, &outcome);
    assert!(raw[0].starts_with("GET https://e.com/a"));
    assert!(
        !raw.iter().chain(&headers).any(|l| l.contains("t0k") || l.contains("k3y-v4lue")),
        "a secret never shown: in the URL, any header, the body"
    );
    assert!(headers.contains(&"X-Auth-Token: ••••••".to_owned()), "{headers:?}");
    assert_eq!(extension, "http");
    let (timeline, _) = part_lines(Part::Timeline, &outcome);
    assert_eq!(timeline.len(), 3);
}

#[test]
fn a_page_opens_in_the_browser_with_its_base() {
    let mut html = Arc::into_inner(outcome(b"<html><head></head><body>hi</body></html>")).unwrap();
    html.response.headers[0].value = "text/html; charset=utf-8".into();
    let runs = Runs::of(Some(kernel::ProjectId::new()), None);
    let id = runs.begin(crate::runs::Run {
        id: kernel::id::new_uuid(),
        number: 0,
        file: "/p/a.http".into(),
        label: "a".into(),
        method: "GET".into(),
        line: 0..5,
        line_number: 0,
        hash: 0,
        state: RunState::Running,
        started: std::time::Instant::now(),
        cancel: kernel::work::Cancel::new(),
    });
    runs.end(id, RunState::Done(Arc::new(html)));
    let mut view = ResponseView::new(Arc::clone(&runs), id, Part::Body);
    let menu = view.context_menu(Point::new(0.0, 0.0)).unwrap();
    let offered =
        menu.iter().any(|e| matches!(e, ViewMenuEntry::Item { id, .. } if id == "open-in-browser"));
    assert!(offered, "a page: offered");
    assert!(view.command("open-in-browser"));
    wait(&mut view, |v| v.action.is_some());
    let Some(ViewerAction::Request(AppRequest::OpenInBrowser(path))) = view.take_action() else {
        panic!("opened")
    };
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.starts_with("<html><head><base href=\"https://e.com/a\">"), "{written}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_tests_tab_says_each_test_and_the_log() {
    let mut outcome = Arc::into_inner(outcome(b"{}")).unwrap();
    outcome.script = Some(http_requests::script::ScriptRun {
        tests: vec![
            http_requests::script::TestResult { name: "ok".into(), failure: None },
            http_requests::script::TestResult { name: "id".into(), failure: Some("no id".into()) },
        ],
        logs: vec!["a\nb".into()],
        ..Default::default()
    });
    let (lines, _) = part_lines(Part::Tests, &outcome);
    assert_eq!(lines[0], "✓ ok");
    assert_eq!(lines[1], "✗ id — no id");
    assert_eq!(&lines[lines.len() - 2..], ["a", "b"], "a log's lines each its own");
}

/// Wrapped, each line's first row: a short line one row, a long one the rows it needs.
#[test]
fn a_body_s_long_lines_take_their_rows() {
    let long = "\"key\": \"value\", ".repeat(20);
    let lines = Lines::new(format!("{{\n{long}\n}}"));
    let firsts = first_rows(&lines, 40);
    assert_eq!(firsts.len(), 4);
    assert_eq!(firsts[1], 1);
    assert!(firsts[2] - firsts[1] > 1, "the long line takes rows: {firsts:?}");
    assert_eq!(firsts[3], firsts[2] + 1);
}
