//! Go to definition inside a JavaScript file, by lexical scope: from the identifier under
//! the cursor, the enclosing scopes are searched outwards (block, function with its
//! parameters and hoisted `var`s, catch / for heads, the module with its imports) for the
//! nearest declaration of the same name — local variables, parameters and nested
//! functions included, like a good editor does. Names not declared in the file are left
//! to the project index.

use syntax_ts::tree_sitter::Node;

/// The byte offset of the declaration of the identifier at `offset`, if it is declared in
/// this file.
pub fn local_definition(root: Node<'_>, text: &str, offset: usize) -> Option<usize> {
    let node = identifier_at(root, offset)?;
    // `a.b`: `b` is a property, not a variable in scope.
    if node.kind() != "identifier" {
        return None;
    }
    let name = node.utf8_text(text.as_bytes()).ok()?;
    let mut scope = node.parent();
    while let Some(current) = scope {
        if let Some(found) = declared_in(current, name, text) {
            return Some(found);
        }
        scope = current.parent();
    }
    None
}

/// The identifier-like node at `offset` (also right after it, e.g. the cursor at the end).
fn identifier_at(root: Node<'_>, offset: usize) -> Option<Node<'_>> {
    let at = |o: usize| root.descendant_for_byte_range(o, o).filter(|n| is_name_node(n.kind()));
    at(offset).or_else(|| offset.checked_sub(1).and_then(at))
}

fn is_name_node(kind: &str) -> bool {
    matches!(kind, "identifier" | "property_identifier" | "shorthand_property_identifier")
}

/// The declaration of `name` that `scope` itself introduces (not its nested scopes).
fn declared_in(scope: Node<'_>, name: &str, text: &str) -> Option<usize> {
    let mut cursor = scope.walk();
    match scope.kind() {
        "program" | "statement_block" | "class_body" | "switch_body" => {
            for child in scope.named_children(&mut cursor) {
                if let Some(found) = declaration(child, name, text) {
                    return Some(found);
                }
            }
            None
        }
        "function_declaration"
        | "function_expression"
        | "function"
        | "arrow_function"
        | "method_definition"
        | "generator_function"
        | "generator_function_declaration" => {
            // Parameters (a single arrow parameter has no parentheses).
            for field in ["parameters", "parameter"] {
                if let Some(found) =
                    scope.child_by_field_name(field).and_then(|p| in_pattern(p, name, text))
                {
                    return Some(found);
                }
            }
            // A named function expression sees its own name.
            if scope.kind() != "function_declaration"
                && let Some(own) = scope.child_by_field_name("name")
                && own.utf8_text(text.as_bytes()).ok() == Some(name)
            {
                return Some(own.start_byte());
            }
            // `var` is hoisted to the function, from any nested block.
            scope.child_by_field_name("body").and_then(|body| hoisted_var(body, name, text))
        }
        "catch_clause" => {
            scope.child_by_field_name("parameter").and_then(|p| in_pattern(p, name, text))
        }
        // `for (let i = 0; …)`, `for (const x of xs)` (there the left side is the bare
        // binding, the keyword being a sibling).
        "for_statement" => {
            scope.child_by_field_name("initializer").and_then(|n| declaration(n, name, text))
        }
        "for_in_statement" => scope
            .child_by_field_name("left")
            .and_then(|n| declaration(n, name, text).or_else(|| in_pattern(n, name, text))),
        _ => None,
    }
}

/// The declaration of `name` made by statement `node`, if any.
fn declaration(node: Node<'_>, name: &str, text: &str) -> Option<usize> {
    let mut cursor = node.walk();
    match node.kind() {
        "lexical_declaration" | "variable_declaration" => {
            for declarator in node.named_children(&mut cursor) {
                if let Some(found) =
                    declarator.child_by_field_name("name").and_then(|n| in_pattern(n, name, text))
                {
                    return Some(found);
                }
            }
            None
        }
        "function_declaration" | "generator_function_declaration" | "class_declaration" => node
            .child_by_field_name("name")
            .filter(|n| n.utf8_text(text.as_bytes()).ok() == Some(name))
            .map(|n| n.start_byte()),
        "export_statement" => {
            node.child_by_field_name("declaration").and_then(|d| declaration(d, name, text))
        }
        "import_statement" => in_pattern(node, name, text),
        // `class A { field = … }`: fields are reached with `this.`, not as names.
        _ => None,
    }
}

/// `var` declarations of `name` anywhere in a function body, without entering nested
/// functions (they have their own scope).
fn hoisted_var(node: Node<'_>, name: &str, text: &str) -> Option<usize> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "variable_declaration" => {
                if let Some(found) = declaration(child, name, text) {
                    return Some(found);
                }
            }
            "function_declaration"
            | "function_expression"
            | "function"
            | "arrow_function"
            | "class_declaration"
            | "class"
            | "method_definition" => {}
            _ => {
                if let Some(found) = hoisted_var(child, name, text) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// The binding of `name` in a pattern: `a`, `{ a, b: c }`, `[a, ...rest]`, `a = 1`,
/// parameter lists, import clauses.
fn in_pattern(node: Node<'_>, name: &str, text: &str) -> Option<usize> {
    match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            (node.utf8_text(text.as_bytes()).ok() == Some(name)).then(|| node.start_byte())
        }
        // `{ key: value }`: only the value side binds a name.
        "pair_pattern" => node.child_by_field_name("value").and_then(|v| in_pattern(v, name, text)),
        // `a = 1`: the left side binds.
        "assignment_pattern" | "object_assignment_pattern" => {
            node.child_by_field_name("left").and_then(|l| in_pattern(l, name, text))
        }
        // `import { a as b }`: the alias (or the name) binds.
        "import_specifier" => node
            .child_by_field_name("alias")
            .or_else(|| node.child_by_field_name("name"))
            .and_then(|n| in_pattern(n, name, text)),
        // Strings (`from './x.js'`) and default values never bind.
        "string" | "number" => None,
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(found) = in_pattern(child, name, text) {
                    return Some(found);
                }
            }
            None
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/definition.rs"]
mod tests;
