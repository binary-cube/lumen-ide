use super::with_cursor;

#[test]
fn a_snippet_places_the_cursor_and_keeps_php_dollars_literal() {
    assert_eq!(with_cursor("foo()", 1), "foo($0)");
    assert_eq!(with_cursor("$a[]", 1), "\\$a[$0]");
    assert_eq!(with_cursor("{}", 1), "{$0\\}");
}
