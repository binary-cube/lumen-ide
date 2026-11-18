use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SAMPLE: &str = include_str!("../data/sample.lua");

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
fn files_are_lua_by_their_extension_or_shebang() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["init.lua", "A.LUA", ".luacheckrc"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "lua", "{name}");
    }
    assert_eq!(registry.detect(Path::new("run"), "#!/usr/bin/env lua\n").as_str(), "lua");
    let d = descriptor();
    assert_eq!((d.line_comment, d.block_comment), (Some("--"), Some(("--[[", "]]"))));
}

#[test]
fn the_sample_is_highlighted() {
    let (c, s) = (config(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("#!/usr", 0).as_deref(), Some("comment"));
    assert_eq!(at("-- A small", 0).as_deref(), Some("comment"));
    assert_eq!(at("--[[ A new", 0).as_deref(), Some("comment"));
    assert_eq!(at("local Queue", 0).as_deref(), Some("keyword"));
    assert_eq!(at("__index", 0).as_deref(), Some("property"));
    assert_eq!(at("MAX_SIZE", 0).as_deref(), Some("constant"));
    assert_eq!(at("const>", 0).as_deref(), Some("attribute"));
    assert_eq!(at("128", 0).as_deref(), Some("number"));
    assert_eq!(at("function Queue.new", 0).as_deref(), Some("keyword"));
    assert_eq!(at("new(...)", 0).as_deref(), Some("function"));
    assert_eq!(at("...", 0).as_deref(), Some("constant.builtin"));
    assert_eq!(at("setmetatable", 0).as_deref(), Some("function.builtin"));
    assert_eq!(at("first = 1", 0).as_deref(), Some("property"));
    assert_eq!(at("self = ", 0).as_deref(), Some("variable.builtin"));
    assert_eq!(at("push(value)", 0).as_deref(), Some("function.method"));
    assert_eq!(at("value)", 0).as_deref(), Some("variable.parameter"));
    assert_eq!(at("then", 0).as_deref(), Some("keyword"));
    assert_eq!(at("\"full\"", 0).as_deref(), Some("string"));
    assert_eq!(at("nil then", 0).as_deref(), Some("null"));
    assert_eq!(at("false", 0).as_deref(), Some("boolean"));
    assert_eq!(at("new()", 0).as_deref(), Some("function.call"));
    assert_eq!(at("push(i", 0).as_deref(), Some("function.method"));
    assert_eq!(at("2.5", 0).as_deref(), Some("number"));
    assert_eq!(at("print", 0).as_deref(), Some("function.builtin"));
    assert_eq!(at("\\n", 0).as_deref(), Some("string.escape"));
    assert_eq!(at("not q", 0).as_deref(), Some("keyword.operator"));
    assert_eq!(at("done::", 0).as_deref(), Some("label"));
    assert_eq!(at(">=", 0).as_deref(), Some("operator"));
}

#[test]
fn functions_blocks_and_tables_fold() {
    let c = config();
    let starts: Vec<u32> = parse(&c, SAMPLE).folds().iter().map(|f| f.start_line).collect();
    // The long comment, `Queue.new`, `Queue:push`, its `if`, the `for` loop.
    for line in [7, 9, 14, 15, 26] {
        assert!(starts.contains(&line), "line {line} in {starts:?}");
    }
}

#[test]
fn new_lines_indent_inside_functions_and_tables() {
    let c = config();
    for (text, inside) in [
        ("function f()\n  return 1\nend\n", "  return"),
        ("local t = {\n  a = 1,\n}\n", "  a"),
        ("if x then\n  y()\nend\n", "  y"),
    ] {
        let doc = Document::new(text);
        let snapshot = parse(&c, text);
        let at = text.find(inside).unwrap();
        let anchor = snapshot.indent_anchor(&doc.buffer().snapshot(), at);
        assert_eq!(anchor.map(|a| a.line), Some(0), "{text}");
    }
}

#[test]
fn a_block_closed_by_end_on_its_line_indents_nothing_after_it() {
    let c = config();
    let text = "function f() return 1 end\n";
    let doc = Document::new(text);
    let snapshot = parse(&c, text);
    let after = text.find('\n').unwrap();
    assert_eq!(snapshot.indent_anchor(&doc.buffer().snapshot(), after).map(|a| a.line), None);
}

#[test]
fn lua_language_server_is_described() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"lua-language-server\""));
    assert!(text.contains("--metapath=${cache}/meta"), "never written into the bundle");
}
