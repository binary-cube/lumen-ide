//! "Extend Selection" on a syntax tree, for every language alike: the ranges around a
//! selection, from the smallest to the whole document — each node holding it, and before a
//! node between delimiters (`(…)`, `[…]`, `{…}`, quotes) what's inside them.

use std::ops::Range;

use tree_sitter::{Node, Tree};

/// The ranges of `tree` that hold `range`, the smallest first, each bigger than the one
/// before (none equal to `range` itself).
pub fn selection_ranges(tree: &Tree, range: Range<usize>) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let Some(mut node) = tree.root_node().descendant_for_byte_range(range.start, range.end) else {
        return out;
    };
    loop {
        if let Some(inner) = inside_delimiters(&node) {
            push(&mut out, inner, &range);
        }
        push(&mut out, node.byte_range(), &range);
        match node.parent() {
            Some(parent) => node = parent,
            None => break,
        }
    }
    out
}

/// What's between a node's opening and closing delimiters (its first and last children,
/// both unnamed and a matching pair), when something is.
fn inside_delimiters(node: &Node<'_>) -> Option<Range<usize>> {
    let count = node.child_count();
    if count < 3 {
        return None;
    }
    let (first, last) = (node.child(0)?, node.child(count - 1)?);
    if first.is_named() || last.is_named() {
        return None;
    }
    let pair = matches!(
        (first.kind(), last.kind()),
        ("(", ")") | ("[", "]") | ("{", "}") | ("\"", "\"") | ("'", "'") | ("`", "`")
    );
    if !pair {
        return None;
    }
    let (from, to) = (node.child(1)?, node.child(count - 2)?);
    (from.start_byte() < to.end_byte()).then(|| from.start_byte()..to.end_byte())
}

/// Adds `candidate` if it holds `range`, is bigger than it and than the last one added.
fn push(out: &mut Vec<Range<usize>>, candidate: Range<usize>, range: &Range<usize>) {
    let holds = candidate.start <= range.start && range.end <= candidate.end;
    let bigger = candidate.len() > range.len();
    let grows = out.last().is_none_or(|last| {
        candidate.start <= last.start && last.end <= candidate.end && candidate != *last
    });
    if holds && bigger && grows {
        out.push(candidate);
    }
}

#[cfg(test)]
#[path = "../tests/unit/selection.rs"]
mod tests;
