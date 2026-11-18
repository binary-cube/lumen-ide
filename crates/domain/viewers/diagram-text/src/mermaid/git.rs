//! `gitGraph`: commits (`id`, `tag`, `type`), branches (with their `order`), checkouts,
//! merges and cherry-picks, in the order they're written; `LR:` / `TB:` / `BT:`.

use super::{Direction, Error, Line, unquote};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    #[default]
    Normal,
    Highlight,
    Reverse,
    Merge,
    CherryPick,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub branch: usize,
    pub parents: Vec<usize>,
    pub tag: Option<String>,
    pub kind: Kind,
    /// Its id was given (shown); otherwise it's made up (and not shown).
    pub named: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitGraph {
    pub direction: Direction,
    pub branches: Vec<Branch>,
    pub commits: Vec<Commit>,
}

/// `key: "value"` options of a statement.
fn options(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while let Some(colon) = rest.find(':') {
        let key = rest[..colon].trim().to_owned();
        let after = rest[colon + 1..].trim_start();
        let (value, next) = if let Some(q) = after.strip_prefix('"') {
            let end = q.find('"').unwrap_or(q.len());
            (q[..end].to_owned(), q.get(end + 1..).unwrap_or(""))
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (after[..end].to_owned(), &after[end..])
        };
        out.push((key, value));
        rest = next.trim_start();
    }
    out
}

pub(super) fn parse(head: Line<'_>, lines: &[Line<'_>]) -> Result<GitGraph, Error> {
    let mut g = GitGraph::default();
    if let Some(d) = head.text.split_whitespace().nth(1) {
        g.direction = match d.trim_end_matches(':') {
            "TB" | "TD" => Direction::Down,
            "BT" => Direction::Up,
            _ => Direction::Right,
        };
    } else {
        g.direction = Direction::Right;
    }
    g.branches.push(Branch { name: "main".into(), order: None });
    let mut current = 0;
    // Each branch's last commit.
    let mut heads: Vec<Option<usize>> = vec![None];
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        let find = |g: &GitGraph, name: &str| g.branches.iter().position(|b| b.name == name);
        match word {
            "commit" => {
                let opts = options(rest);
                let kind = match opts.iter().find(|(k, _)| k == "type").map(|(_, v)| v.as_str()) {
                    Some("HIGHLIGHT") => Kind::Highlight,
                    Some("REVERSE") => Kind::Reverse,
                    _ => Kind::Normal,
                };
                let id = opts.iter().find(|(k, _)| k == "id").map(|(_, v)| v.clone());
                let tag = opts.iter().find(|(k, _)| k == "tag").map(|(_, v)| v.clone());
                let parents = heads[current].into_iter().collect();
                push(&mut g, &mut heads, current, id, tag, kind, parents);
            }
            "branch" => {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = unquote(parts.next().unwrap_or_default()).to_owned();
                let order = options(parts.next().unwrap_or_default())
                    .iter()
                    .find(|(k, _)| k == "order")
                    .and_then(|(_, v)| v.parse().ok());
                if name.is_empty() || find(&g, &name).is_some() {
                    return Err(Error::unexpected(line));
                }
                g.branches.push(Branch { name, order });
                heads.push(heads[current]);
                current = g.branches.len() - 1;
            }
            "checkout" | "switch" => {
                current = find(&g, unquote(rest)).ok_or_else(|| Error::unexpected(line))?;
            }
            "merge" => {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = unquote(parts.next().unwrap_or_default());
                let other = find(&g, name).ok_or_else(|| Error::unexpected(line))?;
                let opts = options(parts.next().unwrap_or_default());
                let id = opts.iter().find(|(k, _)| k == "id").map(|(_, v)| v.clone());
                let tag = opts.iter().find(|(k, _)| k == "tag").map(|(_, v)| v.clone());
                let parents = heads[current].into_iter().chain(heads[other]).collect();
                push(&mut g, &mut heads, current, id, tag, Kind::Merge, parents);
            }
            "cherry-pick" => {
                let opts = options(rest);
                let picked = opts
                    .iter()
                    .find(|(k, _)| k == "id")
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| Error::unexpected(line))?;
                let source = g
                    .commits
                    .iter()
                    .position(|c| c.id == picked)
                    .ok_or_else(|| Error::unexpected(line))?;
                let tag = opts.iter().find(|(k, _)| k == "tag").map(|(_, v)| v.clone());
                let parents = heads[current].into_iter().chain([source]).collect();
                push(
                    &mut g,
                    &mut heads,
                    current,
                    Some(format!("{picked}'")),
                    tag,
                    Kind::CherryPick,
                    parents,
                );
            }
            _ => return Err(Error::unexpected(line)),
        }
    }
    Ok(g)
}

fn push(
    g: &mut GitGraph,
    heads: &mut [Option<usize>],
    branch: usize,
    id: Option<String>,
    tag: Option<String>,
    kind: Kind,
    parents: Vec<usize>,
) {
    let n = g.commits.len();
    let named = id.is_some();
    let id = id.unwrap_or_else(|| format!("{n}"));
    g.commits.push(Commit { id, branch, parents, tag, kind, named });
    heads[branch] = Some(n);
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/git.rs"]
mod tests;
