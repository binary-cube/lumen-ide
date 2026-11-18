//! The recently opened projects organized in groups (folders of the list, nested,
//! collapsible) and where they are kept (the [`RecentStore`] port; the TOML adapter lives in
//! `storage`). The projects outside the user's groups are in the [`RECENT`] group, which
//! always exists: it can be moved and collapsed, never renamed, deleted or given subgroups;
//! at most [`LIMIT`] projects are remembered in it. One put in another group stays until
//! it's taken out.
//!
//! Each group lists its projects most recently opened first, unless the user placed one
//! at a position: from then on the group keeps the order chosen ([`Order::Manual`]) until
//! it's sorted by opening again. The recent group is always by opening.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many projects outside the groups are remembered.
pub const LIMIT: usize = 20;

/// A group's identity (a UUID v4; zero for [`RECENT`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(pub u128);

/// The group of the projects outside the user's groups.
pub const RECENT: GroupId = GroupId(0);

impl GroupId {
    /// A new identity.
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }
}

impl Default for GroupId {
    fn default() -> Self {
        Self::new()
    }
}

/// How a group orders its projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Most recently opened first.
    #[default]
    Recent,
    /// As the user placed them.
    Manual,
}

/// A group of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    /// Empty for [`RECENT`] (its name is the UI's).
    pub name: String,
    /// The group it's in (`None`: at the top).
    pub parent: Option<GroupId>,
    pub collapsed: bool,
    pub order: Order,
    /// The projects' order chosen ([`Order::Manual`]); others placed later come after.
    pub manual: Vec<PathBuf>,
}

impl Group {
    fn recent() -> Self {
        Self {
            id: RECENT,
            name: String::new(),
            parent: None,
            collapsed: false,
            order: Order::Recent,
            manual: Vec::new(),
        }
    }
}

/// Where a project goes in its new group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Where the group's order puts it (the end of a manual order).
    Anywhere,
    /// At the end, chosen: the group keeps the order chosen.
    End,
    /// Before that project, chosen: the group keeps the order chosen.
    Before(PathBuf),
}

/// The list: the projects (most recently opened first), the groups (in their order among
/// siblings) and the group each project outside [`RECENT`] is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProjects {
    pub projects: Vec<PathBuf>,
    pub groups: Vec<Group>,
    pub placed: HashMap<PathBuf, GroupId>,
}

impl Default for RecentProjects {
    fn default() -> Self {
        Self::of(Vec::new())
    }
}

impl RecentProjects {
    /// Only projects, all in the recent group (an old list).
    pub fn of(projects: Vec<PathBuf>) -> Self {
        Self { projects, groups: Vec::new(), placed: HashMap::new() }.repaired()
    }

    /// The opening of `path`: it becomes the most recently opened (added if missing) and
    /// keeps its group; the oldest projects of the recent group beyond [`LIMIT`] are
    /// forgotten.
    pub fn open(&mut self, path: &Path) {
        self.projects.retain(|p| p != path);
        self.projects.insert(0, path.to_path_buf());
        let mut loose = 0;
        let placed = &self.placed;
        self.projects.retain(|p| {
            if placed.contains_key(p) {
                return true;
            }
            loose += 1;
            loose <= LIMIT
        });
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The group `path` is in.
    pub fn group_of(&self, path: &Path) -> GroupId {
        self.placed.get(path).copied().unwrap_or(RECENT)
    }

    /// The groups directly in `parent` (`None`: at the top), in their order.
    pub fn children(&self, parent: Option<GroupId>) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(move |g| g.parent == parent)
    }

    /// The projects directly in `group`, in its order.
    pub fn projects_in(&self, group: GroupId) -> Vec<&PathBuf> {
        let members: Vec<&PathBuf> =
            self.projects.iter().filter(|p| self.group_of(p) == group).collect();
        let Some(g) = self.group(group).filter(|g| g.order == Order::Manual) else {
            return members;
        };
        let mut ordered: Vec<&PathBuf> =
            g.manual.iter().filter_map(|m| members.iter().copied().find(|p| *p == m)).collect();
        for p in members {
            if !ordered.contains(&p) {
                ordered.push(p);
            }
        }
        ordered
    }

    /// `group` is `ancestor` or inside it (at any depth).
    pub fn is_within(&self, group: GroupId, ancestor: GroupId) -> bool {
        let mut at = Some(group);
        // Bounded by the number of groups: a broken file's cycle never loops forever.
        for _ in 0..=self.groups.len() {
            match at {
                Some(g) if g == ancestor => return true,
                Some(g) => at = self.group(g).and_then(|g| g.parent),
                None => return false,
            }
        }
        false
    }

    /// The names from the top down to `group` (`["Clients", "Northwind"]`); the recent
    /// group's own is empty.
    pub fn path_of(&self, group: GroupId) -> Vec<&str> {
        let mut names = Vec::new();
        let mut at = self.group(group);
        while let Some(g) = at {
            if names.len() > self.groups.len() {
                break;
            }
            names.push(g.name.as_str());
            at = g.parent.and_then(|p| self.group(p));
        }
        names.reverse();
        names
    }

    /// How many projects `group` holds, its subgroups' too.
    pub fn count_in(&self, group: GroupId) -> usize {
        self.projects.iter().filter(|p| self.is_within(self.group_of(p), group)).count()
    }

    /// Puts `path` in `group` at `place`. `false`: no such project or group.
    pub fn move_to(&mut self, path: &Path, group: GroupId, place: Place) -> bool {
        if !self.projects.iter().any(|p| p == path) || self.group(group).is_none() {
            return false;
        }
        // Its place in the order it leaves (and in the one it joins, set below).
        let shown: Vec<PathBuf> = self.projects_in(group).into_iter().cloned().collect();
        for g in &mut self.groups {
            g.manual.retain(|p| p != path);
        }
        if group == RECENT {
            self.placed.remove(path);
            return true;
        }
        self.placed.insert(path.to_path_buf(), group);
        let Some(g) = self.group_mut(group) else { return false };
        let chosen = !matches!(place, Place::Anywhere);
        if chosen && g.order == Order::Recent {
            g.order = Order::Manual;
            g.manual = shown.into_iter().filter(|p| p != path).collect();
        }
        if g.order == Order::Manual {
            let at = match &place {
                Place::Before(before) => g.manual.iter().position(|p| p == before),
                Place::Anywhere | Place::End => None,
            };
            g.manual.insert(at.unwrap_or(g.manual.len()), path.to_path_buf());
        }
        true
    }

    /// Puts group `id` in `parent` (`None`: at the top), before its sibling `before`
    /// (`None`: last). Refused (`false`) into itself, one of its own subgroups or the
    /// recent group.
    pub fn move_group(
        &mut self,
        id: GroupId,
        parent: Option<GroupId>,
        before: Option<GroupId>,
    ) -> bool {
        let refused =
            parent.is_some_and(|p| p == RECENT || self.is_within(p, id) || self.group(p).is_none());
        if refused || before == Some(id) {
            return false;
        }
        let Some(at) = self.groups.iter().position(|g| g.id == id) else { return false };
        let mut group = self.groups.remove(at);
        group.parent = parent;
        let at = before
            .and_then(|b| self.groups.iter().position(|g| g.id == b && g.parent == parent))
            .unwrap_or(self.groups.len());
        self.groups.insert(at, group);
        true
    }

    /// A new group named `name` in `parent` (last among its siblings).
    pub fn new_group(&mut self, name: &str, parent: Option<GroupId>) -> GroupId {
        let id = GroupId::new();
        let parent = parent.filter(|p| *p != RECENT && self.group(*p).is_some());
        self.groups.push(Group {
            id,
            name: name.trim().to_owned(),
            parent,
            collapsed: false,
            order: Order::Recent,
            manual: Vec::new(),
        });
        id
    }

    pub fn rename_group(&mut self, id: GroupId, name: &str) -> bool {
        let name = name.trim();
        match self.group_mut(id) {
            Some(g) if id != RECENT && !name.is_empty() => {
                g.name = name.to_owned();
                true
            }
            _ => false,
        }
    }

    /// Collapses or expands group `id`.
    pub fn set_collapsed(&mut self, id: GroupId, collapsed: bool) -> bool {
        match self.group_mut(id) {
            Some(g) if g.collapsed != collapsed => {
                g.collapsed = collapsed;
                true
            }
            _ => false,
        }
    }

    /// Group `id` lists its projects most recently opened first again.
    pub fn sort_by_opening(&mut self, id: GroupId) -> bool {
        match self.group_mut(id) {
            Some(g) if g.order == Order::Manual => {
                g.order = Order::Recent;
                g.manual.clear();
                true
            }
            _ => false,
        }
    }

    /// Removes group `id` (never the recent one): its projects and subgroups go up to its
    /// parent (the projects at the top go to the recent group), none is lost.
    pub fn delete_group(&mut self, id: GroupId) -> bool {
        if id == RECENT {
            return false;
        }
        let Some(at) = self.groups.iter().position(|g| g.id == id) else { return false };
        let parent = self.groups.remove(at).parent;
        for g in &mut self.groups {
            if g.parent == Some(id) {
                g.parent = parent;
            }
        }
        match parent {
            Some(parent) => self.placed.values_mut().filter(|g| **g == id).for_each(|g| {
                *g = parent;
            }),
            None => self.placed.retain(|_, g| *g != id),
        }
        true
    }

    /// Forgets the projects of the recent group (those in the user's groups stay: they
    /// were placed there); `true` if there was one.
    pub fn clear_recent(&mut self) -> bool {
        let before = self.projects.len();
        let placed = &self.placed;
        self.projects.retain(|p| placed.contains_key(p));
        self.projects.len() != before
    }

    /// Takes `path` out of the list (its folder stays where it is).
    pub fn forget(&mut self, path: &Path) -> bool {
        let before = self.projects.len();
        self.projects.retain(|p| p != path);
        self.placed.remove(path);
        for g in &mut self.groups {
            g.manual.retain(|p| p != path);
        }
        self.projects.len() != before
    }

    /// What a broken or older file may hold, made sound: the recent group is there (first
    /// when it was missing) and orders by opening, at the top level or in a group;
    /// groups whose parent is missing, a cycle or the recent group go to the top; projects
    /// placed in a missing group, or not in the list, are let go.
    pub fn repaired(mut self) -> Self {
        match self.groups.iter_mut().find(|g| g.id == RECENT) {
            Some(recent) => {
                recent.name.clear();
                recent.order = Order::Recent;
                recent.manual.clear();
            }
            None => self.groups.insert(0, Group::recent()),
        }
        let ids: Vec<GroupId> = self.groups.iter().map(|g| g.id).collect();
        for i in 0..self.groups.len() {
            let Some(parent) = self.groups[i].parent else { continue };
            let id = self.groups[i].id;
            self.groups[i].parent = None;
            let sound = parent != RECENT && ids.contains(&parent) && !self.is_within(parent, id);
            if sound {
                self.groups[i].parent = Some(parent);
            }
        }
        let projects = &self.projects;
        self.placed.retain(|p, g| *g != RECENT && ids.contains(g) && projects.contains(p));
        let placed = &self.placed;
        for g in &mut self.groups {
            let id = g.id;
            g.manual.retain(|p| placed.get(p) == Some(&id));
        }
        self
    }
}

/// Where the list is kept (port).
pub trait RecentStore: std::fmt::Debug + Send + Sync {
    fn load(&self) -> RecentProjects;
    fn save(&self, list: &RecentProjects) -> std::io::Result<()>;
}

/// Where a project's recently opened files are kept (port; the adapter lives in `storage`).
pub trait RecentFiles: std::fmt::Debug + Send + Sync {
    /// The files of the project at `root` (absolute), the latest last; none when never kept.
    fn load(&self, root: &Path) -> Vec<PathBuf>;
    /// Keeps `files` (those inside the project) for the project at `root`.
    fn save(&self, root: &Path, files: &[PathBuf]) -> std::io::Result<()>;
}

/// Changes the kept list with `change` (read again first: another window may have
/// changed it); saved only when `change` says it changed something.
pub fn update(
    store: &dyn RecentStore,
    change: impl FnOnce(&mut RecentProjects) -> bool,
) -> std::io::Result<bool> {
    let mut list = store.load();
    if !change(&mut list) {
        return Ok(false);
    }
    store.save(&list).map(|()| true)
}

/// Records the opening of `path` in `store`.
pub fn record(store: &dyn RecentStore, path: &Path) -> std::io::Result<()> {
    update(store, |list| {
        list.open(path);
        true
    })
    .map(|_| ())
}

#[cfg(test)]
#[path = "../tests/unit/recent.rs"]
mod tests;
