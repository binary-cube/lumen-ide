use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SAMPLE: &str = include_str!("../data/sample.md");

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    grammars.register(inline_grammar());
    SyntaxConfig::new(grammars)
}

fn parse(config: &SyntaxConfig, text: &str) -> SyntaxSnapshot {
    let doc = Document::new(text);
    parse_document(config, &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0).unwrap()
}

/// The scope drawn at the start of `needle`'s `nth` occurrence (the last span applied wins).
fn scope_at(config: &SyntaxConfig, text: &str, needle: &str, nth: usize) -> Option<String> {
    let doc = Document::new(text);
    let snapshot = parse(config, text);
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
    g.register(inline_grammar());
    assert_eq!(g.validate(), Vec::<String>::new());
}

#[test]
fn markdown_files_are_found_by_extension_or_name() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["README.md", "docs/guide.markdown", "A.MD", "notes.mdx", "README"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "markdown", "{name}");
    }
    assert_ne!(registry.detect(Path::new("main.rs"), "").as_str(), "markdown");
}

/// Headings, the text's emphasis, code and links (the inline grammar, injected), quotes,
/// lists, a table's header: each in its theme's name.
#[test]
fn the_sample_is_highlighted() {
    let (c, s) = (config(), SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("The project", 0).as_deref(), Some("keyword"), "a heading");
    assert_eq!(at("# The", 0).as_deref(), Some("keyword"), "its marker");
    assert_eq!(at("bold", 0).as_deref(), Some("keyword"), "strong");
    assert_eq!(at("slanted", 0).as_deref(), Some("variable"), "emphasis");
    assert_eq!(at("code`", 0).as_deref(), Some("string"), "a code span");
    assert_eq!(at("link]", 0).as_deref(), Some("function"), "a link's text");
    assert_eq!(at("https://", 0).as_deref(), Some("string"), "its address");
    assert_eq!(at("- one", 0).as_deref(), Some("punctuation"), "a list's marker");
    assert_eq!(at("A quote", 0).as_deref(), Some("comment"), "a quote");
    assert_eq!(at("php\n", 0).as_deref(), Some("type"), "a code block's language");
}

/// A section folds from its heading; a code block and a quote on their own.
#[test]
fn sections_and_blocks_fold() {
    let c = config();
    let starts: Vec<u32> = parse(&c, SAMPLE).folds().iter().map(|f| f.start_line).collect();
    assert!(starts.contains(&0), "the first section: {starts:?}");
    assert!(starts.contains(&4), "## Install: {starts:?}");
    assert!(starts.contains(&11), "the code block: {starts:?}");
}
