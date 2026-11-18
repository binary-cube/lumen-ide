use super::*;

#[test]
fn reads_xliff_units() {
    let xlf = "<file>\n<trans-unit id=\"a1\" resname=\"user.hello\">\n  <source>user.hello</source>\n  <target>Bonjour</target>\n</trans-unit>\n<trans-unit id=\"a2\">\n  <source>Bye</source>\n</trans-unit>\n</file>\n";
    let units = from_xliff(xlf);
    let found: Vec<(&str, Option<&str>)> =
        units.iter().map(|(k, t, _)| (k.as_str(), t.as_deref())).collect();
    assert_eq!(found, [("user.hello", Some("Bonjour")), ("Bye", None)]);
    assert_eq!(&xlf[units[0].2..units[0].2 + 10], "user.hello");
}

#[test]
fn finds_the_keys_named() {
    let twig = "{{ 'user.hello'|trans }} {{ 'plain' }} {{ \"title\" | trans({}, 'admin') }}";
    let named: Vec<&str> = uses(twig).into_iter().map(|r| &twig[r]).collect();
    assert_eq!(named, ["user.hello", "title"]);
    let php = "$translator->trans('a.b'); t('c.d'); $this->set('no'); trans('e');";
    let named: Vec<&str> = uses(php).into_iter().map(|r| &php[r]).collect();
    assert_eq!(named, ["a.b", "c.d", "e"]);
    assert_eq!(named_at(php, php.find("a.b").unwrap() + 1).map(|r| &php[r]), Some("a.b"));
}
