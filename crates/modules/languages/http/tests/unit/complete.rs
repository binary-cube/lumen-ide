//! The place at the cursor, and what it offers.

use super::*;

/// The place where `|` is in `text` (the bar taken out), and what it replaces.
fn at(text: &str) -> Option<(Place, String)> {
    let offset = text.find('|').unwrap();
    let text = text.replacen('|', "", 1);
    place(&text, offset).map(|(p, range)| (p, text[range].to_owned()))
}

#[test]
fn each_part_of_a_request_has_its_place() {
    assert_eq!(at("PO|"), Some((Place::Method, "PO".into())));
    assert_eq!(at("### one\nG|"), Some((Place::Method, "G".into())));
    assert_eq!(at("# @name a\nGET https://e.com\nCont|"), Some((Place::HeaderName, "Cont".into())));
    assert_eq!(
        at("GET https://e.com\nContent-Type: appl|"),
        Some((Place::HeaderValue("Content-Type".into()), "appl".into()))
    );
    assert_eq!(
        at("GET https://e.com\nAccept: text/html, app|"),
        Some((Place::HeaderValue("Accept".into()), "app".into())),
        "a list's last value"
    );
    assert_eq!(at("# @no-r|"), Some((Place::Directive, "no-r".into())));
    assert_eq!(at("GET https://{{ ho|"), Some((Place::Variable, "ho".into())));
    assert_eq!(at("GET https://e.com\nX: {{|"), Some((Place::Variable, String::new())));
    // The body, a comment's text, a URL: nothing.
    assert_eq!(at("POST https://e.com\nA: b\n\n{\"x|"), None);
    assert_eq!(at("# just a no|"), None);
    assert_eq!(at("GET https://e.co|"), None);
    // A closed `{{…}}` before the cursor isn't open.
    assert_eq!(at("GET {{a}}/x\nAcc|"), Some((Place::HeaderName, "Acc".into())));
}

#[test]
fn each_place_offers_its_items() {
    let labels = |place: &Place| {
        items(place, &[("host".into(), "dev".into())])
            .into_iter()
            .map(|i| i.label)
            .collect::<Vec<_>>()
    };
    assert!(labels(&Place::Method).contains(&"PATCH".to_owned()));
    assert!(items(&Place::Method, &[]).iter().any(|i| i.insert == "POST "));
    assert!(labels(&Place::HeaderName).contains(&"Authorization".to_owned()));
    assert!(items(&Place::HeaderName, &[]).iter().any(|i| i.insert == "Content-Type: "));
    assert!(
        labels(&Place::HeaderValue("content-type".into())).contains(&"application/json".to_owned())
    );
    assert!(labels(&Place::HeaderValue("X-Mine".into())).is_empty());
    assert!(labels(&Place::Directive).contains(&"no-redirect".to_owned()));
    let variables = labels(&Place::Variable);
    assert_eq!(variables.first().map(String::as_str), Some("host"), "the given ones first");
    assert!(variables.contains(&"$uuid".to_owned()));
}
