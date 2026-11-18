//! Indentation from the tree: a grammar's `indents.scm` marks the nodes whose inner lines
//! go one level in (`@indent`: blocks, objects, lists, elements). A new line's indentation
//! is relative to the line where the innermost such node still open at it starts — so it
//! holds inside other languages (the PHP between HTML, a script in a page) — plus a level,
//! unless the line starts with what closes that node.

use std::collections::HashSet;

use document::TextSnapshot;
use tree_sitter::{Node, QueryCursor, StreamingIterator};

use crate::parse::SyntaxSnapshot;
use crate::text::SnapshotText;

/// Where a line's indentation comes from: the line of the innermost node still open there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentAnchor {
    /// The line where that node starts (its indentation is the base).
    pub line: usize,
}

/// The tokens that close a node: a line starting with one stays at the node's own level.
/// (`end`: Lua's blocks; `fi`, `done`, `esac`: the shell's.)
const CLOSERS: &[&str] =
    &["}", "]", ")", "end_tag", "erroneous_end_tag", "end", "fi", "done", "esac"];

impl SyntaxSnapshot {
    /// The anchor of a line beginning at `offset` (a new line typed there): the innermost
    /// `@indent` node that starts before it and is still open at it, in the deepest
    /// language there that has indentation rules. `None`: none (the previous line's
    /// indentation, as without a tree).
    pub fn indent_anchor(&self, text: &TextSnapshot, offset: usize) -> Option<IndentAnchor> {
        let mut layers: Vec<&crate::parse::Layer> =
            self.layers.iter().filter(|l| l.contains(offset)).collect();
        layers.sort_by_key(|l| std::cmp::Reverse(l.depth));
        for layer in layers {
            let Some(query) = &layer.queries.indents else { continue };
            let Some(index) = query.capture_index_for_name("indent") else { continue };
            // The marked nodes around `offset`.
            let mut marked = HashSet::new();
            let mut cursor = QueryCursor::new();
            cursor.set_byte_range(offset.saturating_sub(1)..offset + 1);
            let root = layer.tree.root_node();
            let mut captures = cursor.captures(query, root, SnapshotText(text));
            while let Some((m, i)) = captures.next() {
                let capture = m.captures()[*i];
                if capture.index == index {
                    marked.insert(capture.node.id());
                }
            }
            if marked.is_empty() {
                continue;
            }
            let mut node = root.descendant_for_byte_range(offset, offset);
            while let Some(n) = node {
                if marked.contains(&n.id()) && n.start_byte() < offset && open_at(n, offset) {
                    return Some(IndentAnchor { line: text.line_of(n.start_byte()) });
                }
                node = n.parent();
            }
        }
        None
    }
}

/// `node` is still open at `offset`: it ends after it, or ends there without its closer
/// (one not typed yet: `function a() {` + Enter).
fn open_at(node: Node<'_>, offset: usize) -> bool {
    if node.end_byte() > offset {
        return true;
    }
    let last = node.child(node.child_count().saturating_sub(1));
    !last.is_some_and(|l| CLOSERS.contains(&l.kind()) && !l.is_missing())
}

#[cfg(test)]
#[path = "../tests/unit/indent.rs"]
mod tests;
