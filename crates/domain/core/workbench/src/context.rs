//! What an action acts on, as every module sees it: where it was asked for (a menu's place,
//! a shortcut's) and its targets (the right-clicked entry, the tree's selection, the active
//! file), each with what is known of it — its facts. Whoever shows the targets (the tree, a
//! tab, the editor) builds it with what it knows (an entry's mark: absent, only here…);
//! modules add what only they know ([`ContextProvider`]: a remote project's excluded
//! paths…). Commands then decide from it whether they apply (`enabled`) and what they act
//! on (`run`): modules learn the context without knowing each other.

use std::path::{Path, PathBuf};

use crate::menus::MenuPlace;

/// The facts known to every module (an entry's state in the tree); a module's own facts
/// are named after it (`remote.excluded`).
pub mod facts {
    /// Not on disk: known elsewhere (a remote project's file not brought yet).
    pub const ABSENT: &str = "entry.absent";
    /// Only here (never transferred).
    pub const ONLY_HERE: &str = "entry.only_here";
    /// Changed here since its last transfer.
    pub const CHANGED: &str = "entry.changed";
    /// Changed elsewhere since its last transfer.
    pub const BEHIND: &str = "entry.behind";
    /// Changed on both sides.
    pub const CONFLICT: &str = "entry.conflict";
}

/// One thing an action acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    pub is_dir: bool,
    pub facts: Vec<&'static str>,
}

impl Target {
    pub fn new(path: PathBuf, is_dir: bool) -> Self {
        Self { path, is_dir, facts: Vec::new() }
    }

    /// The fact of an entry's mark (none for an entry as it should be).
    pub fn with_mark(mut self, mark: workspace::Mark, absent: bool) -> Self {
        use workspace::Mark;
        let fact = match mark {
            _ if absent => Some(facts::ABSENT),
            Mark::None => None,
            Mark::Local => Some(facts::ONLY_HERE),
            Mark::Changed => Some(facts::CHANGED),
            Mark::Behind => Some(facts::BEHIND),
            Mark::Conflict => Some(facts::CONFLICT),
        };
        self.facts.extend(fact);
        self
    }

    pub fn has(&self, fact: &str) -> bool {
        self.facts.contains(&fact)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionContext {
    pub place: MenuPlace,
    pub targets: Vec<Target>,
}

impl ActionContext {
    pub fn new(place: MenuPlace, targets: Vec<Target>) -> Self {
        Self { place, targets }
    }

    /// Nothing to act on (no file active, nothing selected).
    pub fn empty(place: MenuPlace) -> Self {
        Self::new(place, Vec::new())
    }

    /// The first target when it's a folder (as the tree knows it: the disk isn't asked).
    pub fn folder(&self) -> Option<&Path> {
        self.targets.first().filter(|t| t.is_dir).map(|t| t.path.as_path())
    }

    /// The first target's path (most actions take one).
    pub fn path(&self) -> Option<&Path> {
        self.targets.first().map(|t| t.path.as_path())
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.targets.iter().map(|t| t.path.clone()).collect()
    }

    /// Adds `fact` to the target at `path` (a provider's).
    pub fn add(&mut self, path: &Path, fact: &'static str) {
        if let Some(target) = self.targets.iter_mut().find(|t| t.path == path)
            && !target.has(fact)
        {
            target.facts.push(fact);
        }
    }

    /// Some target has `fact`.
    pub fn any(&self, fact: &str) -> bool {
        self.targets.iter().any(|t| t.has(fact))
    }

    /// There are targets, and all have `fact`.
    pub fn all(&self, fact: &str) -> bool {
        !self.targets.is_empty() && self.targets.iter().all(|t| t.has(fact))
    }
}

/// A module's knowledge of the targets, added to every context built (a menu opening, a
/// shortcut): answered from what it holds — never waiting on the disk or the network.
pub trait ContextProvider: Send + Sync + std::fmt::Debug {
    /// Adds its facts to `context`'s targets (`project`: the open project — its identity,
    /// what a module keeps per project goes by it — and its root).
    fn add_facts(&self, project: Option<(kernel::ProjectId, &Path)>, context: &mut ActionContext);
}

#[cfg(test)]
#[path = "../tests/unit/context.rs"]
mod tests;
