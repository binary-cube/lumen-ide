//! `block-beta` (or `block`): blocks on a grid (`columns 3`), each spanning columns
//! (`a:2`), in their shapes (as a flowchart's), nested blocks (`block:id … end`), gaps
//! (`space`, `space:2`) and arrows between blocks.

use super::flowchart::Shape;
use super::{Error, ErrorKind, Line, label};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Block {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    pub span: u32,
    /// A gap (`space`): drawn as nothing.
    pub space: bool,
    /// What a nested block holds, and its columns.
    pub children: Vec<usize>,
    pub columns: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockDiagram {
    pub blocks: Vec<Block>,
    /// The top level's blocks and columns.
    pub top: Vec<usize>,
    pub columns: Option<u32>,
    pub edges: Vec<(usize, usize, Option<String>)>,
}

/// A block's shape and label from how it's written (`a["text"]`, `b(("c"))`).
fn shaped(s: &str) -> (String, String, Shape) {
    const SHAPES: &[(&str, &str, Shape)] = &[
        ("((", "))", Shape::Circle),
        ("([", "])", Shape::Stadium),
        ("[[", "]]", Shape::Subroutine),
        ("[(", ")]", Shape::Cylinder),
        ("{{", "}}", Shape::Hexagon),
        ("[/", "/]", Shape::Parallelogram),
        ("(", ")", Shape::Round),
        ("[", "]", Shape::Rect),
        ("{", "}", Shape::Diamond),
        (">", "]", Shape::Asymmetric),
    ];
    for (open, close, shape) in SHAPES {
        if let Some(i) = s.find(open)
            && let Some(inner) = s[i + open.len()..].strip_suffix(close)
        {
            return (s[..i].to_owned(), label(inner), *shape);
        }
    }
    (s.to_owned(), s.to_owned(), Shape::Rect)
}

/// A line's tokens: blocks (with their spans), arrows between them.
fn tokens(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quoted = false;
    for c in t.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '[' | '(' | '{' if !quoted => {
                depth += 1;
                cur.push(c);
            }
            ']' | ')' | '}' if !quoted => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub(super) fn parse(lines: &[Line<'_>]) -> Result<BlockDiagram, Error> {
    let mut d = BlockDiagram::default();
    // The nested blocks open (their index), innermost last.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let find = |d: &BlockDiagram, id: &str| d.blocks.iter().position(|b| b.id == id && !b.space);
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if t == "end" {
            if open.pop().is_none() {
                return Err(Error::unexpected(line));
            }
            continue;
        }
        if let Some(n) = t.strip_prefix("columns ") {
            let n = n.trim().parse().ok();
            match open.last() {
                Some(&(b, _)) => d.blocks[b].columns = n,
                None => d.columns = n,
            }
            continue;
        }
        if t.starts_with("classDef") || t.starts_with("class ") || t.starts_with("style ") {
            continue;
        }
        let toks = tokens(t);
        let mut i = 0;
        let mut last: Option<usize> = None;
        while i < toks.len() {
            let tok = &toks[i];
            // An arrow: `-->`, `---`, with an optional `|label|`.
            if tok.starts_with("--") || tok.starts_with("==") || tok.starts_with("-.") {
                let mut text = None;
                let mut j = i + 1;
                if toks.get(j).is_some_and(|t| t.starts_with('|')) {
                    text = Some(toks[j].trim_matches('|').to_owned());
                    j += 1;
                } else if toks.get(j).is_some_and(|t| t.starts_with('"'))
                    && toks.get(j + 1).is_some_and(|t| t.starts_with("--"))
                {
                    // `a -- "label" --> b`.
                    text = Some(label(&toks[j]));
                    j += 2;
                }
                let (Some(from), Some(target)) = (last, toks.get(j)) else {
                    return Err(Error::unexpected(line));
                };
                let (id, ..) = shaped(target.split(':').next().unwrap_or(target));
                let to = match find(&d, &id) {
                    Some(b) => b,
                    None => add(&mut d, &open, target),
                };
                d.edges.push((from, to, text));
                last = Some(to);
                i = j + 1;
                continue;
            }
            if let Some(rest) =
                tok.strip_prefix("block:").or_else(|| (tok == "block").then_some(""))
            {
                let id = rest.split(':').next().unwrap_or("").to_owned();
                let b =
                    add(&mut d, &open, &format!("{id}:{}", rest.split(':').nth(1).unwrap_or("1")));
                d.blocks[b].label = String::new();
                open.push((b, line.number));
                last = None;
                i += 1;
                continue;
            }
            let (id, ..) = shaped(tok.split(':').next().unwrap_or(tok));
            last = Some(match find(&d, &id) {
                Some(b) if !tok.starts_with("space") => b,
                _ => add(&mut d, &open, tok),
            });
            i += 1;
        }
    }
    if let Some(&(_, number)) = open.last() {
        return Err(Error::new(number, ErrorKind::Unclosed("end")));
    }
    Ok(d)
}

/// A block written `tok` (`id[label]:span`, `space:2`) added where blocks are open.
fn add(d: &mut BlockDiagram, open: &[(usize, usize)], tok: &str) -> usize {
    let (body, span) = match tok.rsplit_once(':') {
        Some((b, s)) if s.parse::<u32>().is_ok() => (b, s.parse::<u32>().unwrap_or(1)),
        _ => (tok, 1),
    };
    let space = body == "space";
    let (id, label, shape) =
        if space { (String::new(), String::new(), Shape::Rect) } else { shaped(body) };
    let n = d.blocks.len();
    d.blocks.push(Block { id, label, shape, span: span.max(1), space, ..Block::default() });
    match open.last() {
        Some(&(p, _)) => d.blocks[p].children.push(n),
        None => d.top.push(n),
    }
    n
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/block.rs"]
mod tests;
