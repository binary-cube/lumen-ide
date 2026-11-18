use super::*;
use syntax_ts::tree_sitter::{Parser, Query};

#[test]
fn queries_compile_and_templates_parse() {
    for grammar in [grammar(), php_only_grammar()] {
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
    }
    let src = "@extends('layouts.app')\n@section('content')\n  <h1>{{ $title }}</h1>\n  @foreach ($users as $user)\n    <p>{!! $user->name !!}</p>\n  @endforeach\n@endsection\n";
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_blade::LANGUAGE.into()).expect("blade");
    let tree = parser.parse(src, None).expect("parsed");
    assert!(!tree.root_node().has_error(), "{}", tree.root_node().to_sexp());
}
