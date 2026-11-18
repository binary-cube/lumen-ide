use crate::{Grammar, Grammars, SyntaxConfig, parse_document};
use document::Document;
use languages::LanguageId;

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(Grammar {
        language: LanguageId::new_static("javascript"),
        ts: tree_sitter_javascript::LANGUAGE.into(),
        highlights: "".into(),
        injections: "".into(),
        folds: "".into(),
        indents: "[(statement_block) (object) (arguments)] @indent".into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    });
    SyntaxConfig::new(grammars)
}

/// The anchor of a new line at `|`: the line of the innermost open node there.
fn anchor(source: &str) -> Option<usize> {
    let at = source.find('|').unwrap();
    let text = source.replace('|', "");
    let doc = Document::new(&text);
    let config = config();
    let snapshot = doc.buffer().snapshot();
    let parsed =
        parse_document(&config, &LanguageId::new_static("javascript"), &snapshot, None, &[], 0)
            .unwrap();
    parsed.indent_anchor(&snapshot, at).map(|a| a.line)
}

#[test]
fn a_new_line_anchors_on_the_innermost_open_node() {
    assert_eq!(anchor("function a() {|}"), Some(0), "between the braces");
    assert_eq!(anchor("function a() {\n  if (x) {|\n  }\n}"), Some(1), "the inner block");
    assert_eq!(anchor("const o = {\n  a: f(1,|2),\n};"), Some(1), "the arguments");
    assert_eq!(anchor("function a() {\n  if (x) {|\n}"), Some(1), "not closed yet");
    // An unfinished file the parser can't make out: no anchor (the text rule decides).
    assert_eq!(anchor("function a() {|"), None);
    assert_eq!(anchor("function a() {}|"), None, "closed: nothing open");
    assert_eq!(anchor("let x = 1;|"), None);
}
