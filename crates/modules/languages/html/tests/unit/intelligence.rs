use super::*;

fn ctx(src: &str) -> (ContextKind, String) {
    let at = src.find('|').unwrap();
    let c = context(&src.replace('|', ""), at);
    (c.kind, c.prefix)
}

#[test]
fn contexts() {
    assert_eq!(ctx("<di|").0, ContextKind::Vocabulary("tag"));
    assert_eq!(ctx("<div cl|").0, ContextKind::Vocabulary("attribute"));
    assert_eq!(
        ctx("<div class=\"card ac|\">"),
        (ContextKind::Symbols(vec![SymbolKind::CssClass]), "ac".into())
    );
    assert_eq!(ctx("<div id='ma|'>").0, ContextKind::Symbols(vec![SymbolKind::CssId]));
    assert_eq!(ctx("<input type=\"em|\">").0, ContextKind::Vocabulary("value"));
    assert_eq!(ctx("<p>text| </p>").0, ContextKind::None);
    assert_eq!(
        ctx("<?php if ($a) { ?><div class=\"x|\"").0,
        ContextKind::Symbols(vec![SymbolKind::CssClass])
    );
}

#[test]
fn closing_tags() {
    let close = |s: &str| closing_tag(s, s.len());
    assert_eq!(close("<div class=\"card\">"), Some("</div>".into()));
    assert_eq!(close("<br>"), None);
    assert_eq!(close("<img src=\"a.png\" />"), None);
    assert_eq!(close("</div>"), None);
    assert_eq!(close("<!doctype html>"), None);
    assert_eq!(close("<?php echo 1 ?>"), None);
}

#[test]
fn attributes_for_the_tag_and_class_targets() {
    let intel = HtmlIntelligence;
    let text = "<input ty";
    let context = intel.context(text, text.len());
    let items = intel.vocabulary(&context, text);
    let ty = items.iter().find(|i| i.label == "type").unwrap();
    assert_eq!((ty.insert.as_str(), ty.cursor_back), ("type=\"\"", 1));
    assert!(items.iter().any(|i| i.label == "required" && i.insert == "required"));
    let src = "<div class=\"card ac|tive\">";
    let at = src.find('|').unwrap();
    assert_eq!(
        target(&src.replace('|', ""), at),
        Some(Target::Named { name: "active".into(), kinds: vec![SymbolKind::CssClass] })
    );
}

#[test]
fn elements_move_whole_within_their_parent() {
    use code_intelligence::{LanguageIntelligence, StatementMove};
    let html = "<ul>\n  <li>a</li>\n  <li>\n    b\n  </li>\n</ul>\n";
    let intel = HtmlIntelligence;
    assert_eq!(
        intel.statement_move(html, 2..3, true),
        StatementMove::Past { unit: 2..5, past: 1..2 }
    );
    assert_eq!(intel.statement_move(html, 1..2, true), StatementMove::Stay, "the first in <ul>");
}

/// A hover past the end or inside a multi-byte character doesn't panic.
#[test]
fn offsets_outside_characters_are_safe() {
    let src = "<div class=\"ș\">";
    for at in 0..src.len() + 3 {
        let _ = target(src, at);
        let _ = open_tag(src, at);
        let _ = context(src, at);
    }
}

/// What's commented out leads nowhere: a class in `<!-- … -->` is text (plan 75).
#[test]
fn a_comment_is_text() {
    use code_intelligence::CommentPlace;
    let html = HtmlIntelligence;
    let text = "<!-- <div class=\"btn\"> -->\n<div class=\"card\"></div>";
    let place = |needle: &str| html.comment_place(text, text.find(needle).unwrap() + 1);
    assert_eq!(place("btn"), Some(CommentPlace::Text));
    assert_eq!(place("card"), None, "after it closed");
}
