use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SAMPLE: &str = include_str!("../data/sample.sh");

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
fn scripts_are_found_by_extension_name_or_shebang() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["build.sh", "A.BASH", "prompt.zsh", ".bashrc", ".zshrc", ".profile", "PKGBUILD"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "bash", "{name}");
    }
    for shebang in ["#!/bin/sh\n", "#!/usr/bin/env bash\n", "#!/bin/zsh -f\n", "#!/bin/dash\n"] {
        assert_eq!(registry.detect(Path::new("run"), shebang).as_str(), "bash", "{shebang}");
    }
    assert_eq!(descriptor().line_comment, Some("#"));
}

#[test]
fn the_sample_is_highlighted() {
    let (c, s) = (config(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("#!/usr", 0).as_deref(), Some("comment"));
    assert_eq!(at("# Backs", 0).as_deref(), Some("comment"));
    assert_eq!(at("set -euo", 0).as_deref(), Some("function.builtin"));
    assert_eq!(at("-euo", 0).as_deref(), Some("attribute"));
    assert_eq!(at("readonly", 0).as_deref(), Some("keyword"));
    assert_eq!(at("KEEP=5", 0).as_deref(), Some("variable"));
    assert_eq!(at("5\n", 0).as_deref(), Some("number"));
    assert_eq!(at("\"${1", 0).as_deref(), Some("string"));
    assert_eq!(at("${1", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at("HOME", 0).as_deref(), Some("variable"));
    assert_eq!(at("backup()", 0).as_deref(), Some("function"));
    assert_eq!(at("local", 0).as_deref(), Some("keyword"));
    assert_eq!(at("date", 0).as_deref(), Some("function.call"));
    assert_eq!(at("$(date", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at("tar -czf", 0).as_deref(), Some("function.call"));
    assert_eq!(at("2>", 0).as_deref(), Some("number"));
    assert_eq!(at("||", 0).as_deref(), Some("operator"));
    assert_eq!(at("return", 0).as_deref(), Some("function.builtin"));
    assert_eq!(at("echo \"saved", 0).as_deref(), Some("function.builtin"));
    assert_eq!(at("$? -eq", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at("? -eq", 0).as_deref(), Some("variable.builtin"));
    assert_eq!(at("if [[", 0).as_deref(), Some("keyword"));
    assert_eq!(at("-d", 0).as_deref(), Some("operator"));
    assert_eq!(at("elif", 0).as_deref(), Some("keyword"));
    assert_eq!(at("^/tmp", 0).as_deref(), Some("string.regex"));
    assert_eq!(at("'a temporary", 0).as_deref(), Some("string"));
    assert_eq!(at("for dir", 0).as_deref(), Some("keyword"));
    assert_eq!(at("done\n", 0).as_deref(), Some("keyword"));
    assert_eq!(at("case", 0).as_deref(), Some("keyword"));
    assert_eq!(at("esac", 0).as_deref(), Some("keyword"));
    assert_eq!(at("$((", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at("NOTE\n", 0).as_deref(), Some("label"));
    assert_eq!(at("done: $?", 0).as_deref(), Some("string"));
    // Inside a string: `$stamp`.
    assert_eq!(at("$stamp.tar", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at("stamp.tar", 0).as_deref(), Some("variable"));
}

#[test]
fn functions_conditions_loops_and_cases_fold() {
    let c = config();
    let starts: Vec<u32> = parse(&c, SAMPLE).folds().iter().map(|f| f.start_line).collect();
    // `backup`, the `if`, its `elif`, the `for` loop, the `case`, the here-document.
    for line in [7, 15, 17, 21, 25, 30] {
        assert!(starts.contains(&line), "line {line} in {starts:?}");
    }
}

#[test]
fn new_lines_indent_inside_blocks() {
    let c = config();
    for (text, inside) in [
        ("f() {\n  echo 1\n}\n", "  echo"),
        ("if true; then\n  echo 1\nfi\n", "  echo"),
        ("for x in a; do\n  echo 1\ndone\n", "  echo"),
        ("case $x in\n  a) echo ;;\nesac\n", "  a)"),
    ] {
        let doc = Document::new(text);
        let snapshot = parse(&c, text);
        let at = text.find(inside).unwrap();
        let anchor = snapshot.indent_anchor(&doc.buffer().snapshot(), at);
        assert_eq!(anchor.map(|a| a.line), Some(0), "{text}");
    }
}

#[test]
fn a_block_closed_on_its_line_indents_nothing_after_it() {
    let c = config();
    for text in ["if true; then echo; fi\n", "for x in a; do echo; done\n"] {
        let doc = Document::new(text);
        let snapshot = parse(&c, text);
        let after = text.find('\n').unwrap();
        let anchor = snapshot.indent_anchor(&doc.buffer().snapshot(), after);
        assert_eq!(anchor.map(|a| a.line), None, "{text}");
    }
}

/// zsh's startup files read well enough: the grammar finds no error in a typical one.
#[test]
fn a_typical_zshrc_reads_without_errors() {
    let c = config();
    let text = include_str!("../data/sample.zshrc");
    let snapshot = parse(&c, text);
    let errors = [snapshot.errors(), snapshot.quiet_errors()].concat();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn bash_language_server_is_described() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"bash-language-server\""));
    assert!(text.contains("explainshellEndpoint = \"\""), "nothing read from the network");
    assert!(!text.contains("shellcheck\" ="), "ShellCheck isn't bundled");
}
