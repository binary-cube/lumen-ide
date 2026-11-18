use super::*;
use syntax_ts::tree_sitter::Parser;

fn found(src: &str) -> Vec<String> {
    let mut parser = Parser::new();
    let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
    let tree = parser.parse(src, None).expect("parsed");
    find(tree.root_node(), src).into_iter().map(|r| r.name).collect()
}

#[test]
fn extensions_used() {
    let src = "<?php\nnamespace App;\n\nuse http\\Env\\Request;\n\n$r = new Request();\n$c = new \\Redis();\n$n = bcadd('1', '2');\n$s = strlen('x');\n$local = new Response();\n";
    assert_eq!(found(src), ["ext-http", "ext-redis", "ext-bcmath"]);
}
