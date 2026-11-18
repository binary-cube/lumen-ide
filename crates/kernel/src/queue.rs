//! A queue that merges what's asked of it, for work that's costly to repeat (reindexing a
//! file): a key already waiting isn't added again; "everything" swallows the keys; the work
//! waits for the requests to calm down (`quiet`), but never longer than `max_wait` after the
//! first one; while a batch of changes is open (a paste of many files, `hold`), nothing is
//! handed out — it all comes out as one batch when the last hold is released (or, if one is
//! never released, after `hold_limit`).
//!
//! [`Coalescing`] is the plain structure, the time given to it (tested without waiting);
//! [`SharedQueue`] wraps it for a worker thread that sleeps until a batch is ready.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What a batch asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Batch<K> {
    /// Everything (the keys asked meanwhile included).
    All,
    /// These keys, once each, in the order first asked.
    Keys(Vec<K>),
}

/// How much was merged (for the Task Manager, for tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueStats {
    /// Requests received (keys, "everything").
    pub received: u64,
    /// Requests that added nothing (already waiting, or swallowed by "everything").
    pub merged: u64,
    /// Batches handed out.
    pub batches: u64,
}

#[derive(Debug)]
pub struct Coalescing<K> {
    keys: Vec<K>,
    waiting: HashSet<K>,
    all: bool,
    /// Asked for at once (no waiting for quiet): what's waiting goes with it.
    urgent: bool,
    /// Asked for by the user (see [`Self::push_forced`]): not even an open batch holds
    /// these back (only these: what else waits stays held). In the order asked, and as a set
    /// (thousands asked at once: each looked for at once).
    forced: Vec<K>,
    forced_known: HashSet<K>,
    first: Option<Instant>,
    last: Option<Instant>,
    holds: HashMap<u128, Instant>,
    quiet: Duration,
    max_wait: Duration,
    hold_limit: Duration,
    stats: QueueStats,
}

impl<K: Eq + Hash + Clone> Coalescing<K> {
    pub fn new(quiet: Duration, max_wait: Duration, hold_limit: Duration) -> Self {
        Self {
            keys: Vec::new(),
            waiting: HashSet::new(),
            all: false,
            urgent: false,
            forced: Vec::new(),
            forced_known: HashSet::new(),
            first: None,
            last: None,
            holds: HashMap::new(),
            quiet,
            max_wait,
            hold_limit,
            stats: QueueStats::default(),
        }
    }

    fn asked(&mut self, now: Instant) {
        self.stats.received += 1;
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// Asks for `key`. `false`: it added nothing (already waiting, or everything is).
    pub fn push(&mut self, key: K, now: Instant) -> bool {
        self.asked(now);
        if self.all || self.waiting.contains(&key) {
            self.stats.merged += 1;
            return false;
        }
        self.waiting.insert(key.clone());
        self.keys.push(key);
        true
    }

    /// Asks for `key` at once: the batch (with what's waiting) is ready now, unless a batch of
    /// changes is open.
    pub fn push_urgent(&mut self, key: K, now: Instant) -> bool {
        self.urgent = true;
        self.push(key, now)
    }

    /// Asks for `key` at once even while a batch of changes is open (the user asked for it:
    /// an upload clicked). During the batch only the keys asked so go (the batch's own,
    /// half written, stay held); outside one, what's waiting goes with them.
    pub fn push_forced(&mut self, key: K, now: Instant) -> bool {
        if self.forced_known.insert(key.clone()) {
            self.forced.push(key.clone());
        }
        self.push_urgent(key, now)
    }

    /// Asks for everything: the keys waiting are swallowed.
    pub fn push_all(&mut self, now: Instant) {
        self.asked(now);
        if self.all {
            self.stats.merged += 1;
        }
        self.stats.merged += self.keys.len() as u64;
        self.all = true;
        self.keys.clear();
        self.waiting.clear();
    }

    /// A batch of changes starts (`id`: its identity): nothing is handed out until it ends.
    pub fn hold(&mut self, id: u128, now: Instant) {
        self.holds.insert(id, now);
    }

    /// The batch `id` ended; the work waits for the quiet after it (its limit counted from
    /// now too: what came during the batch is new).
    pub fn release(&mut self, id: u128, now: Instant) {
        if self.holds.remove(&id).is_some() && !self.is_empty() {
            self.first = Some(now);
            self.last = Some(now);
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.all && self.keys.is_empty()
    }

    /// The keys waiting (0 while everything is asked for).
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// A batch of changes is open (not past its limit).
    fn held(&self, now: Instant) -> bool {
        self.holds.values().any(|&since| now.saturating_duration_since(since) < self.hold_limit)
    }

    /// When the next batch is ready: `None` if there's nothing, or a batch of changes is open
    /// (then: when its limit passes).
    pub fn ready_at(&self, now: Instant) -> Option<Instant> {
        let (first, last) = (self.first?, self.last?);
        if self.is_empty() {
            return None;
        }
        if self.held(now) {
            if self.forced.is_empty() {
                return self.holds.values().map(|&since| since + self.hold_limit).max();
            }
            return Some(first);
        }
        if self.urgent {
            return Some(first);
        }
        Some((last + self.quiet).min(first + self.max_wait))
    }

    /// The batch, if it's ready at `now`; the queue starts empty again.
    pub fn take(&mut self, now: Instant) -> Option<Batch<K>> {
        if self.held(now) {
            return self.take_forced();
        }
        if self.ready_at(now)? > now {
            return None;
        }
        self.holds.retain(|_, &mut since| now.saturating_duration_since(since) < self.hold_limit);
        self.first = None;
        self.last = None;
        self.urgent = false;
        self.forced.clear();
        self.forced_known.clear();
        self.waiting.clear();
        self.stats.batches += 1;
        Some(if std::mem::take(&mut self.all) {
            self.keys.clear();
            Batch::All
        } else {
            Batch::Keys(std::mem::take(&mut self.keys))
        })
    }

    /// During a batch of changes: the keys the user asked for, alone (the rest stays held,
    /// its times kept).
    fn take_forced(&mut self) -> Option<Batch<K>> {
        if self.forced.is_empty() {
            return None;
        }
        let forced = std::mem::take(&mut self.forced);
        self.forced_known.clear();
        let gone: HashSet<&K> = forced.iter().collect();
        self.keys.retain(|k| !gone.contains(k));
        self.waiting.retain(|k| !gone.contains(k));
        // What stays goes after the batch's quiet, as any batch's (not at once).
        self.urgent = false;
        if self.is_empty() {
            (self.first, self.last) = (None, None);
        }
        self.stats.batches += 1;
        Some(Batch::Keys(forced))
    }

    pub fn stats(&self) -> QueueStats {
        self.stats
    }
}

/// [`Coalescing`] shared with a worker thread: requests from any thread, the worker sleeping
/// in [`Self::next`] until a batch is ready (or the queue is closed).
#[derive(Debug)]
pub struct SharedQueue<K> {
    inner: Arc<(Mutex<State<K>>, Condvar)>,
}

#[derive(Debug)]
struct State<K> {
    queue: Coalescing<K>,
    closed: bool,
}

impl<K> Clone for SharedQueue<K> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl<K: Eq + Hash + Clone> SharedQueue<K> {
    pub fn new(quiet: Duration, max_wait: Duration, hold_limit: Duration) -> Self {
        let state = State { queue: Coalescing::new(quiet, max_wait, hold_limit), closed: false };
        Self { inner: Arc::new((Mutex::new(state), Condvar::new())) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Coalescing<K>, Instant) -> R) -> Option<R> {
        let (lock, wake) = &*self.inner;
        let mut state = lock.lock().ok()?;
        let result = f(&mut state.queue, Instant::now());
        wake.notify_all();
        Some(result)
    }

    /// [`Self::with`] for a look that changes nothing: the worker isn't woken for it.
    fn peek<R>(&self, f: impl FnOnce(&Coalescing<K>) -> R) -> Option<R> {
        let (lock, _) = &*self.inner;
        let state = lock.lock().ok()?;
        Some(f(&state.queue))
    }

    pub fn push(&self, key: K) {
        self.with(|q, now| q.push(key, now));
    }

    pub fn push_many(&self, keys: impl IntoIterator<Item = K>) {
        self.with(|q, now| {
            for key in keys {
                q.push(key, now);
            }
        });
    }

    pub fn push_all(&self) {
        self.with(|q, now| q.push_all(now));
    }

    /// Asked for by the user, even during a batch (see [`Coalescing::push_forced`]).
    pub fn push_forced(&self, keys: impl IntoIterator<Item = K>) {
        self.with(|q, now| {
            for key in keys {
                q.push_forced(key, now);
            }
        });
    }

    /// Asked for at once (see [`Coalescing::push_urgent`]).
    pub fn push_urgent(&self, keys: impl IntoIterator<Item = K>) {
        self.with(|q, now| {
            for key in keys {
                q.push_urgent(key, now);
            }
        });
    }

    pub fn hold(&self, id: u128) {
        self.with(|q, now| q.hold(id, now));
    }

    pub fn release(&self, id: u128) {
        self.with(|q, now| q.release(id, now));
    }

    pub fn stats(&self) -> QueueStats {
        self.peek(Coalescing::stats).unwrap_or_default()
    }

    /// The keys waiting.
    pub fn len(&self) -> usize {
        self.peek(Coalescing::len).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.peek(Coalescing::is_empty).unwrap_or(true)
    }

    /// No more batches: [`Self::next`] returns `None` (the worker ends).
    pub fn close(&self) {
        let (lock, wake) = &*self.inner;
        if let Ok(mut state) = lock.lock() {
            state.closed = true;
        }
        wake.notify_all();
    }

    /// The next batch, waiting (asleep) until one is ready; `None` once closed.
    pub fn next(&self) -> Option<Batch<K>> {
        let (lock, wake) = &*self.inner;
        let mut state = lock.lock().ok()?;
        loop {
            if state.closed {
                return None;
            }
            let now = Instant::now();
            if let Some(batch) = state.queue.take(now) {
                return Some(batch);
            }
            state = match state.queue.ready_at(now) {
                Some(at) => wake.wait_timeout(state, at.saturating_duration_since(now)).ok()?.0,
                None => wake.wait(state).ok()?,
            };
        }
    }
}

/// The latest job per key (a document's newest version), run on a pool's thread (one at a
/// time, in the background; the thread ends when idle): a job asked again before it ran is
/// replaced, not run twice. The results are taken on the UI thread ([`Latest::take`]), the
/// UI woken when some are ready.
pub struct Latest<K, J, O> {
    shared: Arc<LatestShared<K, J, O>>,
    pool: crate::work::Pool,
}

type Run<J, O> = Box<dyn Fn(J) -> O + Send + Sync>;

struct LatestShared<K, J, O> {
    /// The jobs waiting, and whether a run is on its way to take them.
    pending: Mutex<(HashMap<K, J>, bool)>,
    done: Mutex<Vec<O>>,
    run: Run<J, O>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl<K, J, O> std::fmt::Debug for Latest<K, J, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Latest").finish_non_exhaustive()
    }
}

impl<K, J, O> Latest<K, J, O>
where
    K: Eq + Hash + Send + 'static,
    J: Send + 'static,
    O: Send + 'static,
{
    /// Jobs run by `run` on threads named `name`; `wake` called when results are ready.
    pub fn new(
        name: &'static str,
        run: impl Fn(J) -> O + Send + Sync + 'static,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let shared = LatestShared {
            pending: Mutex::default(),
            done: Mutex::default(),
            run: Box::new(run),
            wake,
        };
        let pool = crate::work::Pool::new(name, 1, crate::work::IDLE);
        Self { shared: Arc::new(shared), pool }
    }

    /// Asks for `job` (replacing one for `key` still waiting).
    pub fn request(&self, key: K, job: J) {
        let mut pending =
            self.shared.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.0.insert(key, job);
        if std::mem::replace(&mut pending.1, true) {
            return;
        }
        drop(pending);
        let shared = Arc::clone(&self.shared);
        self.pool.spawn(crate::work::Priority::Background, None, move || shared.drain());
    }

    /// The results finished since the last call.
    pub fn take(&self) -> Vec<O> {
        std::mem::take(
            &mut *self.shared.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

impl<K, J, O> LatestShared<K, J, O> {
    /// Runs what's waiting, until nothing is (asked meanwhile: run too).
    fn drain(&self) {
        loop {
            let jobs = {
                let mut pending =
                    self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if pending.0.is_empty() {
                    pending.1 = false;
                    return;
                }
                std::mem::take(&mut pending.0)
            };
            for job in jobs.into_values() {
                // One that panics is skipped (`run` says why, if it wants): the others go on,
                // and so does the queue.
                let ran =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.run)(job)));
                if let Ok(out) = ran {
                    self.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(out);
                }
            }
            (self.wake)();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/queue.rs"]
mod tests;
