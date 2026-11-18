//! The deprecated HTML elements and attributes a page uses (flagged in `data/html.data`).

use code_intelligence::Deprecated;
use syntax_ts::tree_sitter::{Node, Parser};

pub fn find(text: &str) -> Vec<Deprecated> {
    let html = crate::data::get();
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_html::LANGUAGE.into()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else { return Vec::new() };
    let src = |n: Node<'_>| &text[n.byte_range()];
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "start_tag" | "self_closing_tag") {
            let mut cursor = node.walk();
            let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
            let Some(tag) = children.iter().find(|c| c.kind() == "tag_name") else { continue };
            let name = src(*tag).to_ascii_lowercase();
            if html.element_deprecated(&name) {
                out.push(Deprecated {
                    range: tag.byte_range(),
                    kind: "element",
                    name: format!("<{name}>"),
                });
            }
            for attribute in children.iter().filter(|c| c.kind() == "attribute") {
                let mut c = attribute.walk();
                let Some(attr) =
                    attribute.named_children(&mut c).find(|n| n.kind() == "attribute_name")
                else {
                    continue;
                };
                let attr_name = src(attr).to_ascii_lowercase();
                if html.attribute_deprecated(&name, &attr_name) {
                    out.push(Deprecated {
                        range: attr.byte_range(),
                        kind: "attribute",
                        name: attr_name,
                    });
                }
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
