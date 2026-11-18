use super::*;

const UNITS: &[&str] = &[
    "expression_statement",
    "function_declaration",
    "lexical_declaration",
    "return_statement",
    "method_definition",
    "class_declaration",
];

const SOURCE: &str = "// one\nfunction a() {\n  x();\n  y();\n}\n\n/** doc */\nfunction b() {}\n";

fn moved(lines: Range<usize>, up: bool) -> Moved {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let tree = parser.parse(SOURCE, None).unwrap();
    statement_move(&tree, SOURCE, lines, up, UNITS)
}

#[test]
fn a_function_moves_whole_with_its_comment_past_its_sibling() {
    // On `function b`: it goes above `a` (each with its comment), the blank line between.
    assert_eq!(moved(7..8, true), Moved::Past { unit: 6..8, past: 0..5 });
    // On `function a`'s header or its closing brace: the whole of `a` goes down.
    assert_eq!(moved(1..2, false), Moved::Past { unit: 0..5, past: 6..8 });
    assert_eq!(moved(4..5, false), Moved::Past { unit: 0..5, past: 6..8 });
}

#[test]
fn a_statement_stays_in_its_block() {
    assert_eq!(moved(3..4, true), Moved::Past { unit: 3..4, past: 2..3 });
    assert_eq!(moved(2..3, true), Moved::Stay, "the first of its block");
    assert_eq!(moved(3..4, false), Moved::Stay, "the last of its block");
    assert_eq!(moved(5..6, true), Moved::Lines, "a blank line moves alone");
}
