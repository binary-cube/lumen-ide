use super::*;

const SOURCE: &str = "function total(a, b) {\n  return add(a, 'x');\n}\n";

fn ranges(range: Range<usize>) -> Vec<&'static str> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let tree = parser.parse(SOURCE, None).unwrap();
    selection_ranges(&tree, range).into_iter().map(|r| &SOURCE[r]).collect()
}

#[test]
fn a_cursor_grows_from_its_token_to_the_whole_document() {
    let at = SOURCE.find("add").unwrap() + 1;
    let found = ranges(at..at);
    assert_eq!(found[0], "add", "the name first");
    assert_eq!(found[1], "add(a, 'x')", "then the call");
    assert!(found.contains(&"return add(a, 'x');"), "the statement: {found:?}");
    // Inside the body's braces before the braces themselves.
    let body = found.iter().position(|r| r.starts_with('{')).unwrap();
    assert_eq!(found[body - 1], "return add(a, 'x');");
    assert_eq!(*found.last().unwrap(), SOURCE, "the whole document last");
}

#[test]
fn inside_delimiters_comes_before_the_delimiters() {
    let at = SOURCE.find("a, 'x'").unwrap();
    let found = ranges(at..at + 1);
    assert_eq!(found[0], "a, 'x'", "the arguments without their parentheses");
    assert_eq!(found[1], "(a, 'x')");
    let quote = SOURCE.find("'x'").unwrap() + 1;
    let found = ranges(quote..quote + 1);
    assert_eq!(found[0], "'x'", "a one-character string: nothing smaller to add inside");
}

#[test]
fn every_range_holds_the_one_before() {
    let at = SOURCE.find('b').unwrap();
    let found = ranges(at..at + 1);
    assert!(!found.is_empty());
    for pair in found.windows(2) {
        assert!(pair[1].len() > pair[0].len() && pair[1].contains(pair[0]), "{pair:?}");
    }
    assert!(!found.contains(&"b"), "the selection itself isn't a step");
}
