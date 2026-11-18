//! A request made ready to send: Basic credentials encoded, GraphQL as a POST.

use super::*;

fn request(method: &str, headers: &[(&str, &str)], body: &str) -> ResolvedRequest {
    ResolvedRequest {
        method: method.into(),
        url: "https://e.com/graphql".into(),
        headers: headers
            .iter()
            .map(|(n, v)| Header { name: (*n).into(), value: (*v).into() })
            .collect(),
        body: if body.is_empty() { Vec::new() } else { vec![BodyPart::Text(body.into())] },
        secret: false,
        secrets: Vec::new(),
    }
}

#[test]
fn basic_credentials_written_apart_are_encoded() {
    let sent = prepare(request("GET", &[("Authorization", "Basic ana p4ss word")], ""));
    assert_eq!(sent.headers[0].value, format!("Basic {}", crate::curl::base64(b"ana:p4ss word")));
    let encoded = prepare(request("GET", &[("Authorization", "Basic YW5hOnA=")], ""));
    assert_eq!(encoded.headers[0].value, "Basic YW5hOnA=", "already encoded: as it is");
    let bearer = prepare(request("GET", &[("Authorization", "Bearer a b")], ""));
    assert_eq!(bearer.headers[0].value, "Bearer a b");
}

#[test]
fn a_graphql_request_is_posted_as_json() {
    let body = "query User($id: ID!) {\n  user(id: $id) {\n    name\n  }\n}\n\n{\"id\": 7}\n";
    let sent = prepare(request("GRAPHQL", &[], body));
    assert_eq!(sent.method, "POST");
    assert!(sent.headers.iter().any(|h| h.name == "Content-Type" && h.value == "application/json"));
    let BodyPart::Text(json) = &sent.body[0] else { panic!("text") };
    let json: serde_json::Value = serde_json::from_str(json).unwrap();
    assert!(json["query"].as_str().unwrap().starts_with("query User"));
    assert_eq!(json["variables"]["id"], 7);
    // No variables.
    let bare = prepare(request("graphql", &[], "{ me { id } }"));
    let BodyPart::Text(json) = &bare.body[0] else { panic!("text") };
    let json: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(json["query"], "{ me { id } }");
    assert!(json.get("variables").is_none());
}

#[test]
fn a_url_without_its_scheme_goes_over_https() {
    let mut r = request("GET", &[], "");
    r.url = "example.org/users".into();
    assert_eq!(prepare(r).url, "https://example.org/users");
    let mut r = request("GET", &[("Host", "example.org:8080")], "");
    r.url = "/users?x=1".into();
    assert_eq!(prepare(r).url, "https://example.org:8080/users?x=1", "the path to its Host");
    let mut r = request("GET", &[], "");
    r.url = "https://e.com/a?next=http://x".into();
    assert_eq!(prepare(r).url, "https://e.com/a?next=http://x", "its own kept");
}

#[test]
fn what_a_url_can_t_hold_is_encoded_unless_asked_not_to() {
    let mut r = request("GET", &[], "");
    r.url = "https://e.com/a b?q=café&x=%20|".into();
    assert_eq!(prepare(r.clone()).url, "https://e.com/a%20b?q=caf%C3%A9&x=%20%7C");
    assert_eq!(prepare_with(r, false).url, "https://e.com/a b?q=café&x=%20|", "as written");
    let parsed = crate::parse("# @no-auto-encoding\nGET https://e.com/a b\n");
    assert!(parsed.requests[0].options.no_auto_encoding);
}
