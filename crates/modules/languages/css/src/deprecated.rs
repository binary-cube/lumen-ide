//! The deprecated CSS properties a stylesheet uses (flagged in `data/css.data`).

use code_intelligence::Deprecated;
use syntax_ts::tree_sitter::Parser;

pub fn find(text: &str) -> Vec<Deprecated> {
    let css = crate::data::get();
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_css::LANGUAGE.into()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else { return Vec::new() };
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "property_name" {
            let name = &text[node.byte_range()];
            if css.is_deprecated(&name.to_ascii_lowercase()) {
                out.push(Deprecated {
                    range: node.byte_range(),
                    kind: "property",
                    name: name.to_owned(),
                });
            }
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    out.sort_by_key(|d| d.range.start);
    out
}

#[cfg(test)]
#[path = "../tests/unit/deprecated.rs"]
mod tests;
