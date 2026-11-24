//! What the settings' block shows that the disk says (a tool the project has, what a run
//! would start): looked at in the background, kept a while, the settings woken when it's
//! there. A block is built on the UI thread, which never reads the disk.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A look older than this is made again (the project's tools change as it's worked on).
const FRESH: Duration = Duration::from_secs(10);

/// How many looks are kept at most (a block asks a few; old ones go first).
const MOST: usize = 64;

/// The looks being made now (all of them: what a test waits for).
static LOOKING: AtomicUsize = AtomicUsize::new(0);

/// A look finished since the settings last asked.
static LOOKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a look finished since the last call (the block shows it then).
pub(crate) fn take_looked() -> bool {
    LOOKED.swap(false, Ordering::AcqRel)
}

/// Some look is being made.
#[cfg(test)]
pub(crate) fn busy() -> bool {
    LOOKING.load(Ordering::Acquire) > 0
}

/// The looks made, by what they're of (`None`: being made, nothing known yet).
pub(crate) struct Looked<V> {
    known: Mutex<HashMap<String, (Option<V>, Instant)>>,
}

impl<V: Clone + Send + 'static> Looked<V> {
    pub(crate) fn new() -> Self {
        Self { known: Mutex::new(HashMap::new()) }
    }

    /// Everything looked at forgotten (a test's disk changed under it).
    #[cfg(test)]
    pub(crate) fn forget(&self) {
        self.known.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    /// What `key` was found to be, from memory; looked at again with `look` on a worker
    /// when old or never (`wake` called once it's there). `None`: not known yet.
    pub(crate) fn get(
        self: &Arc<Self>,
        key: String,
        wake: Option<&Arc<dyn Fn() + Send + Sync>>,
        look: impl FnOnce() -> V + Send + 'static,
    ) -> Option<V> {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let had = known.get(&key).cloned();
        if let Some((value, at)) = &had
            && at.elapsed() < FRESH
        {
            return value.clone();
        }
        let value = had.and_then(|(value, _)| value);
        if known.len() >= MOST {
            let oldest = known.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                known.remove(&oldest);
            }
        }
        // Being looked at from now on (the old value shown meanwhile).
        known.insert(key.clone(), (value.clone(), Instant::now()));
        drop(known);
        let (me, wake) = (Arc::clone(self), wake.cloned());
        LOOKING.fetch_add(1, Ordering::AcqRel);
        kernel::work::run("quality-look", move || {
            let found = look();
            me.known
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, (Some(found), Instant::now()));
            LOOKING.fetch_sub(1, Ordering::AcqRel);
            LOOKED.store(true, Ordering::Release);
            if let Some(wake) = wake {
                wake();
            }
        });
        value
    }
}
