//! Highlighting for a zone (usually the viewport), from all layers.

use std::ops::Range;

use document::TextSnapshot;
use tree_sitter::{QueryCursor, StreamingIterator};

use crate::parse::{ScopeKey, SyntaxSnapshot};
use crate::text::SnapshotText;

/// A colored span. On overlap, the deeper layer wins, then the shorter
/// (more specific) span; on the same node, the first pattern in the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    pub depth: u8,
    pub pattern: u16,
    pub scope: ScopeKey,
}

/// Computes highlighting; reuses the cursor and buffer between frames.
#[derive(Default)]
pub struct Highlighter {
    cursor: QueryCursor,
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Highlighter")
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Self { cursor: QueryCursor::new() }
    }

    /// The colored spans in `range`, appended to `out` (cleared first), in the order in
    /// which they must be applied (later ones overwrite).
    pub fn highlight(
        &mut self,
        snapshot: &SyntaxSnapshot,
        text: &TextSnapshot,
        range: Range<usize>,
        out: &mut Vec<HighlightSpan>,
    ) {
        out.clear();
        self.collect(snapshot, text, range, out);
        sort_spans(out);
    }

    /// [`Self::highlight`] for several zones (each long row's drawn part, not the rows
    /// whole: a minified file's), as one list in the order they're applied; a span in
    /// several zones (a node over rows) once.
    pub fn highlight_ranges(
        &mut self,
        snapshot: &SyntaxSnapshot,
        text: &TextSnapshot,
        ranges: &[Range<usize>],
        out: &mut Vec<HighlightSpan>,
    ) {
        out.clear();
        for range in ranges {
            self.collect(snapshot, text, range.clone(), out);
        }
        if ranges.len() > 1 {
            out.sort_unstable_by_key(|s| {
                (s.range.start, s.range.end, s.depth, s.pattern, s.scope.capture)
            });
            out.dedup();
        }
        sort_spans(out);
    }

    /// The captures in `range`, appended to `out` in the query's order.
    fn collect(
        &mut self,
        snapshot: &SyntaxSnapshot,
        text: &TextSnapshot,
        range: Range<usize>,
        out: &mut Vec<HighlightSpan>,
    ) {
        for layer in &snapshot.layers {
            if !layer.intersects(&range) {
                continue;
            }
            let Some(query) = &layer.queries.highlights else { continue };
            let names = query.capture_names();
            self.cursor.set_byte_range(range.clone());
            let mut captures =
                self.cursor.captures(query, layer.tree.root_node(), SnapshotText(text));
            while let Some((m, index)) = captures.next() {
                let capture = m.captures()[*index];
                let name = names[capture.index as usize];
                // Helper captures (`_x`) and those without a color produce no spans.
                if name.starts_with('_') || name == "none" || name.starts_with("spell") {
                    continue;
                }
                out.push(HighlightSpan {
                    range: capture.node.byte_range(),
                    depth: layer.depth,
                    pattern: m.pattern_index as u16,
                    scope: ScopeKey { grammar: layer.grammar, capture: capture.index as u16 },
                });
                // A doc comment: its parts over it (shorter: they're applied after it).
                let node = capture.node;
                if let Some(doc) = &layer.queries.doc
                    && doc.nodes().contains(&node.kind())
                    && node.byte_range().len() <= crate::doc::LONGEST
                {
                    let start = node.start_byte();
                    let comment = text.slice(node.byte_range());
                    for (part_range, part) in doc.parts(&comment) {
                        out.push(HighlightSpan {
                            range: start + part_range.start..start + part_range.end,
                            depth: layer.depth,
                            pattern: 0,
                            scope: ScopeKey {
                                grammar: layer.grammar,
                                capture: crate::doc::capture(part),
                            },
                        });
                    }
                }
            }
        }
    }
}

/// Applied in order, the last one wins: top layers first, then the long spans, and at
/// equal length the later patterns (so the first one stays).
fn sort_spans(out: &mut [HighlightSpan]) {
    out.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then(b.range.len().cmp(&a.range.len()))
            .then(b.pattern.cmp(&a.pattern))
    });
}
