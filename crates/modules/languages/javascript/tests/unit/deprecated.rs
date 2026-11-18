/// `text` parsed as JavaScript.
fn parsed(text: &str) -> syntax_ts::tree_sitter::Tree {
    let mut parser = syntax_ts::tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    parser.parse(text, None).unwrap()
}

#[test]
fn finds_deprecated_apis() {
    let text = "const a = escape(x); document.execCommand('copy'); document.getElementById('a');";
    let found = super::find(&parsed(text), text);
    let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["escape()", "document.execCommand"]);
}
