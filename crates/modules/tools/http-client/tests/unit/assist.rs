//! A response's parts completed from its newest response.

use super::*;

fn answer(request: &str) -> Answer {
    (request == "login").then(|| {
        (
            vec!["Location".to_owned()],
            Some(Arc::new(serde_json::json!({"data": {"items": [{"id": 7}]}, "token": "t"}))),
        )
    })
}

/// What's offered where `|` is.
fn at(text: &str) -> Option<(String, Vec<String>)> {
    let offset = text.find('|').unwrap();
    let text = text.replacen('|', "", 1);
    complete(&text, offset, &answer).map(|(range, items)| {
        (text[range].to_owned(), items.into_iter().map(|(_, insert, _)| insert).collect())
    })
}

#[test]
fn a_response_s_parts_then_its_body_s_keys() {
    let (typed, items) = at("GET {{login.response|").unwrap();
    assert_eq!(typed, "");
    assert_eq!(items, [".body.$", ".headers.", ".status"]);
    let (typed, items) = at("GET {{login.response.body.$|").unwrap();
    assert_eq!(
        (typed.as_str(), items.as_slice()),
        ("", &[".data".to_owned(), ".token".to_owned()][..])
    );
    let (typed, items) = at("GET {{login.response.body.$.data.it|").unwrap();
    assert_eq!((typed.as_str(), items.as_slice()), (".it", &[".items".to_owned()][..]));
    let (_, items) = at("GET {{login.response.body.$.data.items[|").unwrap();
    assert_eq!(items, ["[0]"]);
    let (typed, items) = at("X: {{login.response.headers.Lo|").unwrap();
    assert_eq!((typed.as_str(), items.as_slice()), ("Lo", &["Location".to_owned()][..]));
    assert_eq!(at("GET {{other.response.body.$.|"), None, "no response of it");
    assert_eq!(at("GET {{host|"), None, "not a response: the language's");
}
