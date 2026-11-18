//! A `{{` in the editor: the environment's names with their values (secrets hidden), the
//! file's, the dynamic ones, a response's paths; a choice put in and closed.

use super::*;
use http_requests::env::{Environments, Source};

fn environments() -> Environments {
    Environments::read(&[
        (Source::Public, Some(r#"{"dev": {"host": "e.com", "client": {"id": "c1"}}}"#)),
        (Source::Private, Some(r#"{"dev": {"token": "t0k"}}"#)),
    ])
}

fn none(_: &str) -> crate::assist::Answer {
    None
}

#[test]
fn names_come_with_their_values_and_secrets_stay_hidden() {
    let env = environments();
    let file = "@base = https://{{host}}\n\n# @name login\nPOST {{base}}/login\n";
    let all = suggestions("", &env, Some("dev"), &FileNames::of(file), &none);
    let find = |label: &str| all.iter().find(|s| s.label == label).cloned();
    assert_eq!(find("host").unwrap().detail, "e.com");
    assert_eq!(find("client.id").unwrap().detail, "c1", "an object's member by its path");
    assert_eq!(find("token").unwrap().detail, "••••••");
    assert!(find("base").is_some(), "the file's own");
    assert!(find("login.response.body.$").is_some(), "a named request's response");
    assert!(find("$uuid").is_some());
    let narrowed = suggestions("ho", &env, Some("dev"), &FileNames::of(file), &none);
    assert_eq!(narrowed[0].label, "host");
}

#[test]
fn a_response_s_paths_are_offered_from_its_body() {
    let answer = |request: &str| -> crate::assist::Answer {
        (request == "login").then(|| {
            (
                vec!["X-Session".to_owned()],
                Some(Arc::new(serde_json::json!({"data": {"token": "t"}}))),
            )
        })
    };
    let offered = suggestions(
        "login.response.body.$.",
        &environments(),
        None,
        &FileNames::default(),
        &answer,
    );
    assert!(offered.iter().any(|s| s.insert == "login.response.body.$.data"), "{offered:?}");
}

#[test]
fn a_choice_is_put_in_and_closed() {
    let text = "https://{{ho/users";
    let (start, typed_now) = typed(text, 12).unwrap();
    assert_eq!((start, typed_now), (10, "ho"));
    assert_eq!(apply(text, start, 12, "host"), ("https://{{host}}/users".to_owned(), 16));
    let closed = "https://{{ho}}/users";
    assert_eq!(apply(closed, 10, 12, "host").0, "https://{{host}}/users", "already closed");
    assert_eq!(typed("{{a}} b", 7), None, "none open");
}

/// Only the caret's line is looked at: a `{{` left open on a line above isn't what's typed
/// (nor is a body of megabytes scanned at each key).
#[test]
fn a_brace_open_on_another_line_offers_nothing() {
    assert_eq!(typed("{{host\nplain", 12), None);
    assert_eq!(typed("a\n{{ho", 6), Some((4, "ho")));
    let file = "@token = abc\n### Login\n# @name login\nPOST https://e.com\n";
    let names = FileNames::of(file);
    let offered = suggestions("tok", &environments(), None, &names, &|_| None);
    assert!(offered.iter().any(|s| s.label == "token"), "the file's variable");
    let offered = suggestions("login", &environments(), None, &names, &|_| None);
    assert!(offered.iter().any(|s| s.label.starts_with("login.response")), "its request");
}
