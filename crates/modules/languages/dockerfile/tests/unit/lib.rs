use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SAMPLE: &str = include_str!("../data/Dockerfile");

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    SyntaxConfig::new(grammars)
}

/// With the Bash grammar too, as the Bash module contributes it (what the shell commands are
/// colored as).
fn with_bash() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    grammars.register(Grammar {
        language: LanguageId::new_static("bash"),
        ts: tree_sitter_bash::LANGUAGE.into(),
        highlights: include_str!("../../../bash/queries/highlights.scm").into(),
        injections: "".into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    });
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
fn dockerfiles_are_found_by_their_names() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in [
        "Dockerfile",
        "dockerfile",
        "Containerfile",
        "Dockerfile.dev",
        "dockerfile.prod",
        "api.dockerfile",
        "Containerfile.base",
    ] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "dockerfile", "{name}");
    }
    for name in ["Dockerfiles.md", "docker-compose.yml", "compose.yaml", ".dockerignore"] {
        assert_ne!(registry.detect(Path::new(name), "").as_str(), "dockerfile", "{name}");
    }
    assert_eq!(descriptor().line_comment, Some("#"));
}

#[test]
fn the_sample_is_highlighted() {
    let (c, s) = (config(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("# syntax", 0).as_deref(), Some("comment"));
    assert_eq!(at("ARG", 0).as_deref(), Some("keyword"));
    assert_eq!(at("PHP_VERSION=", 0).as_deref(), Some("variable"));
    assert_eq!(at("FROM node", 0).as_deref(), Some("keyword"));
    assert_eq!(at("node:22", 0).as_deref(), Some("type"));
    assert_eq!(at(":22-alpine", 0).as_deref(), Some("constant"));
    assert_eq!(at("AS assets", 0).as_deref(), Some("keyword"));
    assert_eq!(at("assets\n", 0).as_deref(), Some("label"));
    assert_eq!(at("WORKDIR", 0).as_deref(), Some("keyword"));
    assert_eq!(at("--mount", 0).as_deref(), Some("attribute"));
    assert_eq!(at("${PHP_VERSION}", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at("PHP_VERSION}", 0).as_deref(), Some("constant"));
    assert_eq!(at("@sha256", 0).as_deref(), Some("constant"));
    assert_eq!(at("APP_ENV", 0).as_deref(), Some("variable"));
    assert_eq!(at("org.opencontainers", 0).as_deref(), Some("property"));
    assert_eq!(at("\"shop\"", 0).as_deref(), Some("string"));
    assert_eq!(at("--from=assets", 0).as_deref(), Some("attribute"));
    assert_eq!(at("<<EOT", 0).as_deref(), Some("label"));
    assert_eq!(at("9000", 0).as_deref(), Some("number"));
    assert_eq!(at("\"php-fpm\"", 0).as_deref(), Some("string"));
}

#[test]
fn shell_commands_are_colored_as_bash() {
    let (c, s) = (with_bash(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("npm ci", 0).as_deref(), Some("function.call"));
    assert_eq!(at("&& npm", 0).as_deref(), Some("operator"));
    assert_eq!(at("apt-get update", 0).as_deref(), Some("function.call"));
    assert_eq!(at("-y", 0).as_deref(), Some("attribute"));
    assert_eq!(at("exit 1", 0).as_deref(), Some("function.builtin"));
    // Without the Bash module, they're the Dockerfile's plain text.
    let plain = scope_at(&config(), s, "npm ci", 0);
    assert_ne!(plain.as_deref(), Some("function.call"));
}

#[test]
fn the_sample_reads_without_errors() {
    let snapshot = parse(&with_bash(), SAMPLE);
    assert!(snapshot.errors().is_empty(), "{:?}", snapshot.errors());
}

#[test]
fn long_instructions_and_heredocs_fold() {
    let c = config();
    let starts: Vec<u32> = parse(&c, SAMPLE).folds().iter().map(|f| f.start_line).collect();
    // The continued `RUN`, the continued `ENV`, the `RUN` with a here-document (once).
    for line in [7, 11, 15] {
        assert!(starts.contains(&line), "line {line} in {starts:?}");
    }
    assert!(!starts.contains(&16), "{starts:?}");
}

#[test]
fn dockerfile_language_server_is_described() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"dockerfile-language-server\""));
    assert!(text.contains("languages = [\"dockerfile\"]"));
}
