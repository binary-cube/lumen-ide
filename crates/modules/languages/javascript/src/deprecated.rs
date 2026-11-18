//! The deprecated JavaScript / DOM APIs a script uses (flagged in `data/js.data`): global
//! functions (`escape()`) and members of the well-known objects (`document.execCommand`).

use code_intelligence::Deprecated;
use syntax_ts::tree_sitter::Node;

/// In `text`, parsed with `language` (the dialect's grammar: JavaScript, TypeScript, TSX).
pub fn find(tree: &syntax_ts::tree_sitter::Tree, text: &str) -> Vec<Deprecated> {
    let js = crate::data::get();
    let src = |n: Node<'_>| &text[n.byte_range()];
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "call_expression" => {
                if let Some(function) = node.child_by_field_name("function")
                    && function.kind() == "identifier"
                    && js.global_deprecated(src(function))
                {
                    out.push(Deprecated {
                        range: function.byte_range(),
                        kind: "api",
                        name: format!("{}()", src(function)),
                    });
                }
            }
            "member_expression" => {
                if let (Some(object), Some(property)) =
                    (node.child_by_field_name("object"), node.child_by_field_name("property"))
                    && object.kind() == "identifier"
                    && let Some(owner) = js.owner(src(object))
                    && js.member_deprecated(owner, src(property))
                {
                    out.push(Deprecated {
                        range: property.byte_range(),
                        kind: "api",
                        name: format!("{}.{}", src(object), src(property)),
                    });
                }
            }
            _ => {}
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
