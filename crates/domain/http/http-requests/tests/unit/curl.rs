//! cURL out of a request, and a pasted `curl` command into one.

use super::*;
use crate::model::Header;

#[test]
fn a_request_as_curl_quotes_what_the_shell_would_split() {
    let request = ResolvedRequest {
        method: "POST".into(),
        url: "https://example.com/a?b=1&c=2".into(),
        headers: vec![Header { name: "Content-Type".into(), value: "application/json".into() }],
        body: vec![BodyPart::Text("{\"it's\": 1}".into())],
        secret: false,
        secrets: Vec::new(),
    };
    assert_eq!(
        to_curl(&request),
        "curl -X POST \\\n  'https://example.com/a?b=1&c=2' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"it'\\''s\": 1}'"
    );
}

#[test]
fn a_browser_s_copy_as_curl_becomes_a_request() {
    let pasted = r#"curl 'https://api.example.com/users?page=2' \
  -H 'accept: application/json' \
  -H $'x-note: a\'b' \
  --data-raw '{"name":"ana"}' \
  --compressed"#;
    let text = from_curl(pasted).unwrap();
    assert_eq!(
        text,
        "POST https://api.example.com/users?page=2\naccept: application/json\nx-note: a'b\nContent-Type: application/x-www-form-urlencoded\n\n{\"name\":\"ana\"}\n"
    );
    // It reads back as a request.
    let file = crate::parse(&text);
    assert_eq!(file.requests[0].method, "POST");
}

#[test]
fn json_basic_auth_and_get_data() {
    let json = from_curl("curl --json '{\"a\":1}' https://e.com").unwrap();
    assert!(
        json.starts_with("POST https://e.com\n") && json.contains("Content-Type: application/json")
    );
    let basic = from_curl("curl -u ana:pass https://e.com").unwrap();
    assert!(basic.contains("Authorization: Basic YW5hOnBhc3M="), "{basic}");
    let get = from_curl("curl -G -d q=rust --data-urlencode x=1 https://e.com/s").unwrap();
    assert!(get.starts_with("GET https://e.com/s?q=rust&x=1\n"), "{get}");
    assert_eq!(from_curl("wget https://e.com"), None);
    let file = from_curl("curl --data-binary @payload.json https://e.com").unwrap();
    assert!(file.ends_with("\n< payload.json\n"), "the file's body, not its name: {file}");
    let raw = from_curl("curl --data-raw @literal https://e.com").unwrap();
    assert!(raw.ends_with("\n@literal\n"), "--data-raw: as written");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
}

#[test]
fn a_request_as_guzzle_code() {
    let request = ResolvedRequest {
        method: "POST".into(),
        url: "https://e.com/a".into(),
        headers: vec![crate::Header { name: "Accept".into(), value: "application/json".into() }],
        body: vec![BodyPart::Text("{\"it's\": 1}".into())],
        secret: false,
        secrets: Vec::new(),
    };
    let php = to_guzzle(&request);
    assert!(php.starts_with("$client = new \\GuzzleHttp\\Client();\n"), "{php}");
    assert!(php.contains("$client->request('POST', 'https://e.com/a', ["));
    assert!(php.contains("'Accept' => 'application/json',"));
    assert!(php.contains("'body' => '{\"it\\'s\": 1}',"), "{php}");
}
