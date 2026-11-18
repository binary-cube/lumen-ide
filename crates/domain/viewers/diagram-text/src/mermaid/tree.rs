//! The types written as an indented tree: `kanban` (columns and their cards, `id[text]`,
//! `@{ … }` metadata) and `treemap-beta` (sections and leaves `"name": value`).

use super::{Error, Line, label, unquote};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeNode {
    pub text: String,
    pub value: Option<f64>,
    /// `@{ assigned: 'x', ticket: 1, priority: 'High' }`.
    pub meta: Vec<(String, String)>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

/// An indented tree (several roots allowed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tree {
    pub nodes: Vec<TreeNode>,
    pub roots: Vec<usize>,
}

pub type Kanban = Tree;
pub type Treemap = Tree;

pub(super) fn parse(lines: &[Line<'_>], values: bool) -> Result<Tree, Error> {
    let mut tree = Tree::default();
    let mut path: Vec<(usize, usize)> = Vec::new();
    for line in lines {
        let raw = line.text.trim_end();
        let text = raw.trim_start();
        if text.is_empty() {
            continue;
        }
        let indent = raw.len() - text.len();
        while path.last().is_some_and(|&(i, _)| i >= indent) {
            path.pop();
        }
        let parent = path.last().map(|&(_, n)| n);
        let (body, meta) = match text.split_once("@{") {
            Some((b, m)) => (b.trim(), metadata(m.trim_end_matches('}'))),
            None => (text, Vec::new()),
        };
        let (body, value) = if values {
            match body.rsplit_once(':') {
                Some((name, v)) if v.trim().parse::<f64>().is_ok() => {
                    (name.trim(), v.trim().parse().ok())
                }
                _ => (body, None),
            }
        } else {
            (body, None)
        };
        let body = body.split(":::").next().unwrap_or(body).trim();
        // `id[text]` or `[text]` or the text alone.
        let text = match body.find('[') {
            Some(i) if body.ends_with(']') => label(&body[i + 1..body.len() - 1]),
            _ => label(unquote(body)),
        };
        if text.is_empty() {
            return Err(Error::unexpected(line));
        }
        let n = tree.nodes.len();
        tree.nodes.push(TreeNode { text, value, meta, children: Vec::new(), parent });
        match parent {
            Some(p) => tree.nodes[p].children.push(n),
            None => tree.roots.push(n),
        }
        path.push((indent, n));
    }
    Ok(tree)
}

fn metadata(s: &str) -> Vec<(String, String)> {
    s.split(',')
        .filter_map(|p| p.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().trim_matches(['\'', '"']).to_owned()))
        .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/tree.rs"]
mod tests;
