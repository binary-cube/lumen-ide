//! The pages of one table's view (one order, one filter) kept in memory, within a limit of
//! bytes: past it, the pages farthest from the one just read go first (out of view, the
//! last to be asked again). Built on [`kernel::GenerationalCache`]; let go whole on "Clear
//! caches" ([`Purge`]) or when the order or the filter changes.

use std::sync::Arc;

use database::Rows;
use kernel::{GenerationalCache, Purge};

/// The pages kept by default: 32 MB (a few thousand rows of a wide table).
pub const DEFAULT_LIMIT: usize = 32 * 1024 * 1024;

/// A page's place: its first row and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageKey {
    pub offset: u64,
    pub limit: u32,
}

#[derive(Debug)]
pub struct PageCache {
    /// Each page with its bytes (counted once).
    pages: GenerationalCache<PageKey, (Arc<Rows>, usize)>,
    bytes: usize,
    limit: usize,
}

impl PageCache {
    /// Pages up to `limit` bytes (one page is kept whatever its size: the one shown).
    pub fn new(limit: usize) -> Self {
        // The bytes decide; the count only bounds the generations' maps.
        Self { pages: GenerationalCache::new(4096), bytes: 0, limit }
    }

    pub fn get(&mut self, key: PageKey) -> Option<Arc<Rows>> {
        self.pages.get(&key).map(|(rows, _)| Arc::clone(rows))
    }

    pub fn insert(&mut self, key: PageKey, rows: Arc<Rows>) {
        let bytes = rows.bytes();
        self.pages.insert(key, (rows, bytes));
        // Overflowing generations drop entries without telling: the bytes are counted
        // again from what's left.
        self.bytes = self.pages.values().map(|(_, b)| b).sum();
        while self.bytes > self.limit && self.pages.len() > 1 {
            let mut farthest: Option<(u64, PageKey)> = None;
            self.pages.retain(|k, _| {
                let distance = k.offset.abs_diff(key.offset);
                if *k != key && farthest.is_none_or(|(d, _)| distance > d) {
                    farthest = Some((distance, *k));
                }
                true
            });
            let Some((_, gone)) = farthest else { break };
            self.pages.retain(|k, _| *k != gone);
            self.bytes = self.pages.values().map(|(_, b)| b).sum();
        }
    }

    /// The bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

impl Purge for PageCache {
    fn purge(&mut self) {
        self.pages.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
#[path = "../tests/unit/cache.rs"]
mod tests;
