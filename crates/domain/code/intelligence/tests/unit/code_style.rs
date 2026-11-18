use super::*;

#[test]
fn styles_are_found_by_extension_else_plain() {
    register(CodeStyle {
        extensions: &["tstyle"],
        keywords: &["fn", "let"],
        sigil: Some('@'),
        comments: &["--"],
    });
    let style = for_extension("TSTYLE");
    assert!(style.is_keyword("LET") && !style.is_keyword("var"));
    assert_eq!(style.sigil, Some('@'));
    assert_eq!(for_extension("unknown-ext"), CodeStyle::default());
}
