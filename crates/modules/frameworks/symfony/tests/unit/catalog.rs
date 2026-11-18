use super::*;

#[test]
fn a_strings_kind_is_the_code_around_it() {
    let at = |text: &str, word: &str| named_at(text, text.find(word).unwrap() + 1).map(|(k, _)| k);
    assert_eq!(at("$container->get('mailer')", "mailer"), Some(Named::Service));
    assert_eq!(at("['%kernel.debug%']", "kernel"), Some(Named::Parameter));
    assert_eq!(at("{{ 'user.hello'|trans }}", "user"), Some(Named::Translation));
    assert_eq!(at("echo 'hello'", "hello"), None);
    assert!(Named::Translation.checked("user.hello"));
    assert!(!Named::Translation.checked("Hello world"), "a sentence is its own text");
}

#[test]
fn translations_are_one_name_with_every_locales_text() {
    let t = |key: &str, locale: &str, text: &str| translations::Translation {
        key: key.into(),
        domain: "messages".into(),
        locale: locale.into(),
        text: Some(text.into()),
        file: PathBuf::from(format!("/p/translations/messages.{locale}.yaml")),
        offset: 0,
        line: 0,
    };
    let catalog =
        translations_catalog(vec![t("hi", "en", "Hi"), t("hi", "ro", "Salut"), t("x", "en", "X")]);
    let hi = catalog.get("hi").unwrap();
    assert_eq!(hi.detail.as_deref(), Some("Hi"));
    assert_eq!(hi.doc.as_deref(), Some("en (messages): Hi\nro (messages): Salut"));
    assert!(catalog.complete && catalog.get("nope").is_none());
}

/// Nothing typed yet in the quotes: still a name to complete, never a use to check.
#[test]
fn an_empty_name_is_completed_not_checked() {
    for (text, kind) in [
        ("$this->container->get('')", Named::Service),
        ("['@']", Named::Service),
        ("$this->getParameter('')", Named::Parameter),
        ("t('')", Named::Translation),
    ] {
        let at = text.find("''").or_else(|| text.find("'@")).unwrap() + 1;
        let at = if text.contains("'@'") { at + 1 } else { at };
        assert_eq!(
            named_at(text, at).map(|(k, r)| (k, r.is_empty())),
            Some((kind, true)),
            "{text}"
        );
        assert!(kind.uses(text).is_empty(), "{text}");
    }
}
