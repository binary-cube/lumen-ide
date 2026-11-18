//! The repository's places beside the commit screen in the Git window (as a Git client's
//! sidebar): the workspace (the files' status — this screen —, the history, a search), the
//! branches (in folders, the current one marked, what it has to push), the tags, the remotes
//! (their branches by remote), the stashes and the submodules. Each section folds.

use std::collections::HashSet;

use version_control::{RefKind, RefLabel, Stash, SyncState};

use crate::branches::{self, BranchItem};

/// What the places are made of, read in the background with the lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Places {
    pub refs: Vec<RefLabel>,
    pub stashes: Vec<Stash>,
    pub submodules: Vec<String>,
    pub states: Vec<SyncState>,
}

/// The workspace's places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Work {
    /// The files' status: this screen.
    FileStatus,
    History,
    Search,
}

/// A row of the places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    /// A section's head (its key, folded or not).
    Section {
        key: &'static str,
        expanded: bool,
    },
    Work(Work),
    /// A folder of branches (`feature` of `feature/login`), by its key.
    Folder {
        key: String,
        label: String,
        expanded: bool,
    },
    /// A branch or a tag: its name (what the log shows the history of) and what's shown;
    /// for a local branch, whether it's the current one and what it has to push.
    Ref {
        name: String,
        label: String,
        kind: RefKind,
        current: bool,
        ahead: usize,
    },
    Stash(Stash),
    Submodule(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaceRow {
    pub depth: usize,
    pub place: Place,
}

/// The sections in order: their keys, the references they list (if they're references),
/// their names' keys and icons.
pub(crate) const SECTIONS: &[(&str, Option<RefKind>, &str, &str)] = &[
    ("workspace", None, "git.places_workspace", "app-window"),
    ("branches", Some(RefKind::Branch), "git.places_branches", "git-branch"),
    ("tags", Some(RefKind::Tag), "git.places_tags", "tag"),
    ("remotes", Some(RefKind::Remote), "git.places_remotes", "cloud"),
    ("stashes", None, "git.places_stashes", "layers"),
    ("submodules", None, "git.places_submodules", "modules"),
];

/// The rows for `places` (`branch`: the current one), the sections and folders in
/// `collapsed` folded. With a `query`, only what contains it (the references by name, the
/// stashes by message, the submodules by path), every fold open, the sections finding
/// nothing and the workspace left out.
pub(crate) fn rows(
    places: &Places,
    branch: Option<&str>,
    collapsed: &HashSet<String>,
    query: &str,
) -> Vec<PlaceRow> {
    let filter = ui_kit::ListFilter::new(query);
    let searching = !filter.is_empty();
    let mut out = Vec::new();
    for (key, kind, _, _) in SECTIONS {
        let mut found = Vec::new();
        match (*key, kind) {
            ("workspace", _) if searching => {}
            ("workspace", _) => {
                for work in [Work::FileStatus, Work::History, Work::Search] {
                    found.push(PlaceRow { depth: 1, place: Place::Work(work) });
                }
            }
            (_, Some(kind)) => references(&mut found, places, *kind, branch, collapsed, query),
            ("stashes", _) => found.extend(
                places
                    .stashes
                    .iter()
                    .filter(|s| filter.matches_any([s.message.as_str(), s.name.as_str()]))
                    .map(|s| PlaceRow { depth: 1, place: Place::Stash(s.clone()) }),
            ),
            _ => found.extend(
                places
                    .submodules
                    .iter()
                    .filter(|path| filter.matches(path))
                    .map(|path| PlaceRow { depth: 1, place: Place::Submodule(path.clone()) }),
            ),
        }
        if searching && found.is_empty() {
            continue;
        }
        let expanded = searching || !collapsed.contains(*key);
        out.push(PlaceRow { depth: 0, place: Place::Section { key, expanded } });
        if expanded {
            out.extend(found);
        }
    }
    out
}

/// One kind of references, in folders (the branches pane's: its own section's head left
/// out, this one has its own), those containing `query`.
fn references(
    out: &mut Vec<PlaceRow>,
    places: &Places,
    kind: RefKind,
    branch: Option<&str>,
    collapsed: &HashSet<String>,
    query: &str,
) {
    let refs: Vec<RefLabel> = places.refs.iter().filter(|r| r.kind == kind).cloned().collect();
    for row in branches::rows(&refs, collapsed, query) {
        let place = match row.item {
            BranchItem::All | BranchItem::Head => continue,
            // The branches pane's section of this kind: this one stands for it.
            BranchItem::Group { .. } if row.depth == 0 => continue,
            BranchItem::Group { key, label, expanded } => Place::Folder { key, label, expanded },
            BranchItem::Ref { name, label, kind } => {
                let current = kind == RefKind::Branch && branch == Some(name.as_str());
                let ahead = places
                    .states
                    .iter()
                    .find(|s| kind == RefKind::Branch && s.branch == name)
                    .map_or(0, |s| s.ahead);
                Place::Ref { name, label, kind, current, ahead }
            }
        };
        out.push(PlaceRow { depth: row.depth, place });
    }
}

#[cfg(test)]
#[path = "../../tests/unit/commit_view/places.rs"]
mod tests;
