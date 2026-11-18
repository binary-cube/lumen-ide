//! A node's children walked with a cursor. `Node::child(i)` counts from the first child each
//! time, so a loop over `0..child_count()` is quadratic on a wide node (a file's thousands
//! of top-level statements, an array literal of 10,000 items); these are linear, in the
//! same order.

use tree_sitter::Node;

/// `node`'s children, in order.
pub fn children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    let mut cursor = node.walk();
    let mut more = cursor.goto_first_child();
    std::iter::from_fn(move || {
        if !more {
            return None;
        }
        let child = cursor.node();
        more = cursor.goto_next_sibling();
        Some(child)
    })
}

/// `node`'s named children, in order.
pub fn named_children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    children(node).filter(Node::is_named)
}

/// `node`'s children pushed on a depth-first walk's `stack`, so they come off it in order.
pub fn push_children<'t>(stack: &mut Vec<Node<'t>>, node: Node<'t>) {
    let at = stack.len();
    stack.extend(children(node));
    stack[at..].reverse();
}

#[cfg(test)]
#[path = "../tests/unit/children.rs"]
mod tests;
