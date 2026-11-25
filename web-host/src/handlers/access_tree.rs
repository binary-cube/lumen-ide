//! A page's accessibility tree while its app asks for it (a screen reader runs): kept from
//! Chromium's updates, told when it changed, at most every [`ACCESS_EVERY`].

use std::time::{Duration, Instant};

use web_engine_protocol::AccessNode;

use crate::access::Tree;

/// The least time between two trees told for a page: a page changing all the time (a field
/// typed in, a ticking text) tells at most ten a second, the latest (a screen reader reads
/// far slower; the window's tree goes no faster).
pub const ACCESS_EVERY: Duration = Duration::from_millis(100);

/// A page's tree while asked: what's changed since it was last told, and when.
#[derive(Debug, Default)]
pub struct AccessState {
    pub tree: Tree,
    /// Changed since it was last told (or asked again whole).
    changed: bool,
    /// When it was last told (`None`: never).
    told: Option<Instant>,
}

/// Whether a tree is told now.
#[derive(Debug)]
pub enum Due {
    /// Nothing changed: nothing to tell.
    Nothing,
    /// Changed, told at this time (the last one was told too recently).
    At(Instant),
    /// Told now: its nodes.
    Now(Vec<AccessNode>),
}

impl AccessState {
    /// Something changed (or the tree is asked again whole): told when due.
    pub fn changed(&mut self) {
        self.changed = true;
    }

    /// The tree told if it changed and [`ACCESS_EVERY`] has passed since it was last.
    pub fn due(&mut self, now: Instant) -> Due {
        if !self.changed {
            return Due::Nothing;
        }
        if let Some(due) = self.told.map(|t| t + ACCESS_EVERY).filter(|due| *due > now) {
            return Due::At(due);
        }
        self.changed = false;
        self.told = Some(now);
        Due::Now(self.tree.told())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/handlers/access_tree.rs"]
mod tests;
