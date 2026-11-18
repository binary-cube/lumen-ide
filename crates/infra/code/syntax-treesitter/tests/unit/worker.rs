use super::*;
use crate::grammar::{Grammar, Grammars};
use document::{EditKind, Transaction};

fn config() -> Arc<SyntaxConfig> {
    let mut grammars = Grammars::default();
    grammars.register(Grammar {
        language: LanguageId::new_static("javascript"),
        ts: tree_sitter_javascript::LANGUAGE.into(),
        highlights: "(identifier) @variable".into(),
        injections: "".into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    });
    Arc::new(SyntaxConfig::new(grammars))
}

/// A result taken (and brought up to the document) between a key and the next paint: the
/// paint's sync doesn't apply the same changes to it again (its colors would shift by what
/// was typed).
#[test]
fn a_result_brought_up_to_date_is_not_shifted_again() {
    let js = LanguageId::new_static("javascript");
    let config = config();
    let worker = SyntaxWorker::spawn(Arc::clone(&config), Arc::new(|| {})).unwrap();
    let mut doc = Document::new("let a = 1;");
    let first =
        parse_document(&config, &js, &doc.buffer().snapshot(), None, &[], doc.version()).unwrap();
    let mut syntax = DocumentSyntax::new(js);
    // v0 sent; then a key: v1.
    syntax.sync(&doc, &worker);
    doc.apply(Transaction::single(0..0, "xx"), None, EditKind::Typing);
    // The v0 result arrives (a wake before the paint): brought up to v1.
    syntax.accept(first, &doc);
    let end = |s: &DocumentSyntax| s.snapshot().unwrap().layers[0].tree.root_node().end_byte();
    assert_eq!(end(&syntax), doc.buffer().len());
    // The paint's sync: v1 sent, the snapshot left as it is.
    syntax.sync(&doc, &worker);
    assert_eq!(end(&syntax), doc.buffer().len(), "not shifted twice");
    assert_eq!(syntax.snapshot().unwrap().version(), doc.version());
}
