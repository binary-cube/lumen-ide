//! A variable's card: where it's defined, a private value hidden.

use super::*;
use http_requests::env::Source;

#[test]
fn the_variable_under_the_mouse_is_found() {
    let text = "GET https://{{ host }}/a?x={{id}}";
    let at = text.find("host").unwrap() + 1;
    assert_eq!(variable_at(text, at), Some((at - 1..at + 3, "host")));
    assert_eq!(variable_at(text, text.find("{{id").unwrap()).map(|(_, n)| n), Some("id"));
    assert_eq!(variable_at(text, 0), None);
}

#[test]
fn its_values_by_environment_a_private_one_hidden() {
    let environments = Environments::read(&[
        (
            Source::Public,
            Some(
                r#"{"$shared": {"v": "1"}, "dev": {"host": "localhost"}, "prod": {"host": "e.com"}}"#,
            ),
        ),
        (Source::Private, Some(r#"{"dev": {"token": "t0k"}}"#)),
    ]);
    let values = |name: &str| {
        card("@host = mine\n", name, 0..1, &environments)
            .params
            .into_iter()
            .map(|p| (p.ty.unwrap_or_default(), p.name))
            .collect::<Vec<_>>()
    };
    assert_eq!(values("host").len(), 3, "the file's, dev's, prod's");
    assert_eq!(values("token"), [("dev".to_owned(), "••••••".to_owned())], "hidden");
    assert_eq!(values("v"), [("$shared".to_owned(), "1".to_owned())], "once, as $shared");
    assert!(card("", "nothing", 0..1, &environments).doc.is_some(), "defined nowhere");
    assert!(card("", "$uuid", 0..1, &environments).doc.is_some(), "a dynamic one's meaning");
}
