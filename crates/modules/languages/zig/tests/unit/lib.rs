use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SAMPLE: &str = include_str!("../data/sample.zig");

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
fn files_are_zig_by_their_extension() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["main.zig", "build.zig", "build.zig.zon"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "zig", "{name}");
    }
    assert_eq!(descriptor().line_comment, Some("//"));
}

#[test]
fn the_sample_is_highlighted() {
    let (c, s) = (config(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("//! A tiny", 0).as_deref(), Some("comment"));
    assert_eq!(at("/// The most", 0).as_deref(), Some("comment"));
    assert_eq!(at("const std", 0).as_deref(), Some("keyword"));
    assert_eq!(at("@import", 0).as_deref(), Some("function.builtin"));
    assert_eq!(at("\"std\"", 0).as_deref(), Some("string"));
    assert_eq!(at("MAX_ITEMS", 0).as_deref(), Some("constant"));
    assert_eq!(at("usize", 0).as_deref(), Some("type.builtin"));
    assert_eq!(at("16", 0).as_deref(), Some("number"));
    assert_eq!(at("pub const", 0).as_deref(), Some("keyword"));
    assert_eq!(at("Stack = struct", 0).as_deref(), Some("type"));
    assert_eq!(at("struct", 0).as_deref(), Some("keyword"));
    assert_eq!(at("items:", 0).as_deref(), Some("property"));
    assert_eq!(at("i32", 0).as_deref(), Some("type.builtin"));
    assert_eq!(at("undefined", 0).as_deref(), Some("constant.builtin"));
    assert_eq!(at("fn push", 0).as_deref(), Some("keyword"));
    assert_eq!(at("push(self", 0).as_deref(), Some("function"));
    assert_eq!(at("self: *Stack", 0).as_deref(), Some("variable.parameter"));
    assert_eq!(at("Stack, value", 0).as_deref(), Some("type"));
    assert_eq!(at("len ==", 0).as_deref(), Some("property"));
    assert_eq!(at("return error", 0).as_deref(), Some("keyword"));
    assert_eq!(at("null;", 0).as_deref(), Some("null"));
    assert_eq!(at("test \"push", 0).as_deref(), Some("keyword"));
    assert_eq!(at("try", 0).as_deref(), Some("keyword"));
    assert_eq!(at("push(42)", 0).as_deref(), Some("function.method"));
    assert_eq!(at("orelse", 0).as_deref(), Some("keyword"));
    assert_eq!(at("unreachable", 0).as_deref(), Some("constant.builtin"));
    assert_eq!(at("print", 0).as_deref(), Some("function.method"));
    assert_eq!(at("\\n", 0).as_deref(), Some("string.escape"));
    assert_eq!(at("true", 0).as_deref(), Some("boolean"));
    assert_eq!(at("and 'x'", 0).as_deref(), Some("keyword.operator"));
    assert_eq!(at("'x'", 0).as_deref(), Some("string"));
    assert_eq!(at("3.5", 0).as_deref(), Some("number"));
    assert_eq!(at("_ = ok", 0).as_deref(), Some("variable.builtin"));
    assert_eq!(at("+=", 0).as_deref(), Some("operator"));
}

#[test]
fn declarations_and_blocks_fold() {
    let c = config();
    let starts: Vec<u32> = parse(&c, SAMPLE).folds().iter().map(|f| f.start_line).collect();
    // The struct, `push`'s body, `pop`'s body, the test.
    for line in [6, 10, 16, 23] {
        assert!(starts.contains(&line), "line {line} in {starts:?}");
    }
}

#[test]
fn new_lines_indent_inside_blocks() {
    let c = config();
    let text = "fn f() void {\n    return;\n}\n";
    let doc = Document::new(text);
    let snapshot = parse(&c, text);
    let at = text.find("    return").unwrap();
    let anchor = snapshot.indent_anchor(&doc.buffer().snapshot(), at);
    assert_eq!(anchor.map(|a| a.line), Some(0));
}

#[test]
fn zls_is_described_without_zig() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"zls\""));
    assert!(!text.contains("zig_exe_path"), "zig is found in PATH by zls itself");
}
