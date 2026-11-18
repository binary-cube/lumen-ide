//! The profiles a console keeps ([`Profiles`]): the newest last, at most so many and so many
//! bytes (the settings' `console_profile_keep` and `console_profile_memory`; the oldest
//! unpinned go first, never the newest), let go when the caches are cleared (the pinned
//! stay), all gone with the console.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use database::profile::rules::Facts;
use database::profile::{ProfileId, ProfileReport};

/// A profile kept.
#[derive(Debug, Clone)]
pub struct StoredProfile {
    pub report: Arc<ProfileReport>,
    /// The statement as sent (its secrets masked) and its place in the console's text.
    pub statement: String,
    pub range: Range<usize>,
    pub pinned: bool,
    /// Its bytes (counted once, when kept).
    pub bytes: usize,
    /// What was known of its tables (their structures, their statistics once read).
    pub facts: Option<Arc<Facts>>,
    /// The statement's fingerprint (the same query with other values: the same one).
    pub fingerprint: u64,
    /// When it was made (the Output's time of day) and how long it took.
    pub at: String,
    pub elapsed: Duration,
    /// Its tables' statistics were asked for (read, or not to be had).
    pub stats_asked: bool,
}

impl StoredProfile {
    pub fn new(report: Arc<ProfileReport>, statement: String, range: Range<usize>) -> Self {
        let bytes = report.memory() + statement.len();
        Self {
            report,
            statement,
            range,
            pinned: false,
            bytes,
            facts: None,
            fingerprint: 0,
            at: String::new(),
            elapsed: Duration::ZERO,
            stats_asked: false,
        }
    }

    pub fn id(&self) -> ProfileId {
        self.report.profile.id
    }
}

/// A profile kept, as "Compare ▾" lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileBrief {
    pub id: ProfileId,
    pub at: String,
    pub elapsed: Duration,
    /// The statement's time (the engine's, else the runner's), when known.
    pub time_ms: Option<f64>,
    pub fingerprint: u64,
    /// The statement, on one line.
    pub statement: String,
    pub analyze: bool,
    pub pinned: bool,
}

impl ProfileBrief {
    pub fn of(p: &StoredProfile) -> Self {
        Self {
            id: p.id(),
            at: p.at.clone(),
            elapsed: p.elapsed,
            time_ms: p.report.totals.time_ms.or(p.report.profile.statement.wall_ms),
            fingerprint: p.fingerprint,
            statement: kernel::text::one_line(&p.statement).into_owned(),
            analyze: p.report.profile.depth.runs(),
            pinned: p.pinned,
        }
    }
}

/// A console's profiles.
#[derive(Debug, Default)]
pub struct Profiles {
    list: Vec<StoredProfile>,
    /// Bumped whenever one comes, goes or is pinned.
    pub generation: u64,
}

impl Profiles {
    /// Keeps `profile` (the newest), then trims to `keep` profiles and `max_bytes`.
    pub fn push(&mut self, profile: StoredProfile, keep: usize, max_bytes: usize) {
        self.list.push(profile);
        self.trim(keep, max_bytes);
        self.generation += 1;
    }

    /// The oldest unpinned let go while past `keep` or `max_bytes` (the newest stays, even
    /// alone past them).
    pub fn trim(&mut self, keep: usize, max_bytes: usize) {
        loop {
            let over = self.list.len() > keep.max(1) || self.bytes() > max_bytes;
            if !over {
                return;
            }
            let newest = self.list.len().saturating_sub(1);
            let Some(at) = (0..newest).find(|&i| !self.list[i].pinned) else { return };
            self.list.remove(at);
            self.generation += 1;
        }
    }

    pub fn bytes(&self) -> usize {
        self.list.iter().map(|p| p.bytes).sum()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, id: ProfileId) -> Option<&StoredProfile> {
        self.list.iter().find(|p| p.id() == id)
    }

    pub fn get_mut(&mut self, id: ProfileId) -> Option<&mut StoredProfile> {
        self.list.iter_mut().find(|p| p.id() == id)
    }

    /// The newest.
    pub fn latest(&self) -> Option<&StoredProfile> {
        self.list.last()
    }

    /// Oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &StoredProfile> {
        self.list.iter()
    }

    pub fn pin(&mut self, id: ProfileId, pinned: bool) {
        if let Some(p) = self.list.iter_mut().find(|p| p.id() == id) {
            p.pinned = pinned;
            self.generation += 1;
        }
    }

    pub fn remove(&mut self, id: ProfileId) {
        self.list.retain(|p| p.id() != id);
        self.generation += 1;
    }

    /// "Clear and Rebuild Caches": the unpinned go.
    pub fn purge(&mut self) {
        let before = self.list.len();
        self.list.retain(|p| p.pinned);
        if self.list.len() != before {
            self.generation += 1;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/store.rs"]
mod tests;
