use super::*;

/// PHP's manual writes tables: each row a line, its cells joined; paragraphs apart;
/// entities as characters; `array<int>` (no tag of HTML) as written.
#[test]
fn tables_paragraphs_and_entities() {
    let html = "<table><tbody><tr valign=\"top\"><td>Name</td><td>Value</td></tr><tr><td>INFO_GENERAL</td><td>1</td></tr></tbody></table><p>See &quot;php.ini&quot; &amp; more&#46;</p>Returns array<int>.";
    assert_eq!(
        to_text(html),
        "Name · Value\nINFO_GENERAL · 1\nSee \"php.ini\" & more.\nReturns array<int>."
    );
}

#[test]
fn lists_breaks_case_and_unknown_entities() {
    assert_eq!(to_text("<ul><li>one</li><li>two</li></ul>"), "• one\n• two\n");
    assert_eq!(to_text("a<br/>b<BR>c"), "a\nb\nc");
    assert_eq!(to_text("&unknown; & x < y"), "&unknown; & x < y");
    assert_eq!(to_text("<B>bold</B> é&#x41;"), "bold éA");
    assert_eq!(to_text("<?php echo 1; ?>"), "<?php echo 1; ?>", "not a tag of its rules");
}

/// Extensible: a tag of its own, a rule changed, another separator and mark.
#[test]
fn rules_can_be_added_and_changed() {
    let text = HtmlText::default()
        .with("note", Rule::Block)
        .with("span", Rule::Break)
        .with_cell_separator(" | ")
        .with_item_mark("- ");
    assert_eq!(text.rule("note"), Some(Rule::Block));
    assert_eq!(
        text.convert("a<note>b</note><span>c<table><tr><td>1</td><td>2</td></tr></table><li>x"),
        "a\nb\nc\n1 | 2\n- x"
    );
}
