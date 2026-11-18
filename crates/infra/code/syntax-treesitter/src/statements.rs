//! "Move Statement" on a syntax tree, for every language alike: the unit the selected lines
//! are in — the nearest node of a kind the language calls a unit (its statements,
//! declarations, members, elements, rules), with the comments right above it — and the
//! sibling unit it moves past. A unit doesn't leave its block: the first / last one stays.
//! Each language module only says which kinds are units.

use std::ops::Range;

use tree_sitter::{Node, Tree};

pub use code_intelligence::StatementMove as Moved;

fn is_comment(node: &Node<'_>) -> bool {
    node.kind().contains("comment")
}

/// The node's lines.
fn rows(node: &Node<'_>) -> Range<usize> {
    let end = node.end_position();
    // A node ending at a line's start (its newline included) doesn't take that line.
    let last =
        if end.column == 0 && end.row > node.start_position().row { end.row - 1 } else { end.row };
    node.start_position().row..last + 1
}

/// What moves when `lines` of `text` (parsed into `tree`) go up / down; `units`: the node
/// kinds moved whole.
pub fn statement_move(
    tree: &Tree,
    text: &str,
    lines: Range<usize>,
    up: bool,
    units: &[&str],
) -> Moved {
    // The selected lines' text, without the indentation and the trailing blanks.
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let (Some(&from), Some(&to_line)) =
        (starts.get(lines.start), starts.get(lines.end.saturating_sub(1)))
    else {
        return Moved::Lines;
    };
    let to = text[to_line..].find('\n').map_or(text.len(), |i| to_line + i);
    let content = &text[from..to.max(from)];
    let lead = content.len() - content.trim_start().len();
    let trail = content.len() - content.trim_end().len();
    if content.trim().is_empty() {
        return Moved::Lines;
    }
    let (start, end) = (from + lead, to - trail);
    let Some(mut node) = tree.root_node().descendant_for_byte_range(start, end) else {
        return Moved::Lines;
    };
    // The nearest unit around the lines.
    while !units.contains(&node.kind()) {
        let Some(parent) = node.parent() else { return Moved::Lines };
        node = parent;
    }
    let Some(parent) = node.parent() else { return Moved::Lines };
    // The parent's children as groups: a unit with the comments right above it; a comment
    // standing alone is a group too; anything else is a wall (a tag, a name, `<?php`).
    let mut groups: Vec<(Range<usize>, bool)> = Vec::new();
    let mut pending: Option<Range<usize>> = None;
    let mut cursor = parent.walk();
    let mut mine = None;
    for child in parent.named_children(&mut cursor) {
        let span = rows(&child);
        if is_comment(&child) {
            // Comments next to each other stay together.
            pending = Some(match pending {
                Some(p) if span.start <= p.end => p.start..span.end,
                Some(p) => {
                    groups.push((p, true));
                    span
                }
                None => span,
            });
            continue;
        }
        let movable = units.contains(&child.kind());
        let span = match pending.take() {
            // Its doc comment, right above it.
            Some(p) if movable && span.start <= p.end => p.start..span.end,
            Some(p) => {
                groups.push((p, true));
                span
            }
            None => span,
        };
        if child.id() == node.id() {
            mine = Some(groups.len());
        }
        groups.push((span, movable));
    }
    if let Some(p) = pending {
        groups.push((p, true));
    }
    let Some(i) = mine else { return Moved::Lines };
    let unit = groups[i].0.clone();
    let neighbour = if up { i.checked_sub(1) } else { Some(i + 1) };
    match neighbour.and_then(|n| groups.get(n)) {
        // Sharing a line with it (`a(); b();`): the lines move alone.
        Some((past, true)) if past.end <= unit.start || unit.end <= past.start => {
            Moved::Past { unit, past: past.clone() }
        }
        Some((_, true)) => Moved::Lines,
        _ => Moved::Stay,
    }
}

#[cfg(test)]
#[path = "../tests/unit/statements.rs"]
mod tests;
