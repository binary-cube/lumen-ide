use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const C_SAMPLE: &str = include_str!("../data/sample.c");
const CPP_SAMPLE: &str = include_str!("../data/sample.cpp");

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(c_grammar());
    grammars.register(cpp_grammar());
    SyntaxConfig::new(grammars)
}

fn parse(config: &SyntaxConfig, language: &LanguageId, text: &str) -> SyntaxSnapshot {
    let doc = Document::new(text);
    parse_document(config, language, &doc.buffer().snapshot(), None, &[], 0).unwrap()
}

/// The scope drawn at the start of `needle`'s `nth` occurrence (the last span applied wins).
fn scope_at(
    config: &SyntaxConfig,
    language: &LanguageId,
    text: &str,
    needle: &str,
    nth: usize,
) -> Option<String> {
    let doc = Document::new(text);
    let snapshot = parse_document(config, language, &doc.buffer().snapshot(), None, &[], 0)?;
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
    g.register(c_grammar());
    g.register(cpp_grammar());
    assert_eq!(g.validate(), Vec::<String>::new());
}

#[test]
fn files_are_c_or_cpp_by_their_extension() {
    let mut registry = LanguageRegistry::default();
    registry.register(c_descriptor());
    registry.register(cpp_descriptor());
    for name in ["main.c", "LIB.C"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "c", "{name}");
    }
    for name in ["a.cpp", "a.cc", "a.cxx", "a.c++", "a.h", "a.hpp", "a.hh", "a.hxx", "a.ipp"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "cpp", "{name}");
    }
    for d in [c_descriptor(), cpp_descriptor()] {
        assert_eq!((d.line_comment, d.block_comment), (Some("//"), Some(("/*", "*/"))));
    }
}

#[test]
fn c_sample_is_highlighted() {
    let (c, s) = (config(), C_SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, &C, s, needle, nth);
    assert_eq!(at("/* A small", 0).as_deref(), Some("comment"));
    assert_eq!(at("// Done", 0).as_deref(), Some("comment"));
    assert_eq!(at("#include", 0).as_deref(), Some("keyword.directive"));
    assert_eq!(at("<stdio.h>", 0).as_deref(), Some("string"));
    assert_eq!(at("\"list.h\"", 0).as_deref(), Some("string"));
    assert_eq!(at("MAX_ITEMS", 0).as_deref(), Some("constant"));
    assert_eq!(at("SQUARE", 0).as_deref(), Some("function"));
    assert_eq!(at("typedef", 0).as_deref(), Some("keyword"));
    assert_eq!(at("struct", 0).as_deref(), Some("keyword"));
    assert_eq!(at("int value", 0).as_deref(), Some("type.builtin"));
    assert_eq!(at("unsigned long", 0).as_deref(), Some("type.builtin"));
    assert_eq!(at("value;", 0).as_deref(), Some("property"));
    assert_eq!(at("node_t;", 0).as_deref(), Some("type"));
    assert_eq!(at("RED", 0).as_deref(), Some("constant"));
    assert_eq!(at("count(const", 0).as_deref(), Some("function"));
    assert_eq!(at("head,", 0).as_deref(), Some("variable.parameter"));
    assert_eq!(at("limit)", 0).as_deref(), Some("variable.parameter"));
    assert_eq!(at("for", 0).as_deref(), Some("keyword"));
    assert_eq!(at("NULL &&", 0).as_deref(), Some("null"));
    assert_eq!(at("next)", 0).as_deref(), Some("property"));
    assert_eq!(at("printf", 0).as_deref(), Some("function.call"));
    assert_eq!(at("\"%d items", 0).as_deref(), Some("string"));
    assert_eq!(at("\\n", 0).as_deref(), Some("string.escape"));
    assert_eq!(at("'a'", 0).as_deref(), Some("string"));
    assert_eq!(at("true", 0).as_deref(), Some("boolean"));
    assert_eq!(at("0x1f", 0).as_deref(), Some("number"));
    assert_eq!(at("return", 0).as_deref(), Some("keyword"));
    assert_eq!(at("!=", 0).as_deref(), Some("operator"));
    assert_eq!(at("{\n    int value", 0).as_deref(), Some("punctuation.bracket"));
}

#[test]
fn cpp_sample_is_highlighted() {
    let (c, s) = (config(), CPP_SAMPLE);
    let at = |needle: &str, nth: usize| scope_at(&c, &CPP, s, needle, nth);
    assert_eq!(at("// A shape", 0).as_deref(), Some("comment"));
    assert_eq!(at("namespace", 0).as_deref(), Some("keyword"));
    assert_eq!(at("geometry", 0).as_deref(), Some("type"));
    assert_eq!(at("template", 0).as_deref(), Some("keyword"));
    assert_eq!(at("typename", 0).as_deref(), Some("keyword"));
    assert_eq!(at("class", 0).as_deref(), Some("keyword"));
    assert_eq!(at("Shape {", 0).as_deref(), Some("type"));
    assert_eq!(at("public", 0).as_deref(), Some("keyword"));
    assert_eq!(at("explicit", 0).as_deref(), Some("keyword"));
    assert_eq!(at("std::string name)", 0).as_deref(), Some("type"));
    assert_eq!(at("move", 0).as_deref(), Some("function.call"));
    assert_eq!(at("virtual", 0).as_deref(), Some("keyword"));
    assert_eq!(at("noexcept", 0).as_deref(), Some("keyword"));
    assert_eq!(at("area() const noexcept", 0).as_deref(), Some("function.method"));
    assert_eq!(at("name_;", 1).as_deref(), Some("property"));
    assert_eq!(at("auto total", 0).as_deref(), Some("type.builtin"));
    assert_eq!(at("total", 0).as_deref(), Some("function"));
    assert_eq!(at("shapes)", 0).as_deref(), Some("variable.parameter"));
    assert_eq!(at("double sum", 0).as_deref(), Some("type.builtin"));
    assert_eq!(at("0.0", 0).as_deref(), Some("number"));
    assert_eq!(at("area();", 0).as_deref(), Some("function.method"));
    assert_eq!(at("nullptr", 0).as_deref(), Some("null"));
    assert_eq!(at("empty()", 0).as_deref(), Some("function.method"));
    assert_eq!(at("lambda(values", 0).as_deref(), Some("function.call"));
    assert_eq!(at("::", 0).as_deref(), Some("operator"));
}

#[test]
fn grammar_errors_are_never_reported() {
    // Macros the grammar can't expand: the server's diagnostics are the real ones.
    for grammar in [c_grammar(), cpp_grammar()] {
        assert!(!grammar.own_errors && !grammar.embedded_errors && !grammar.injected_errors);
    }
}

#[test]
fn bodies_lists_and_conditional_compilation_fold() {
    let c = config();
    let folds = |language: &LanguageId, text: &str| -> Vec<u32> {
        parse(&c, language, text).folds().iter().map(|f| f.start_line).collect()
    };
    let c_folds = folds(&C, C_SAMPLE);
    // The struct's fields, the function's body, the loop's body, main's body.
    for line in [7, 14, 16, 22] {
        assert!(c_folds.contains(&line), "line {line} in {c_folds:?}");
    }
    let ifdef = "#ifdef DEBUG\nint a;\nint b;\n#endif\n";
    assert!(folds(&C, ifdef).contains(&0));
    let cpp_folds = folds(&CPP, CPP_SAMPLE);
    // The namespace, the class, the function, the loop.
    for line in [4, 7, 18, 20] {
        assert!(cpp_folds.contains(&line), "line {line} in {cpp_folds:?}");
    }
}

#[test]
fn new_lines_indent_inside_braces() {
    let c = config();
    for language in [C, CPP] {
        let text = "int f(void) {\n    return 1;\n}\n";
        let doc = Document::new(text);
        let snapshot = parse(&c, &language, text);
        let inside = text.find("    return").unwrap();
        let anchor = snapshot.indent_anchor(&doc.buffer().snapshot(), inside);
        assert_eq!(anchor.map(|a| a.line), Some(0), "{language}");
    }
}

#[test]
fn clangd_is_described_for_both_languages() {
    let text = include_str!("../../data/language-servers.toml");
    assert!(text.contains("name = \"clangd\""));
    assert!(text.contains("languages = [\"c\", \"cpp\"]"));
}
