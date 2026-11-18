//! The types that are boxes, groups and lines with their own syntax: `requirementDiagram`
//! (requirements and elements with their fields, relations like `- satisfies ->`),
//! C4 (`C4Context`, `C4Container`, `C4Component`, `C4Dynamic`, `C4Deployment`: people,
//! systems, containers, components, boundaries, `Rel`s) and `architecture-beta` (groups,
//! services, junctions, edges between sides). Read into one model: boxes of a kind, with a
//! title and lines, in groups, joined by labeled lines.

use std::collections::HashMap;

use super::{Direction, Error, ErrorKind, Line, Meta, unquote};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoxNode {
    pub id: String,
    /// What it is (`requirement`, `element`, `Person`, `System_Ext`, `service`…).
    pub kind: String,
    pub title: String,
    /// What it says under its title (fields, a technology, a description).
    pub lines: Vec<String>,
    pub group: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoxGroup {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoxEdge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    /// The line's own words (C4's technology, a requirement relation's kind).
    pub detail: Option<String>,
    /// Arrows at its target and at its source.
    pub head: bool,
    pub tail: bool,
    pub dashed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoxGraph {
    pub direction: Direction,
    pub nodes: Vec<BoxNode>,
    pub groups: Vec<BoxGroup>,
    pub edges: Vec<BoxEdge>,
}

impl BoxGraph {
    fn node(&mut self, ids: &mut HashMap<String, usize>, id: &str, group: Option<usize>) -> usize {
        if let Some(&n) = ids.get(id) {
            return n;
        }
        self.nodes.push(BoxNode {
            id: id.to_owned(),
            title: id.to_owned(),
            group,
            ..BoxNode::default()
        });
        ids.insert(id.to_owned(), self.nodes.len() - 1);
        self.nodes.len() - 1
    }
}

/// `requirementDiagram`.
pub(super) fn requirements(lines: &[Line<'_>]) -> Result<BoxGraph, Error> {
    const KINDS: &[&str] = &[
        "requirement",
        "functionalRequirement",
        "interfaceRequirement",
        "performanceRequirement",
        "physicalRequirement",
        "designConstraint",
        "element",
    ];
    let mut g = BoxGraph::default();
    let mut ids = HashMap::new();
    let mut open: Option<(usize, usize)> = None;
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if let Some((n, _)) = open {
            if t == "}" {
                open = None;
            } else if let Some((k, v)) = t.split_once(':') {
                g.nodes[n].lines.push(format!("{}: {}", k.trim(), unquote(v.trim())));
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("direction") {
            g.direction = Direction::parse(rest).ok_or_else(|| Error::unexpected(line))?;
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        if KINDS.contains(&word) {
            let name = t[word.len()..].trim().trim_end_matches('{').trim();
            let n = g.node(&mut ids, unquote(name), None);
            g.nodes[n].kind = word.to_owned();
            g.nodes[n].title = unquote(name).to_owned();
            open = Some((n, line.number));
            continue;
        }
        // `a - satisfies -> b`, `a <- copies - b`.
        let (from, rel, to, reverse) = if let Some((a, rest)) = t.split_once(" - ") {
            let (rel, b) = rest.split_once(" -> ").ok_or_else(|| Error::unexpected(line))?;
            (a, rel, b, false)
        } else if let Some((a, rest)) = t.split_once(" <- ") {
            let (rel, b) = rest.split_once(" - ").ok_or_else(|| Error::unexpected(line))?;
            (a, rel, b, true)
        } else {
            return Err(Error::unexpected(line));
        };
        let (a, b) = (
            g.node(&mut ids, unquote(from.trim()), None),
            g.node(&mut ids, unquote(to.trim()), None),
        );
        let (from, to) = if reverse { (b, a) } else { (a, b) };
        g.edges.push(BoxEdge {
            from,
            to,
            label: Some(format!("«{}»", rel.trim())),
            head: true,
            dashed: true,
            ..BoxEdge::default()
        });
    }
    if let Some((_, number)) = open {
        return Err(Error::new(number, ErrorKind::Unclosed("}")));
    }
    Ok(g)
}

/// A call's arguments (`Person(a, "Label", "Descr")`), quotes kept out, `$key=value` left.
fn arguments(s: &str) -> Vec<String> {
    let inner = s.split_once('(').map(|(_, r)| r).unwrap_or("");
    let inner = inner.rsplit_once(')').map(|(i, _)| i).unwrap_or(inner);
    let mut out = Vec::new();
    let (mut field, mut quoted) = (String::new(), false);
    for c in inner.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut field).trim().to_owned()),
            c => field.push(c),
        }
    }
    if !field.trim().is_empty() || !out.is_empty() {
        out.push(field.trim().to_owned());
    }
    out.into_iter().filter(|a| !a.starts_with('$')).collect()
}

/// C4's diagrams.
pub(super) fn c4(lines: &[Line<'_>], meta: &mut Meta) -> Result<BoxGraph, Error> {
    let mut g = BoxGraph::default();
    let mut ids = HashMap::new();
    let mut open: Vec<(usize, usize)> = Vec::new();
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        if t == "}" {
            if open.pop().is_none() {
                return Err(Error::unexpected(line));
            }
            continue;
        }
        if let Some(title) = t.strip_prefix("title ") {
            meta.title = Some(title.trim().to_owned());
            continue;
        }
        let call = t.split('(').next().unwrap_or_default().trim();
        let args = arguments(t);
        let group = open.last().map(|&(g, _)| g);
        match call {
            c if c.ends_with("Boundary")
                || c == "Node"
                || c.starts_with("Deployment_Node")
                || c == "Node_L"
                || c == "Node_R" =>
            {
                let id = args.first().cloned().unwrap_or_default();
                let title = args.get(1).cloned().unwrap_or_else(|| id.clone());
                g.groups.push(BoxGroup { id, kind: c.to_owned(), title, parent: group });
                if t.ends_with('{') {
                    open.push((g.groups.len() - 1, line.number));
                }
            }
            c if c.starts_with("Rel") || c == "BiRel" => {
                if args.len() < 2 {
                    return Err(Error::unexpected(line));
                }
                let a = g.node(&mut ids, &args[0], None);
                let b = g.node(&mut ids, &args[1], None);
                let back = c == "Rel_Back";
                let (from, to) = if back { (b, a) } else { (a, b) };
                g.edges.push(BoxEdge {
                    from,
                    to,
                    label: args.get(2).cloned().filter(|l| !l.is_empty()),
                    detail: args
                        .get(3)
                        .cloned()
                        .filter(|l| !l.is_empty())
                        .map(|d| format!("[{d}]")),
                    head: true,
                    tail: c == "BiRel",
                    dashed: true,
                });
            }
            c if c.starts_with("Update")
                || c.starts_with("Lay_")
                || c == "AddElementTag"
                || c == "AddRelTag" => {}
            c if [
                "Person",
                "Person_Ext",
                "System",
                "System_Ext",
                "SystemDb",
                "SystemDb_Ext",
                "SystemQueue",
                "SystemQueue_Ext",
                "Container",
                "Container_Ext",
                "ContainerDb",
                "ContainerDb_Ext",
                "ContainerQueue",
                "ContainerQueue_Ext",
                "Component",
                "Component_Ext",
                "ComponentDb",
                "ComponentDb_Ext",
                "ComponentQueue",
                "ComponentQueue_Ext",
            ]
            .contains(&c) =>
            {
                let id = args.first().cloned().ok_or_else(|| Error::unexpected(line))?;
                let n = g.node(&mut ids, &id, group);
                let node = &mut g.nodes[n];
                node.kind = c.to_owned();
                node.group = group;
                node.title = args.get(1).cloned().unwrap_or(id);
                // Containers and components: a technology, then a description.
                let has_tech = c.starts_with("Container") || c.starts_with("Component");
                if has_tech && let Some(tech) = args.get(2).filter(|s| !s.is_empty()) {
                    node.lines.push(format!("[{tech}]"));
                }
                let descr = if has_tech { args.get(3) } else { args.get(2) };
                if let Some(d) = descr.filter(|s| !s.is_empty()) {
                    node.lines.push(d.clone());
                }
            }
            _ => return Err(Error::unexpected(line)),
        }
    }
    if let Some(&(_, number)) = open.last() {
        return Err(Error::new(number, ErrorKind::Unclosed("}")));
    }
    Ok(g)
}

/// `architecture-beta`.
pub(super) fn architecture(lines: &[Line<'_>], meta: &mut Meta) -> Result<BoxGraph, Error> {
    let mut g = BoxGraph { direction: Direction::Right, ..BoxGraph::default() };
    let mut ids = HashMap::new();
    let mut group_ids: HashMap<String, usize> = HashMap::new();
    // `id(icon)[Title] in parent`.
    let decl = |s: &str| -> (String, String, String, Option<String>) {
        let (s, parent) = match s.split_once(" in ") {
            Some((a, p)) => (a.trim(), Some(p.trim().to_owned())),
            None => (s.trim(), None),
        };
        let id_end = s.find(['(', '[']).unwrap_or(s.len());
        let id = s[..id_end].trim().to_owned();
        let icon = s
            .split_once('(')
            .and_then(|(_, r)| r.split_once(')'))
            .map(|(i, _)| i.to_owned())
            .unwrap_or_default();
        let title = s
            .split_once('[')
            .and_then(|(_, r)| r.rsplit_once(']'))
            .map(|(t, _)| t.to_owned())
            .unwrap_or_else(|| id.clone());
        (id, icon, title, parent)
    };
    for line in lines {
        let t = line.text.trim();
        if t.is_empty() {
            continue;
        }
        let word = t.split_whitespace().next().unwrap_or_default();
        let rest = t[word.len()..].trim();
        match word {
            "title" => meta.title = Some(rest.to_owned()),
            "group" => {
                let (id, icon, title, parent) = decl(rest);
                let parent = parent.and_then(|p| group_ids.get(&p).copied());
                group_ids.insert(id.clone(), g.groups.len());
                g.groups.push(BoxGroup { id, kind: icon, title, parent });
            }
            "service" | "junction" => {
                let (id, icon, title, parent) = decl(rest);
                let group = parent.and_then(|p| group_ids.get(&p).copied());
                let n = g.node(&mut ids, &id, group);
                let node = &mut g.nodes[n];
                node.kind = if word == "junction" { "junction".into() } else { icon };
                node.title = if word == "junction" { String::new() } else { title };
                node.group = group;
            }
            _ => {
                // `a:R -- L:b`, `a:R --> L:b`, `a{group}:B <-- T:c`.
                let (left, right, head, tail) = if let Some((a, b)) = t.split_once("<-->") {
                    (a, b, true, true)
                } else if let Some((a, b)) = t.split_once("-->") {
                    (a, b, true, false)
                } else if let Some((a, b)) = t.split_once("<--") {
                    (a, b, false, true)
                } else if let Some((a, b)) = t.split_once("--") {
                    (a, b, false, false)
                } else {
                    return Err(Error::unexpected(line));
                };
                let end = |s: &str, left: bool| -> String {
                    let s = s.trim();
                    let s = if left {
                        s.rsplit_once(':').map_or(s, |(a, _)| a)
                    } else {
                        s.split_once(':').map_or(s, |(_, b)| b)
                    };
                    s.split('{').next().unwrap_or(s).trim().to_owned()
                };
                let (a, b) = (end(left, true), end(right, false));
                if a.is_empty() || b.is_empty() {
                    return Err(Error::unexpected(line));
                }
                let (from, to) = (g.node(&mut ids, &a, None), g.node(&mut ids, &b, None));
                g.edges.push(BoxEdge { from, to, head, tail, ..BoxEdge::default() });
            }
        }
    }
    Ok(g)
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/boxes.rs"]
mod tests;
