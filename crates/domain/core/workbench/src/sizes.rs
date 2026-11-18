//! The sizes the user gave a project's parts by dragging them — a dock's width or height,
//! a splitter's place in a view (the data view's tables list, the profile's panel, the Task
//! Manager's detail, the comparison's columns, the Git log's panels) — kept per project:
//! made with the project (`ProjectData`), saved with its session and given back when it
//! opens again; another project has its own. Views get the handle when they're made
//! (`Viewer::set_data` / `ToolPanel::set_data` with a [`LayoutSizes`]), read their size
//! from it and write it when the user lets go of a splitter. A size never dragged is absent:
//! the part's own default applies.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

/// The left dock's width (logical pixels): every tool window shown there.
pub const DOCK_LEFT: &str = "dock.left";
/// The right dock's width (logical pixels).
pub const DOCK_RIGHT: &str = "dock.right";
/// The bottom dock's height (logical pixels): the terminal, Services, Remote… share it.
pub const DOCK_BOTTOM: &str = "dock.bottom";

/// The share of a preview tab's width its text has in Split mode, for the tabs of the
/// viewer `provider` opened from now on (the last one the user chose; absent: half).
pub fn preview_split(provider: &str) -> String {
    format!("preview-split.{provider}")
}

/// A project's dragged sizes, by key (`dock.bottom`, `database.tables-list`): a cheap,
/// shared handle (clones see the same values).
#[derive(Clone, Default)]
pub struct LayoutSizes {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    values: BTreeMap<String, f32>,
    /// Changed on every change (what follows it reads again only then).
    generation: u64,
}

impl std::fmt::Debug for LayoutSizes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.entries()).finish()
    }
}

impl LayoutSizes {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The size kept for `key` (`None`: never dragged).
    pub fn get(&self, key: &str) -> Option<f32> {
        self.lock().values.get(key).copied()
    }

    /// Keeps `value` for `key` (a value that isn't a finite number is ignored).
    pub fn set(&self, key: &str, value: f32) {
        if !value.is_finite() {
            return;
        }
        let mut inner = self.lock();
        if inner.values.get(key) != Some(&value) {
            inner.values.insert(key.to_owned(), value);
            inner.generation += 1;
        }
    }

    /// Forgets `key`'s size: its default applies again (a separator double-clicked).
    pub fn remove(&self, key: &str) {
        let mut inner = self.lock();
        if inner.values.remove(key).is_some() {
            inner.generation += 1;
        }
    }

    /// Every size kept, sorted by key (what the session saves).
    pub fn entries(&self) -> Vec<(String, f32)> {
        self.lock().values.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    /// The sizes of a session read back, in place of what was kept.
    pub fn replace(&self, entries: &[(String, f32)]) {
        let mut inner = self.lock();
        inner.values =
            entries.iter().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k.clone(), *v)).collect();
        inner.generation += 1;
    }

    /// Changes on every change.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }
}

#[cfg(test)]
#[path = "../tests/unit/sizes.rs"]
mod tests;
