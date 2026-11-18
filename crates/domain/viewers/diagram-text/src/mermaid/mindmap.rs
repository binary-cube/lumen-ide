//! `mindmap`: a tree by indentation, each node's shape by its brackets (`[square]`,
//! `(rounded)`, `((circle))`, `))bang((`, `)cloud(`, `{{hexagon}}`, plain text).
//! `::icon(…)` and `:::class` lines are left out.

use super::{Error, Line, label};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Shape {
    #[default]
    Plain,
    Square,
    Rounded,
    Circle,
    Bang,
    Cloud,
    Hexagon,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    pub text: String,
    pub shape: Shape,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Its depth (the root: 0).
    pub depth: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mindmap {
    pub nodes: Vec<Node>,
}

pub(super) fn parse(lines: &[Line<'_>]) -> Result<Mindmap, Error> {
    let mut m = Mindmap::default();
    // The nodes on the path to the line read, with their indentation.
    let mut path: Vec<(usize, usize)> = Vec::new();
    for line in lines {
        let raw = line.text.trim_end();
        let text = raw.trim_start();
        if text.is_empty() || text.starts_with("::icon") || text.starts_with(":::") {
            continue;
        }
        let indent = raw.len() - text.len();
        while path.last().is_some_and(|&(i, _)| i >= indent) {
            path.pop();
        }
        let parent = path.last().map(|&(_, n)| n);
        if parent.is_none() && !m.nodes.is_empty() {
            // A second root: Mermaid has one.
            return Err(Error::unexpected(line));
        }
        let text = text.split(":::").next().unwrap_or(text).trim();
        let (text, shape) = shape_of(text);
        let n = m.nodes.len();
        m.nodes.push(Node { text, shape, parent, children: Vec::new(), depth: path.len() });
        if let Some(p) = parent {
            m.nodes[p].children.push(n);
        }
        path.push((indent, n));
    }
    Ok(m)
}

/// A node's text and shape from how it's written (`id((text))`).
fn shape_of(s: &str) -> (String, Shape) {
    const SHAPES: &[(&str, &str, Shape)] = &[
        ("((", "))", Shape::Circle),
        ("))", "((", Shape::Bang),
        ("{{", "}}", Shape::Hexagon),
        ("[", "]", Shape::Square),
        ("(", ")", Shape::Rounded),
        (")", "(", Shape::Cloud),
    ];
    for (open, close, shape) in SHAPES {
        if let Some(i) = s.find(open)
            && let Some(inner) = s[i + open.len()..].strip_suffix(close)
        {
            return (label(inner), *shape);
        }
    }
    (label(s), Shape::Plain)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/mindmap.rs"]
mod tests;
