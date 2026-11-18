#[test]
fn local_definitions_in_the_same_stylesheet() {
    let css = ":root { --accent: red; }\n.card { color: var(--accent); }\n.x { @extend .card; }";
    let intel = CssIntelligence::scss();
    let use_at = css.find("var(--accent)").map(|i| i + 6).unwrap_or(0);
    assert_eq!(intel.local_definition(css, use_at), css.find("--accent"));
    let card = css.rfind(".card").map(|i| i + 2).unwrap_or(0);
    assert_eq!(intel.local_definition(css, card), css.find(".card").map(|i| i + 1));
}

use super::*;

const SCSS: &str = "/* .comment { } */\n:root { --accent: #3574f0; }\n$gap: 8px;\n@mixin card-shadow($x) { box-shadow: 0 0 $x; }\n.card, .card--active > a[href='x.y'] { color: var(--accent); }\n#main .grid { gap: $gap; }\n";

#[test]
fn extracts_selectors_variables_and_mixins() {
    let file = extract(SCSS);
    let names: Vec<(&str, SymbolKind)> =
        file.symbols.iter().map(|s| (s.name.as_ref(), s.kind)).collect();
    assert!(names.contains(&("--accent", SymbolKind::CssVariable)));
    assert!(names.contains(&("$gap", SymbolKind::CssVariable)));
    assert!(names.contains(&("card-shadow", SymbolKind::Mixin)));
    assert!(names.contains(&("card", SymbolKind::CssClass)));
    assert!(names.contains(&("card--active", SymbolKind::CssClass)));
    assert!(names.contains(&("main", SymbolKind::CssId)));
    assert!(names.contains(&("grid", SymbolKind::CssClass)));
    assert!(
        !names.iter().any(|(n, _)| *n == "comment" || *n == "y" || *n == "3574f0"),
        "{names:?}"
    );
    let card = file.symbols.iter().find(|s| s.name == "card".into()).unwrap();
    assert_eq!((card.line, &SCSS[card.offset..card.offset + 4]), (4, "card"));
}

fn ctx(src: &str, scss: bool) -> ContextKind {
    let at = src.find('|').unwrap();
    context(&src.replace('|', ""), at, scss).kind
}

#[test]
fn contexts() {
    assert_eq!(ctx(".ca|", false), ContextKind::Symbols(vec![SymbolKind::CssClass]));
    assert_eq!(ctx("a { dis| }", false), ContextKind::Vocabulary("property"));
    assert_eq!(ctx("a { display: fl| }", false), ContextKind::Vocabulary("value"));
    assert_eq!(
        ctx("a { color: var(--ac|) }", false),
        ContextKind::Symbols(vec![SymbolKind::CssVariable])
    );
    assert_eq!(ctx("a { gap: $g| }", true), ContextKind::Symbols(vec![SymbolKind::CssVariable]));
    assert_eq!(ctx(".a { @include ca| }", true), ContextKind::Symbols(vec![SymbolKind::Mixin]));
    assert_eq!(ctx("<style>\n.ca|", false), ContextKind::Symbols(vec![SymbolKind::CssClass]));
    assert_eq!(ctx("/* .ca| */", false), ContextKind::None);
    assert_eq!(property_before("a { display: fl", 15), "display");
}

#[test]
fn values_for_the_property() {
    let intel = CssIntelligence::css();
    let text = "a { display: f }";
    let context = intel.context(text, 14);
    let values: Vec<String> =
        intel.vocabulary(&context, text).into_iter().map(|i| i.label).collect();
    assert!(values.contains(&"flex".to_owned()) && values.contains(&"inherit".to_owned()));
    assert_eq!(
        target("a { color: var(--acc|ent) }".replace('|', "").as_str(), 18),
        Some(Target::Named { name: "--accent".into(), kinds: vec![SymbolKind::CssVariable] })
    );
}

#[test]
fn rules_and_declarations_move_whole() {
    use code_intelligence::{LanguageIntelligence, StatementMove};
    let css = "a {\n  color: red;\n  margin: 0;\n}\n\nb {\n  padding: 0;\n}\n";
    let intel = CssIntelligence::css();
    assert_eq!(
        intel.statement_move(css, 2..3, true),
        StatementMove::Past { unit: 2..3, past: 1..2 }
    );
    assert_eq!(
        intel.statement_move(css, 5..6, true),
        StatementMove::Past { unit: 5..8, past: 0..4 }
    );
}

#[test]
fn sassdoc_completes_its_tags() {
    let intel = CssIntelligence::scss();
    let text = "/// @par\n@mixin m($a) {}";
    let context = intel.context(text, 8);
    assert_eq!(context.kind, ContextKind::Vocabulary(SASSDOC_TAG));
    let labels: Vec<String> =
        intel.vocabulary(&context, text).into_iter().map(|i| i.label).collect();
    assert!(labels.contains(&"@param".to_owned()), "{labels:?}");
    assert_eq!(intel.context("// @par", 7).kind, ContextKind::None, "a plain comment");
    assert_eq!(CssIntelligence::css().context("/// @par", 8).kind, ContextKind::None);
}
