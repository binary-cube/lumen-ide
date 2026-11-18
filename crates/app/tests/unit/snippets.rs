use std::sync::Arc;

use extensions::ModuleSelection;
use ui_workbench::snippets::SnippetHighlighter;

use super::*;

fn snippets() -> Snippets {
    let (support, _) = crate::modules::load(&ModuleSelection::default());
    Snippets { support: Arc::new(support) }
}

#[test]
fn snippets_are_highlighted_with_the_grammars() {
    let s = snippets();
    // PHP without its opening tag: read as PHP all the same, the ranges in the snippet.
    let code = "$total = count($items); // sum";
    let spans = s.highlight("php", code).expect("php has a grammar");
    let named = |text: &str| {
        spans
            .iter()
            .filter(|(r, _)| &code[r.clone()] == text)
            .map(|(_, n)| n.to_string())
            .collect::<Vec<_>>()
    };
    assert!(named("$total").iter().any(|n| n.starts_with("variable")), "{spans:?}");
    assert!(named("// sum").iter().any(|n| n.starts_with("comment")), "{spans:?}");
    assert!(s.highlight("json", "{\"a\": 1}").is_some());
    assert!(s.highlight("no-such-language", "x").is_none());
}
