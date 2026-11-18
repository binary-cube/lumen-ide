use super::*;

/// A line's opener in a string (as the caller's language says): not a comment's.
fn quoted(line: &str) -> bool {
    line.matches('\'').count() % 2 == 1
}

#[test]
fn the_block_around_a_place() {
    let text = "a /* one */ b /** two\n * three */ c";
    let at = |needle: &str| block_at(text, text.find(needle).unwrap(), &quoted);
    assert_eq!(at("one"), Some((2, 11)));
    assert_eq!(at(" b"), None, "after it closed");
    let (open, close) = at("three").unwrap();
    assert_eq!(&text[open..close], "/** two\n * three */");
    assert!(is_doc(text, open) && !is_doc(text, 2));
    assert!(!is_doc("/**/", 0), "an empty comment");
    let text = "x = '/*'; y /* real */";
    assert_eq!(block_at(text, text.find("real").unwrap(), &quoted), Some((12, 22)));
    assert_eq!(block_at("'/*' z", 5, &quoted), None, "the only opener is quoted");
    assert_eq!(block_at("/* open", 5, &quoted), Some((0, 7)), "not closed yet");
}

#[test]
fn a_doc_line_without_its_margin() {
    let text = "/**\n * @param int $x\n */";
    let line = doc_line(text, (0, text.len()), text.find("int").unwrap());
    assert_eq!(&text[line], "@param int $x");
    let text = "/** @var User */";
    let line = doc_line(text, (0, text.len()), 6);
    assert_eq!(&text[line], "@var User");
}

#[test]
fn the_name_around_a_place() {
    let s = "array<App\\User, int-mask>";
    let name = |at: usize| word_around(s, at, &|c| c == '\\' || c == '-').map(|r| &s[r]);
    assert_eq!(name(8), Some("App\\User"));
    assert_eq!(name(s.find(',').unwrap()), Some("App\\User"), "right after it");
    assert_eq!(name(s.find("mask").unwrap()), Some("int-mask"));
    assert_eq!(word_around(" , ", 1, &|_| false), None);
    assert_eq!(first_word("  User $u"), (Some(2..6), 6));
    assert_eq!(first_word("   "), (None, 3));
}

#[test]
fn the_code_between_backticks_around_a_place() {
    let text = "// Use `Cart::total()` or ``a `b` c`` here\n// `next`";
    let inside = |needle: &str| backticked(text, text.find(needle).unwrap()).map(|r| &text[r]);
    assert_eq!(inside("total"), Some("Cart::total()"));
    assert_eq!(inside("Cart"), Some("Cart::total()"));
    assert_eq!(inside("a `b"), Some("a `b` c"), "double backticks hold single ones");
    assert_eq!(inside("here"), None, "after them");
    assert_eq!(inside("Use"), None, "before them");
    assert_eq!(inside("next"), Some("next"), "each line its own");
    assert_eq!(backticked("// `open only", 6), None, "never closed");
}

#[test]
fn a_word_of_code_and_the_one_before_it() {
    let code = "new Cart($items)";
    let (range, before) = code_word(code, 5).unwrap();
    assert_eq!((&code[range], before), ("Cart($items)", Some("new")));
    assert_eq!(code_word(code, 1).map(|(r, b)| (&code[r], b)), Some(("new", None)));
}

#[test]
fn a_doc_tag_being_typed_starts_the_line_s_text_or_an_inline_tag() {
    let at = |text: &str| tag_at(text, text.len());
    assert_eq!(at("/**\n * @par"), Some(7));
    assert_eq!(at("/** @"), Some(4));
    assert_eq!(at("/// @ret"), Some(4), "SassDoc's margin");
    assert_eq!(at("/**\n * See {@li"), Some(12), "an inline tag");
    assert_eq!(at("/**\n * mail me@ex"), None, "an address");
    assert_eq!(at("/**\n * text @x"), None, "inside the text");
}

/// The parts of `comment` as (text, kind).
fn parts<'a>(comment: &'a str, style: &DocStyle<'_>) -> Vec<(&'a str, DocText)> {
    doc_parts(comment, style).into_iter().map(|(r, k)| (&comment[r], k)).collect()
}

#[test]
fn a_braced_doc_comment_s_tags_types_and_names() {
    let style = DocStyle {
        typed: &|t| matches!(t, "param" | "returns"),
        named: &|t| t == "param",
        is_name: &|_| true,
        type_len: None,
    };
    let comment = "/**\n * Totals it. {@link Cart}\n * @param {Map<string, {a: 1}>} [items=x] the items\n * @returns {number}\n * @since 2\n */";
    assert_eq!(
        parts(comment, &style),
        [
            ("@link", DocText::Tag),
            ("@param", DocText::Tag),
            ("Map<string, {a: 1}>", DocText::Type),
            ("items", DocText::Name),
            ("@returns", DocText::Tag),
            ("number", DocText::Type),
            ("@since", DocText::Tag),
        ]
    );
}

#[test]
fn a_first_word_type_doc_comment_and_text_left_alone() {
    let style = DocStyle {
        typed: &|t| matches!(t, "param" | "var"),
        named: &|t| matches!(t, "param" | "var"),
        is_name: &|w| w.starts_with('$'),
        type_len: Some(&|rest: &str| rest.find(' ').unwrap_or(rest.len())),
    };
    assert_eq!(
        parts("/** @var int The count */", &style),
        [("@var", DocText::Tag), ("int", DocText::Type)],
        "a description's word isn't a name"
    );
    let comment = "/** @param int|null $id the id, not an email@x.com */";
    assert_eq!(
        parts(comment, &style),
        [("@param", DocText::Tag), ("int|null", DocText::Type), ("$id", DocText::Name)]
    );
    assert!(parts("/** Just text with an @ in it */", &style).is_empty());
}
