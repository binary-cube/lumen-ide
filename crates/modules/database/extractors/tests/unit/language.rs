use super::*;
use code_intelligence::DocumentAssist;
use syntax_ts::tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

#[test]
fn the_highlights_compile_and_every_tag_is_colored() {
    let grammar = grammar();
    let query = Query::new(&grammar.ts, &grammar.highlights).expect("the highlights compile");
    let mut parser = Parser::new();
    parser.set_language(&grammar.ts).unwrap();
    for (name, text) in crate::examples::EXAMPLES {
        let tree = parser.parse(text, None).unwrap();
        let mut cursor = QueryCursor::new();
        let mut tags = 0;
        assert!(!tree.root_node().to_sexp().contains("(comment)") || text.contains("{#"), "{name}");
        let names = query.capture_names();
        let mut captures = cursor.captures(&query, tree.root_node(), text.as_bytes());
        while let Some((m, i)) = captures.next() {
            let capture = m.captures()[*i];
            if names[capture.index as usize] == "tag.delimiter"
                && &text[capture.node.byte_range()] == "{{"
            {
                tags += 1;
            }
        }
        // Every tag read as one of the grammar's (its colors stay in it), nearly all with both
        // delimiters colored (a section's may be in the grammar's error: a keyword whole).
        let written = text.matches("{{").count();
        let directives = tree.root_node().to_sexp().matches("(output_directive").count();
        assert_eq!(directives, written, "{name}: every tag read as one");
        assert!(tags * 4 >= written * 3, "{name}: {tags} of {written} tags colored");
    }
}

#[test]
fn completion_comes_from_the_template_language() {
    let assist = TemplateAssist;
    let text = "{{#rows}}{{value|j";
    let at = AssistContext {
        project: None,
        root: None,
        path: std::path::Path::new("/x/a.extractor"),
        language: "extractor",
        text,
        offset: text.len(),
        version: 1,
    };
    let done = assist.complete(&at).unwrap();
    assert!(!done.merge);
    assert_eq!(done.replace, text.len() - 1..text.len());
    assert_eq!(done.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["json"]);
    assert_eq!(done.items[0].kind, CompletionKind::Function);
    let outside = AssistContext { text: "plain", offset: 5, ..at };
    assert!(assist.complete(&outside).is_none());
    assert!(TemplateIntelligence.completion_triggers().contains('|'));
    assert_eq!(descriptor().matchers, vec![languages::FileMatcher::Extension("extractor")]);
}
