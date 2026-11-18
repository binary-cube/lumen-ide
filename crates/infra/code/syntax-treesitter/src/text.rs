//! Access to the document text for tree-sitter, without copying.

use document::TextSnapshot;
use tree_sitter::{Node, TextProvider};

/// Provides queries with a node's text directly from the rope's chunks.
pub(crate) struct SnapshotText<'a>(pub &'a TextSnapshot);

impl<'a> TextProvider<&'a [u8]> for SnapshotText<'a> {
    type I = Box<dyn Iterator<Item = &'a [u8]> + 'a>;

    fn text(&mut self, node: Node<'_>) -> Self::I {
        Box::new(self.0.chunks_in(node.byte_range()).map(str::as_bytes))
    }
}
