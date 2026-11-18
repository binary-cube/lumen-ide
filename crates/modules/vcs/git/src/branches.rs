//! The Git Log's branches panel: every commit ("All"), `HEAD`, then the local branches, the
//! remote ones (by remote) and the tags — names with `/` in folders (`feature/login` under
//! `feature`), folders collapsible. Choosing one shows only its history.

use std::collections::HashSet;

use version_control::{RefKind, RefLabel};

/// What a row of the panel is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchItem {
    /// Every branch's history (no filter).
    All,
    /// The checked-out commit's line.
    Head,
    /// A section or a folder (its key: `local`, `local/feature`, `remote/origin`…).
    Group { key: String, label: String, expanded: bool },
    /// A reference: its full name (what the log is filtered by) and the name shown.
    Ref { name: String, label: String, kind: RefKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRow {
    pub depth: usize,
    pub item: BranchItem,
}

/// The panel's rows for `refs`; `collapsed`: the groups closed; `query`: only the references
/// whose name has it (any case; their folders open), when there's one.
pub fn rows(refs: &[RefLabel], collapsed: &HashSet<String>, query: &str) -> Vec<BranchRow> {
    let query = query.to_lowercase();
    let searching = !query.is_empty();
    let empty = HashSet::new();
    let collapsed = if searching { &empty } else { collapsed };
    let mut out = vec![
        BranchRow { depth: 0, item: BranchItem::All },
        BranchRow { depth: 0, item: BranchItem::Head },
    ];
    let sections = [
        ("local", "git.branches_local", RefKind::Branch),
        ("remote", "git.branches_remote", RefKind::Remote),
        ("tags", "git.branches_tags", RefKind::Tag),
    ];
    for (key, label, kind) in sections {
        let mut names: Vec<&str> = refs
            .iter()
            .filter(|r| r.kind == kind && (!searching || r.name.to_lowercase().contains(&query)))
            .map(|r| r.name.as_str())
            .collect();
        if names.is_empty() {
            continue;
        }
        names.sort_unstable();
        let expanded = !collapsed.contains(key);
        out.push(BranchRow {
            depth: 0,
            item: BranchItem::Group { key: key.into(), label: i18n::t(label), expanded },
        });
        if expanded {
            let entries: Vec<(&str, &str)> = names.iter().map(|n| (*n, *n)).collect();
            // Tags aren't foldered (`v1.2/…` is rare and reads better flat).
            if kind == RefKind::Tag {
                for (name, _) in entries {
                    out.push(leaf(1, name, name, kind));
                }
            } else {
                tree(&mut out, &entries, key, 1, kind, collapsed);
            }
        }
    }
    out
}

fn leaf(depth: usize, name: &str, label: &str, kind: RefKind) -> BranchRow {
    BranchRow {
        depth,
        item: BranchItem::Ref { name: name.to_owned(), label: label.to_owned(), kind },
    }
}

/// `entries` (full name, the rest under the current folder) as folders and leaves.
fn tree(
    out: &mut Vec<BranchRow>,
    entries: &[(&str, &str)],
    key: &str,
    depth: usize,
    kind: RefKind,
    collapsed: &HashSet<String>,
) {
    let mut i = 0;
    while i < entries.len() {
        let (name, rest) = entries[i];
        let Some((folder, _)) = rest.split_once('/') else {
            out.push(leaf(depth, name, rest, kind));
            i += 1;
            continue;
        };
        // Everything in this folder (the entries are sorted: they're together).
        let inside: Vec<(&str, &str)> = entries[i..]
            .iter()
            .take_while(|(_, r)| r.split_once('/').is_some_and(|(f, _)| f == folder))
            .map(|(n, r)| (*n, r.split_once('/').map_or(*r, |(_, rest)| rest)))
            .collect();
        let count = inside.len();
        let key = format!("{key}/{folder}");
        let expanded = !collapsed.contains(&key);
        out.push(BranchRow {
            depth,
            item: BranchItem::Group { key: key.clone(), label: folder.to_owned(), expanded },
        });
        if expanded {
            tree(out, &inside, &key, depth + 1, kind, collapsed);
        }
        i += count;
    }
}

#[cfg(test)]
#[path = "../tests/unit/branches.rs"]
mod tests;
