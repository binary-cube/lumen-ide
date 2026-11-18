use super::*;
use syntax_ts::tree_sitter::{Parser, Query};

#[test]
fn queries_compile_and_templates_parse() {
    let grammar = grammar();
    for (kind, source) in [
        ("highlights", &grammar.highlights),
        ("injections", &grammar.injections),
        ("folds", &grammar.folds),
    ] {
        if source.trim().is_empty() {
            continue;
        }
        if let Err(e) = Query::new(&grammar.ts, source) {
            panic!("{} {kind}: {e}", grammar.language);
        }
    }
    let src = "{% extends 'base.html.twig' %}\n{% block body %}\n  <h1>{{ title|upper }}</h1>\n  {% for user in users %}<p>{{ user.name }}</p>{% endfor %}\n{% endblock %}\n";
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_twig::LANGUAGE.into()).expect("twig");
    let tree = parser.parse(src, None).expect("parsed");
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}
