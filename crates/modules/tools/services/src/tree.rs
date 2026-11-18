//! The window's rows: each contributor's group, then its nodes under their parents (in
//! their order), folded or not, filtered by what's typed (a node shown when it or one of
//! its children matches, with its parents; everything open while filtering).

use std::collections::{HashMap, HashSet};

use ui_components::{ServiceId, ServiceNode, ServiceState};
use ui_kit::ListFilter;

/// A row's identity: a contributor's group, or one of its nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Group(&'static str),
    Node(&'static str, ServiceId),
}

impl Key {
    /// The contributor it's of.
    pub fn provider(&self) -> &'static str {
        match self {
            Self::Group(p) | Self::Node(p, _) => p,
        }
    }

    pub fn node(&self) -> Option<ServiceId> {
        match self {
            Self::Node(_, id) => Some(*id),
            Self::Group(_) => None,
        }
    }
}

/// A row as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: Key,
    pub depth: u8,
    pub icon: &'static str,
    pub label: String,
    pub detail: String,
    pub state: ServiceState,
    pub color: Option<String>,
    pub expandable: bool,
    pub expanded: bool,
}

/// A contributor's group, as the rows are made from it.
#[derive(Debug, Clone, Copy)]
pub struct Group<'a> {
    pub provider: &'static str,
    pub icon: &'static str,
    pub title: &'a str,
    pub nodes: &'a [ServiceNode],
}

/// The rows of `groups` (a group without nodes isn't shown), `collapsed` folded unless
/// `filter` has something typed.
pub fn rows(groups: &[Group<'_>], collapsed: &HashSet<Key>, filter: &ListFilter) -> Vec<Row> {
    let mut out = Vec::new();
    for group in groups.iter().filter(|g| !g.nodes.is_empty()) {
        let known: HashSet<ServiceId> = group.nodes.iter().map(|n| n.id).collect();
        // Each node's children, in their order; a node whose parent is unknown is at the top.
        let mut children: HashMap<Option<ServiceId>, Vec<usize>> = HashMap::new();
        for (i, node) in group.nodes.iter().enumerate() {
            let parent = node.parent.filter(|p| known.contains(p) && *p != node.id);
            children.entry(parent).or_default().push(i);
        }
        let filtering = !filter.is_empty();
        let whole = filtering && filter.matches(group.title);
        // While filtering: the nodes shown (matching, or with a match below).
        let mut shown = vec![!filtering || whole; group.nodes.len()];
        if filtering && !whole {
            fn mark(
                i: usize,
                nodes: &[ServiceNode],
                children: &HashMap<Option<ServiceId>, Vec<usize>>,
                filter: &ListFilter,
                shown: &mut [bool],
                depth: usize,
            ) -> bool {
                let node = &nodes[i];
                let mut any = filter.matches_any([node.name.as_str(), node.detail.as_str()]);
                // A cycle can't recurse forever: a tree is never deeper than its nodes.
                if depth < nodes.len() {
                    for &c in children.get(&Some(node.id)).map_or(&[][..], Vec::as_slice) {
                        any |= mark(c, nodes, children, filter, shown, depth + 1);
                    }
                }
                shown[i] = any;
                any
            }
            for &i in children.get(&None).map_or(&[][..], Vec::as_slice) {
                mark(i, group.nodes, &children, filter, &mut shown, 0);
            }
            if !shown.iter().any(|s| *s) {
                continue;
            }
        }
        let key = Key::Group(group.provider);
        let open = filtering || !collapsed.contains(&key);
        out.push(Row {
            key,
            depth: 0,
            icon: group.icon,
            label: group.title.to_owned(),
            detail: String::new(),
            state: ServiceState::Idle,
            color: None,
            expandable: true,
            expanded: open,
        });
        if !open {
            continue;
        }
        let mut stack: Vec<(usize, u8)> = children
            .get(&None)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .rev()
            .map(|&i| (i, 1))
            .collect();
        let mut seen = HashSet::new();
        while let Some((i, depth)) = stack.pop() {
            let node = &group.nodes[i];
            if !shown[i] || !seen.insert(node.id) {
                continue;
            }
            let key = Key::Node(group.provider, node.id);
            let kids = children.get(&Some(node.id)).map_or(&[][..], Vec::as_slice);
            let expandable = kids.iter().any(|&c| shown[c]);
            let expanded = expandable && (filtering || !collapsed.contains(&key));
            out.push(Row {
                key,
                depth,
                icon: node.icon,
                label: node.name.clone(),
                detail: node.detail.clone(),
                state: node.state,
                color: node.color.clone(),
                expandable,
                expanded,
            });
            if expanded {
                stack.extend(kids.iter().rev().map(|&c| (c, depth.saturating_add(1))));
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/tree.rs"]
mod tests;
