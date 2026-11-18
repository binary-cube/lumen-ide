use super::*;
use syntax_ts::Grammars;

#[test]
fn queries_compile() {
    let mut g = Grammars::default();
    g.register(grammar());
    assert_eq!(g.validate(), Vec::<String>::new());
}

#[test]
fn sniffer_accepts_json_and_rejects_lookalikes() {
    let s = JsonSniffer;
    assert_eq!(s.sniff(r#"{"a": [1, 2.5e3, true, null], "b": "x\"y"}"#), Some(Confidence(90)));
    assert_eq!(s.sniff("  [1,2]  "), Some(Confidence(90)));
    assert_eq!(s.sniff("{}"), Some(Confidence(50)));
    assert_eq!(s.sniff("{ $a = 1; }"), None);
    assert_eq!(s.sniff("[1, 2"), None);
    assert_eq!(s.sniff("{\"a\": 1} trailing"), None);
    assert_eq!(s.sniff("hello"), None);
    assert_eq!(s.sniff("[toast]"), None);
}

/// `composer.json` and `package.json` are served with their schemas (kept beside the server:
/// nothing is downloaded).
#[test]
fn the_project_manifests_have_their_schemas() {
    let data = include_str!("../../data/language-servers.toml");
    for (file, schema) in [
        ("composer.json", "https://getcomposer.org/schema.json"),
        ("package.json", "https://json.schemastore.org/package.json"),
    ] {
        let line = data.lines().find(|l| l.contains(file) && l.contains("url =")).unwrap();
        assert!(line.contains(schema), "{file}");
    }
}
