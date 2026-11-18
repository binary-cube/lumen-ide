use super::*;
use syntax_ts::Grammars;

#[test]
fn queries_compile() {
    let mut g = Grammars::default();
    g.register(grammar());
    assert_eq!(g.validate(), Vec::<String>::new());
}

#[test]
fn yaml_language_server_is_described_offline() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"yaml-language-server\""));
    assert!(text.contains("schemaStore = { enable = false"), "SchemaStore's catalog never read");
    assert!(text.contains("kubernetesCRDStore = { enable = false }"));
    assert!(text.contains("on-initialized = [\"yaml/registerContentRequest\"]"));
    // Every schema it's given is one the app answers (`contents`).
    let urls: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix('"').and_then(|l| l.split('"').next()))
        .filter(|u| u.ends_with(".json"))
        .collect();
    assert!(urls.len() >= 5, "{urls:?}");
    let prefixes = [
        "https://json.schemastore.org/",
        "https://raw.githubusercontent.com/compose-spec/compose-go/master/schema/",
    ];
    for url in urls {
        assert!(prefixes.iter().any(|p| url.starts_with(p)), "{url}");
    }
}
