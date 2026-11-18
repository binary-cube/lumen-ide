use super::*;
use std::path::PathBuf;

fn doc(name: &str, text: &str) -> EnvDoc {
    EnvDoc::new(PathBuf::from("/p").join(name), Arc::from(text))
}

fn docs() -> Vec<EnvDoc> {
    vec![
        doc(".env", "APP_ENV=dev\nAPP_SECRET=abc123\n"),
        doc(".env.local", "APP_ENV=prod\n"),
        doc(".env.example", "APP_ENV=\nAPP_SECRET=\n"),
    ]
}

#[test]
fn every_file_defining_a_key_in_order_but_the_cursors() {
    let docs = docs();
    let places = definitions(&docs, "APP_ENV", None, Some(Path::new("/p")));
    let rows: Vec<(&str, &str, u32)> =
        places.iter().map(|p| (p.label.as_str(), p.detail.as_str(), p.at.line)).collect();
    assert_eq!(rows[0], (".env", "= dev", 0));
    assert_eq!(rows[1], (".env.local", "= prod", 0));
    assert!(rows[2].1.starts_with("= "), "{:?}", rows[2]);
    assert_eq!(places[1].at.offset, 0);
    // On `.env`'s own definition: the others.
    let here = (Path::new("/p/.env"), 0);
    assert_eq!(definitions(&docs, "APP_ENV", Some(here), None).len(), 2);
    assert!(definitions(&docs, "NOPE", None, None).is_empty());
}

#[test]
fn a_secrets_value_is_hidden_in_the_places_and_the_card() {
    let docs = docs();
    let places = definitions(&docs, "APP_SECRET", None, None);
    assert_eq!(places[0].detail, "= ••••••");
    let card = hover(&docs, "APP_SECRET", 3..13, Some(Path::new("/p")));
    let rows: Vec<(Option<&str>, &str)> =
        card.params.iter().map(|p| (p.ty.as_deref(), p.name.as_str())).collect();
    assert_eq!(rows[0], (Some(".env"), "••••••"));
    assert_eq!(rows[1].0, Some(".env.example"));
    assert!(card.params[1].doc.is_some(), "the documentation marked");
    assert_eq!((card.code.as_str(), card.range.clone()), ("APP_SECRET", 3..13));
    assert!(card.doc.is_none());
}

#[test]
fn a_key_no_file_defines() {
    let card = hover(&docs(), "MISSING", 0..7, None);
    assert!(card.params.is_empty() && card.doc.is_some());
}

#[test]
fn the_keys_are_suggested_each_once_a_secret_hidden() {
    let docs =
        vec![doc(".env", "APP_NAME=shop\nAPI_SECRET=abcdef\n"), doc(".env.local", "APP_NAME=x\n")];
    let items = suggestions(&docs, "AP");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels.iter().filter(|l| **l == "APP_NAME").count(), 1);
    assert!(labels.contains(&"API_SECRET"));
    let secret = items.iter().find(|i| i.label == "API_SECRET").unwrap();
    assert!(!secret.detail.as_deref().unwrap_or_default().contains("abcdef"));
}
