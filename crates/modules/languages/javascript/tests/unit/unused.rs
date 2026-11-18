/// `text` parsed as JavaScript.
fn parsed(text: &str) -> syntax_ts::tree_sitter::Tree {
    let mut parser = syntax_ts::tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    parser.parse(text, None).unwrap()
}

#[test]
fn unused_imports() {
    let src = "import React, { useState, useMemo as memo } from 'react';\nimport * as utils from './utils';\nimport Gone from './gone';\n\nexport function App() {\n  const [n] = useState(0);\n  return <div>{utils.x(n)}</div>;\n}\n";
    let found: Vec<_> = super::find(&parsed(src), src).into_iter().map(|u| u.name).collect();
    assert_eq!(found, ["React", "memo", "Gone"]);
}
