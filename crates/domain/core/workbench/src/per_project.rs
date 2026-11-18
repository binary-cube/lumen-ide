//! The state a module keeps per open project, reachable from a command's host or a view: held
//! by the project's own subscriptions (gone with it), found here by the project's identity.

use std::sync::{Arc, Mutex, PoisonError, Weak};

use kernel::ProjectId;

/// Each open project's `T`.
#[derive(Debug)]
pub struct PerProject<T>(Mutex<Vec<(ProjectId, Weak<T>)>>);

impl<T> PerProject<T> {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(ProjectId, Weak<T>)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Project `id`'s, from now on (those gone are forgotten).
    pub fn insert(&self, id: ProjectId, value: &Arc<T>) {
        let mut all = self.lock();
        all.retain(|(_, v)| v.strong_count() > 0);
        all.push((id, Arc::downgrade(value)));
    }

    /// Project `id`'s, while it's open.
    pub fn get(&self, id: ProjectId) -> Option<Arc<T>> {
        self.lock().iter().find(|(p, _)| *p == id).and_then(|(_, v)| v.upgrade())
    }

    /// Project `id` is gone.
    pub fn remove(&self, id: ProjectId) {
        self.lock().retain(|(p, _)| *p != id);
    }

    /// Every open project's.
    pub fn all(&self) -> Vec<Arc<T>> {
        self.lock().iter().filter_map(|(_, v)| v.upgrade()).collect()
    }
}

impl<T> Default for PerProject<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/unit/per_project.rs"]
mod tests;
