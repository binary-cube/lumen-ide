use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SAMPLE: &str = include_str!("../data/sample.toml");

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    SyntaxConfig::new(grammars)
}

fn parse(config: &SyntaxConfig, text: &str) -> SyntaxSnapshot {
    let doc = Document::new(text);
    parse_document(config, &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0).unwrap()
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
fn toml_files_are_found_by_extension_or_name() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["Cargo.toml", "pyproject.toml", "A.TOML", "Cargo.lock", "uv.lock", "Pipfile"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "toml", "{name}");
    }
    // Git's and SSH's `config` aren't TOML.
    assert_ne!(registry.detect(Path::new("config"), "").as_str(), "toml");
    assert_eq!(descriptor().line_comment, Some("#"));
}

#[test]
fn the_sample_is_highlighted() {
    let (c, s) = (config(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("# The package", 0).as_deref(), Some("comment"));
    assert_eq!(at("package]", 0).as_deref(), Some("tag"));
    assert_eq!(at("name =", 0).as_deref(), Some("property"));
    assert_eq!(at("\"shop\"", 0).as_deref(), Some("string"));
    assert_eq!(at("=", 0).as_deref(), Some("operator"));
    assert_eq!(at("'Ion'", 0).as_deref(), Some("string"));
    assert_eq!(at("dependencies]", 0).as_deref(), Some("tag"));
    assert_eq!(at("version = \"1.0\"", 0).as_deref(), Some("property"));
    assert_eq!(at("workspace", 0).as_deref(), Some("property"));
    assert_eq!(at("\"quoted-key\"", 0).as_deref(), Some("property"));
    assert_eq!(at("target.", 0).as_deref(), Some("tag"));
    assert_eq!(at("'cfg(unix)'", 0).as_deref(), Some("tag"));
    assert_eq!(at("bin]]", 0).as_deref(), Some("tag"));
    assert_eq!(at("false", 0).as_deref(), Some("boolean"));
    assert_eq!(at("3\n", 0).as_deref(), Some("number"));
    assert_eq!(at("0.5", 0).as_deref(), Some("number"));
    assert_eq!(at("2026-11-02", 0).as_deref(), Some("constant"));
    assert_eq!(at("\\t", 0).as_deref(), Some("string.escape"));
}

#[test]
fn the_sample_reads_without_errors() {
    assert!(parse(&config(), SAMPLE).errors().is_empty());
}

#[test]
fn tables_arrays_and_inline_tables_fold() {
    let c = config();
    let starts: Vec<u32> = parse(&c, SAMPLE).folds().iter().map(|f| f.start_line).collect();
    // `[package]`, its `authors`, `[dependencies]`, `[[bin]]`, `[profile.release]`.
    for line in [1, 5, 10, 18, 23] {
        assert!(starts.contains(&line), "line {line} in {starts:?}");
    }
}

#[test]
fn new_lines_indent_inside_arrays_and_inline_tables() {
    let c = config();
    for (text, inside) in [("a = [\n  1,\n]\n", "  1"), ("t = {\n  a = 1 }\n", "  a")] {
        let doc = Document::new(text);
        let snapshot = parse(&c, text);
        let at = text.find(inside).unwrap();
        let anchor = snapshot.indent_anchor(&doc.buffer().snapshot(), at);
        assert_eq!(anchor.map(|a| a.line), Some(0), "{text}");
    }
    // A table's keys stay at its header's level.
    let text = "[package]\nname = \"x\"\n";
    let doc = Document::new(text);
    let at = text.find("name").unwrap();
    assert_eq!(parse(&c, text).indent_anchor(&doc.buffer().snapshot(), at), None);
}

#[test]
fn taplo_is_described_offline() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"taplo\""));
    assert!(text.contains("catalogs = []"), "SchemaStore's catalog never fetched");
    assert!(text.contains("HTTPS_PROXY = \"http://127.0.0.1:9\""), "nothing fetched");
    assert!(text.contains("${server-uri}/schemas/cargo.json"));
}

/// A long row highlighted in parts (its drawn part and the short rows around it): each byte
/// in them gets the scope highlighting the whole text gives it.
#[test]
fn highlighting_in_parts_colors_as_the_whole() {
    let config = config();
    let long: String = (0..2000).map(|i| format!("{i}, \"s{i}\", true, ")).collect();
    let text = format!("# head\n[table]\nkey = [{long}0]\nother = 'x' # tail\n");
    let doc = Document::new(&text);
    let snapshot = parse(&config, &text);
    let buffer = doc.buffer();
    let line = |n: usize| buffer.line_start(n)..buffer.line_end(n);
    let windows = [
        buffer.line_start(0)..buffer.line_end(1),
        buffer.line_start(2)..buffer.line_start(2) + 700,
        line(3),
    ];
    let mut highlighter = Highlighter::new();
    let (mut whole, mut parts) = (Vec::new(), Vec::new());
    highlighter.highlight(&snapshot, &buffer.snapshot(), 0..text.len(), &mut whole);
    highlighter.highlight_ranges(&snapshot, &buffer.snapshot(), &windows, &mut parts);
    assert!(parts.len() < whole.len() / 4, "the long row's rest not highlighted");
    let scope = |spans: &[syntax_ts::HighlightSpan], at: usize| {
        spans.iter().rev().find(|s| s.range.contains(&at)).map(|s| s.scope)
    };
    for window in &windows {
        for at in window.clone() {
            assert_eq!(scope(&parts, at), scope(&whole, at), "byte {at}");
        }
    }
}
