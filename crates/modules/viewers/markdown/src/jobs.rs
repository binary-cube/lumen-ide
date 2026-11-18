//! The heavy blocks' work (a code block's highlighting, a diagram's layout, a formula's),
//! done on a pool of the module's own, never while painting: a block is looked up by the
//! hash of what it's made from, its result kept while the document uses it. Until it
//! arrives the layout makes do without it (plain code, a placeholder), then is built again.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use kernel::GenerationalCache;

/// The threads doing it.
pub const RENDER_THREAD: &str = "markdown-render";

/// How many results a cache generation holds.
const LIMIT: usize = 64;

/// What `parts` hash to (a block's key).
pub fn key(parts: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// The results by key, and those being worked out.
pub struct Jobs<V> {
    done: GenerationalCache<u64, V>,
    /// The keys the layout being built uses (the rest go when it's done).
    used: HashSet<u64>,
    pending: HashMap<u64, Receiver<V>>,
    pool: kernel::Pool,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Worked out right away, while laying out (tests).
    inline: bool,
}

impl<V: Clone + Send + 'static> Default for Jobs<V> {
    fn default() -> Self {
        Self {
            done: GenerationalCache::new(LIMIT),
            used: HashSet::new(),
            pending: HashMap::new(),
            pool: render_pool(),
            wake: None,
            inline: false,
        }
    }
}

/// The module's render pool, shared by its caches (threads only while there's work).
fn render_pool() -> kernel::Pool {
    static POOL: std::sync::OnceLock<kernel::Pool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| kernel::Pool::new(RENDER_THREAD, 2, kernel::work::IDLE)).clone()
}

impl<V: Clone + Send + 'static> Jobs<V> {
    /// One holding up to `limit` results a generation.
    pub fn with_limit(limit: usize) -> Self {
        Self { done: GenerationalCache::new(limit), ..Self::default() }
    }

    /// Worked out while laying out (tests of what's cached, without waiting).
    #[cfg(test)]
    pub fn inline() -> Self {
        Self { inline: true, ..Self::default() }
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.wake = wake;
    }

    /// Results are being worked out.
    pub fn loading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Takes the results that arrived. `true` if one did (the layout is built again).
    pub fn poll(&mut self) -> bool {
        let mut arrived = Vec::new();
        self.pending.retain(|key, rx| match rx.try_recv() {
            Ok(v) => {
                arrived.push((*key, v));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        let any = !arrived.is_empty();
        for (key, v) in arrived {
            self.done.insert(key, v);
        }
        any
    }

    /// A layout starts: what it uses is recorded.
    pub fn begin(&mut self) {
        self.used.clear();
    }

    /// The layout is done: the results it didn't use go.
    pub fn end(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.done.retain(|key, _| used.contains(key));
        self.used = used;
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.used.clear();
        self.pending.clear();
    }

    /// `key`'s result if it's there (nothing asked for).
    pub fn peek(&mut self, key: u64) -> Option<V> {
        self.used.insert(key);
        self.done.get(&key).cloned()
    }

    /// Forgets `key`'s result (worked out again when asked).
    pub fn remove(&mut self, key: u64) {
        self.done.retain(|k, _| *k != key);
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.done.values()
    }

    /// The result for `key`, if it's there; otherwise `work` is sent to the pool (once)
    /// and `None` comes back until it's done.
    pub fn get(&mut self, key: u64, work: impl FnOnce() -> V + Send + 'static) -> Option<V> {
        self.used.insert(key);
        if let Some(v) = self.done.get(&key) {
            return Some(v.clone());
        }
        if self.inline {
            let v = work();
            self.done.insert(key, v.clone());
            return Some(v);
        }
        if !self.pending.contains_key(&key) {
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            self.pool.spawn(kernel::Priority::Interactive, None, move || {
                let _ = tx.send(work());
                if let Some(wake) = wake {
                    wake();
                }
            });
            self.pending.insert(key, rx);
        }
        None
    }
}
