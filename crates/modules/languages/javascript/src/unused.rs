//! The imports a script never uses (`import X`, `import { a, b as c }`,
//! `import * as ns`): a statement whose names are all unused dims whole.

use std::collections::HashSet;

use code_intelligence::Unused;
use syntax_ts::tree_sitter::Node;

/// In `text`, parsed with `language` (the dialect's grammar: JavaScript, TypeScript, TSX).
pub fn find(tree: &syntax_ts::tree_sitter::Tree, text: &str) -> Vec<Unused> {
    let src = |n: Node<'_>| &text[n.byte_range()];
    let mut used: HashSet<&str> = HashSet::new();
    // (statement, its bindings: (the node to dim, the local name)).
    let mut imports: Vec<(Node<'_>, Vec<(Node<'_>, &str)>)> = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "import_statement" => {
                imports.push((node, bindings(node, text)));
                continue;
            }
            "identifier" | "type_identifier" | "shorthand_property_identifier" => {
                used.insert(src(node));
            }
            _ => {}
        }
        syntax_ts::push_children(&mut stack, node);
    }
    let mut out = Vec::new();
    for (statement, names) in imports {
        let unused: Vec<_> = names.iter().filter(|(_, name)| !used.contains(name)).collect();
        if !names.is_empty() && unused.len() == names.len() {
            let name = names.iter().map(|(_, n)| *n).collect::<Vec<_>>().join(", ");
            out.push(Unused { range: statement.byte_range(), kind: "import", name });
            continue;
        }
        for (node, name) in unused {
            out.push(Unused { range: node.byte_range(), kind: "import", name: (*name).to_owned() });
        }
    }
    out.sort_by_key(|u| u.range.start);
    out
}

/// The local names an import statement binds, with the node to dim for each.
fn bindings<'a>(statement: Node<'a>, text: &'a str) -> Vec<(Node<'a>, &'a str)> {
    let mut out = Vec::new();
    let mut stack = vec![statement];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "import_specifier" => {
                let local =
                    node.child_by_field_name("alias").or_else(|| node.child_by_field_name("name"));
                if let Some(local) = local {
                    out.push((node, &text[local.byte_range()]));
                }
                continue;
            }
            "namespace_import" => {
                if let Some(name) = syntax_ts::children(node).find(|c| c.kind() == "identifier") {
                    out.push((node, &text[name.byte_range()]));
                }
                continue;
            }
            // The default import: an identifier directly in the import clause.
            "identifier" if node.parent().is_some_and(|p| p.kind() == "import_clause") => {
                out.push((node, &text[node.byte_range()]));
                continue;
            }
            "string" => continue,
            _ => {}
        }
        stack.extend(syntax_ts::children(node));
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/unused.rs"]
mod tests;
