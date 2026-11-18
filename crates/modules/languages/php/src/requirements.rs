//! The PHP extensions a file uses (classes, functions, constants they provide), so the
//! app can point out those missing from `composer.json` (`ext-http`, `ext-redis`…).
//! Names are resolved like PHP does: through the `use` imports, the current namespace
//! for classes, the global fallback for functions.

use std::collections::{HashMap, HashSet};

use code_intelligence::Requirement;
use syntax_ts::tree_sitter::Node;

pub fn find(root: Node<'_>, text: &str) -> Vec<Requirement> {
    let stubs = crate::stubs::get();
    let src = |n: Node<'_>| &text[n.byte_range()];
    let mut imports: HashMap<String, String> = HashMap::new();
    let mut namespace = false;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "namespace_definition" => namespace = true,
            "namespace_use_clause" => {
                let full = src(node).trim();
                let (path, alias) = match full.split_once(" as ") {
                    Some((p, a)) => (p.trim(), a.trim()),
                    None => (full, full.rsplit('\\').next().unwrap_or(full)),
                };
                imports
                    .insert(alias.to_ascii_lowercase(), path.trim_start_matches('\\').to_owned());
                continue;
            }
            "object_creation_expression"
            | "scoped_call_expression"
            | "class_constant_access_expression"
            | "named_type"
            | "instanceof_expression" => {
                if let Some(name) = class_name(node) {
                    let written = src(name);
                    let resolved = if let Some(global) = written.strip_prefix('\\') {
                        Some(global.to_owned())
                    } else {
                        let first =
                            written.split('\\').next().unwrap_or(written).to_ascii_lowercase();
                        match imports.get(&first) {
                            Some(path) => Some(match written.split_once('\\') {
                                Some((_, rest)) => format!("{path}\\{rest}"),
                                None => path.clone(),
                            }),
                            // Unqualified, not imported: the current namespace's (unless none).
                            None => {
                                (!namespace || written.contains('\\')).then(|| written.to_owned())
                            }
                        }
                    };
                    if let Some(ext) = resolved.and_then(|r| stubs.extension(&r)) {
                        push(&mut out, &mut seen, name, ext);
                    }
                }
            }
            "function_call_expression" => {
                if let Some(function) = node.child_by_field_name("function")
                    && matches!(function.kind(), "name" | "qualified_name")
                    && let Some(ext) = stubs.extension(src(function).trim_start_matches('\\'))
                {
                    push(&mut out, &mut seen, function, ext);
                }
            }
            _ => {}
        }
        syntax_ts::push_children(&mut stack, node);
    }
    out
}

/// The class named by an expression or a type (`new X`, `X::y()`, `X $x`).
fn class_name(node: Node<'_>) -> Option<Node<'_>> {
    let candidate = match node.kind() {
        "named_type" => node.child(0),
        "instanceof_expression" => node.child_by_field_name("right"),
        "scoped_call_expression" | "class_constant_access_expression" => {
            node.child_by_field_name("scope").or_else(|| node.child(0))
        }
        _ => syntax_ts::children(node).find(|c| matches!(c.kind(), "name" | "qualified_name")),
    }?;
    matches!(candidate.kind(), "name" | "qualified_name").then_some(candidate)
}

/// One requirement per extension (at its first use).
fn push(
    out: &mut Vec<Requirement>,
    seen: &mut HashSet<&'static str>,
    node: Node<'_>,
    ext: &'static str,
) {
    if seen.insert(ext) {
        out.push(Requirement { range: node.byte_range(), name: format!("ext-{ext}") });
    }
}

#[cfg(test)]
#[path = "../tests/unit/requirements.rs"]
mod tests;
