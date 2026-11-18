//! Where a local variable is declared: a parameter of its function (or
//! of the closure / arrow function it's in), a closure's `use ($x)`, else the first place its
//! scope binds it — an assignment (`$x = …`, `[$a, $b] = …`, `list(…) = …`), a `foreach`'s
//! key or value, a `catch (T $e)`, `global $x`, `static $x`.

use syntax_ts::tree_sitter::Node;

use crate::infer::named_children;

/// The function-like nodes that start a scope of their own.
fn is_scope(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition" | "method_declaration" | "anonymous_function" | "arrow_function"
    )
}

/// What isn't part of the scope it's written in (its variables are its own).
fn is_nested(kind: &str) -> bool {
    is_scope(kind)
        || matches!(
            kind,
            "class_declaration"
                | "trait_declaration"
                | "interface_declaration"
                | "enum_declaration"
        )
}

/// The declaration of the variable at `offset` of `text` (the byte its `$` is at); `None`
/// when the cursor isn't on a variable, on `$this`, or nothing in its scope binds it.
pub(crate) fn definition(text: &str, offset: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let is_name = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80;
    // On the `$` itself: the name after it.
    let at = if bytes.get(offset) == Some(&b'$') { offset + 1 } else { offset };
    let mut start = at;
    while start > 0 && is_name(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < bytes.len() && is_name(bytes[end]) {
        end += 1;
    }
    if start == end || start == 0 || bytes[start - 1] != b'$' {
        return None;
    }
    let dollar = start - 1;
    let name = &text[dollar..end];
    // `$this`, `self::$x` (a static property), `$$x`.
    let before = text[..dollar].trim_end();
    if name == "$this" || before.ends_with("::") || before.ends_with('$') {
        return None;
    }
    let parsed = crate::doctree::parsed(text)?;
    let root = parsed.tree.root_node();
    let node = root.descendant_for_byte_range(dollar, end)?;
    let variable =
        std::iter::successors(Some(node), |n| n.parent()).find(|n| n.kind() == "variable_name")?;
    declared_in_scope(text, variable, name)
}

/// The text of `node`.
fn src<'a>(text: &'a str, node: Node<'_>) -> &'a str {
    text.get(node.byte_range()).unwrap_or_default()
}

/// Where `name` (with its `$`) is declared for the variable `at`: up through its scopes.
fn declared_in_scope(text: &str, at: Node<'_>, name: &str) -> Option<usize> {
    let scope = std::iter::successors(at.parent(), |n| n.parent()).find(|n| is_scope(n.kind()));
    let Some(scope) = scope else {
        let root = std::iter::successors(Some(at), |n| n.parent()).last()?;
        return first_binding(text, root, name);
    };
    // A parameter: the signature's.
    let parameter = scope.child_by_field_name("parameters").and_then(|ps| {
        named_children(ps).find_map(|p| {
            let n = p.child_by_field_name("name")?;
            (src(text, n) == name).then(|| n.start_byte())
        })
    });
    if let Some(at) = parameter {
        return Some(at);
    }
    match scope.kind() {
        // An arrow function sees the scope it's written in.
        "arrow_function" => declared_in_scope(text, scope, name),
        "anonymous_function" => {
            let used = named_children(scope)
                .filter(|c| c.kind() == "anonymous_function_use_clause")
                .flat_map(named_children)
                .find_map(|v| {
                    let v = if v.kind() == "variable_name" {
                        v
                    } else {
                        named_children(v).find(|c| c.kind() == "variable_name")?
                    };
                    (src(text, v) == name).then(|| v.start_byte())
                });
            used.or_else(|| first_binding(text, scope.child_by_field_name("body")?, name))
        }
        _ => first_binding(text, scope.child_by_field_name("body")?, name),
    }
}

/// The first place `body` binds `name` (its nested scopes left out), in the text's order.
fn first_binding(text: &str, body: Node<'_>, name: &str) -> Option<usize> {
    let mut stack: Vec<Node<'_>> = named_children(body).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if is_nested(node.kind()) {
            continue;
        }
        if node.kind() == "variable_name" && src(text, node) == name && binds(node) {
            return Some(node.start_byte());
        }
        let mut children: Vec<Node<'_>> = named_children(node).collect();
        children.reverse();
        stack.extend(children);
    }
    None
}

/// Whether the variable `node` is given a value where it stands.
fn binds(node: Node<'_>) -> bool {
    let mut child = node;
    for parent in std::iter::successors(node.parent(), |n| n.parent()) {
        match parent.kind() {
            "assignment_expression" | "reference_assignment_expression" => {
                // The left side: the variable itself, or inside a `[…]` / `list(…)`.
                return parent.child_by_field_name("left").is_some_and(|l| l.id() == child.id());
            }
            "list_literal"
            | "array_creation_expression"
            | "array_element_initializer"
            | "pair"
            | "by_ref" => {}
            "foreach_statement" => {
                // After the iterated value, before the body.
                let iterable = parent.named_child(0);
                let body = parent.child_by_field_name("body");
                return iterable
                    .is_some_and(|i| i.id() != child.id() && node.start_byte() >= i.end_byte())
                    && body.is_none_or(|b| node.end_byte() <= b.start_byte());
            }
            "catch_clause" => {
                return parent.child_by_field_name("name").is_some_and(|n| n.id() == node.id());
            }
            "global_declaration"
            | "function_static_declaration"
            | "static_variable_declaration" => {
                return true;
            }
            _ => return false,
        }
        child = parent;
    }
    false
}

#[cfg(test)]
#[path = "../tests/unit/locals.rs"]
mod tests;
