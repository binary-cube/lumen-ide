//! The `.http` language: found by its extensions, its queries valid, its parts colored.

use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, parse_document};

const FILE: &str = "\
@host = api.example.com

### Log in
# @name login
POST https://{{host}}/login HTTP/1.1
Content-Type: application/json

{\"user\": \"ana\"}

> {% client.global.set(\"token\", response.body.token); %}

###
GET https://{{host}}/users
Authorization: Bearer {{token}}
";

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    SyntaxConfig::new(grammars)
}

/// The scope drawn at the start of `needle`'s `nth` occurrence (the last span applied wins).
fn scope_at(config: &SyntaxConfig, text: &str, needle: &str, nth: usize) -> Option<String> {
    let doc = Document::new(text);
    let snapshot = parse_document(config, &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0)?;
    let mut spans = Vec::new();
    Highlighter::new().highlight(&snapshot, &doc.buffer().snapshot(), 0..text.len(), &mut spans);
    let at = text.match_indices(needle).nth(nth).unwrap_or_else(|| panic!("{needle}")).0;
    spans
        .iter()
        .rev()
        .find(|sp| sp.range.contains(&at))
        .and_then(|sp| config.scope(sp.scope))
        .map(|(_, name)| name)
}

#[test]
fn queries_compile() {
    let mut g = Grammars::default();
    g.register(grammar());
    assert_eq!(g.validate(), Vec::<String>::new());
}

#[test]
fn request_files_are_found_by_their_extensions() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["api.http", "users.rest"] {
        assert_eq!(registry.detect(Path::new(name), ""), LANGUAGE, "{name}");
    }
}

#[test]
fn a_request_s_parts_are_colored() {
    let config = config();
    let scope = |needle, nth| scope_at(&config, FILE, needle, nth);
    assert_eq!(scope("POST", 0).as_deref(), Some("keyword"), "the method");
    assert_eq!(scope("https://", 0).as_deref(), Some("function"), "the URL");
    assert_eq!(scope("Content-Type", 0).as_deref(), Some("property"), "a header's name");
    assert_eq!(scope("application/json", 0).as_deref(), Some("string"), "its value");
    assert_eq!(scope("host", 1).as_deref(), Some("variable"), "a variable used");
    assert_eq!(scope("### Log", 0).as_deref(), Some("comment"), "the separator");
    assert_eq!(scope("HTTP/1.1", 0).as_deref(), Some("constant"));
}
