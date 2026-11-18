//! Background workers: a bounded pool instead of a thread per job. Each kind of work has
//! its pool, named (its threads' CPU is shown under that name in the Task Manager). Threads
//! start when work arrives (never more than [`Pool::limit`]) and end after [`IDLE`] without
//! work, so an idle app holds none. What the user waits for ([`Priority::Interactive`]) goes
//! before the rest (indexing, blame…).
//!
//! A job that may become useless (its project closed, a newer request) takes a [`Cancel`]:
//! cancelled before it starts, it doesn't run; while running, it can check it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// How long a worker waits for work before it ends.
pub const IDLE: Duration = Duration::from_secs(30);

/// What a job is for: the interactive ones first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// The user waits for it (an opened file's blame, a command's result).
    Interactive,
    /// Nobody waits for it right now (indexing, prefetching).
    Background,
}

/// A job's cancellation, shared by whoever may cancel it and the job. Cloning is cheap.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

type Task = (Option<Cancel>, Box<dyn FnOnce() + Send>);

#[derive(Default)]
struct State {
    interactive: VecDeque<Task>,
    background: VecDeque<Task>,
    /// Workers alive, and how many of them wait for work.
    workers: usize,
    idle: usize,
}

impl State {
    fn next(&mut self) -> Option<Task> {
        self.interactive.pop_front().or_else(|| self.background.pop_front())
    }

    fn waiting(&self) -> usize {
        self.interactive.len() + self.background.len()
    }
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    limit: usize,
    idle: Duration,
    name: &'static str,
}

/// A bounded pool of named worker threads (see the module's doc).
#[derive(Clone)]
pub struct Pool {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool").field("limit", &self.shared.limit).finish_non_exhaustive()
    }
}

/// A pool's usual limit: the cores but one (the UI's), at least 2, at most 8.
pub fn default_limit() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    cores.saturating_sub(1).clamp(2, 8)
}

/// Runs `job` on the shared pool named `name` (made at its first job, at most
/// [`default_limit`] threads, each ending when idle): for one-off work a view or a command
/// starts — a log page, a diff, a search — instead of a thread each time. Its threads bear
/// the name (the Task Manager shows their CPU under it).
pub fn run(name: &'static str, job: impl FnOnce() + Send + 'static) {
    static POOLS: Mutex<Vec<(&'static str, Pool)>> = Mutex::new(Vec::new());
    let pool = {
        let mut pools = POOLS.lock().unwrap_or_else(PoisonError::into_inner);
        match pools.iter().find(|(n, _)| *n == name) {
            Some((_, pool)) => pool.clone(),
            None => {
                let pool = Pool::new(name, default_limit(), IDLE);
                pools.push((name, pool.clone()));
                pool
            }
        }
    };
    pool.spawn(Priority::Interactive, None, job);
}

impl Pool {
    /// At most `limit` threads named `name`, each ending after `idle` without work.
    pub fn new(name: &'static str, limit: usize, idle: Duration) -> Self {
        let shared = Shared {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            limit: limit.max(1),
            idle,
            name,
        };
        Self { shared: Arc::new(shared) }
    }

    pub fn limit(&self) -> usize {
        self.shared.limit
    }

    /// Workers alive right now (for the Task Manager, for tests).
    pub fn workers(&self) -> usize {
        self.lock().workers
    }

    /// Runs `job` on a worker; with `cancel`, not at all if it's cancelled before it starts.
    pub fn spawn(
        &self,
        priority: Priority,
        cancel: Option<&Cancel>,
        job: impl FnOnce() + Send + 'static,
    ) {
        let mut state = self.lock();
        let task = (cancel.cloned(), Box::new(job) as Box<dyn FnOnce() + Send>);
        match priority {
            Priority::Interactive => state.interactive.push_back(task),
            Priority::Background => state.background.push_back(task),
        }
        // A waiting worker takes it; otherwise one more, while under the limit.
        if state.idle >= state.waiting() {
            self.shared.ready.notify_one();
            return;
        }
        if state.workers < self.shared.limit && self.start() {
            state.workers += 1;
        } else {
            self.shared.ready.notify_one();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One more worker. A failed start (the system out of threads) leaves the job to the
    /// workers alive.
    fn start(&self) -> bool {
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name(self.shared.name.into())
            .spawn(move || work(&shared))
            .is_ok()
    }
}

fn work(shared: &Shared) {
    let lock = || shared.state.lock().unwrap_or_else(PoisonError::into_inner);
    let mut state = lock();
    loop {
        let Some((cancel, job)) = state.next() else {
            state.idle += 1;
            let (next, timeout) = shared
                .ready
                .wait_timeout_while(state, shared.idle, |s| s.waiting() == 0)
                .unwrap_or_else(PoisonError::into_inner);
            state = next;
            state.idle -= 1;
            if timeout.timed_out() && state.waiting() == 0 {
                state.workers -= 1;
                return;
            }
            continue;
        };
        drop(state);
        if !cancel.is_some_and(|c| c.is_cancelled()) {
            // A job that panics doesn't take the worker with it.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
        }
        state = lock();
    }
}

/// Work to do later, on a thread of its own (a pool's would be held meanwhile): dropped
/// before its time, it's cancelled and its thread ends at once.
#[derive(Debug)]
pub struct Later {
    /// Dropped: the waiting thread is told.
    _cancel: std::sync::mpsc::Sender<()>,
    /// When the job runs.
    due: std::time::Instant,
}

impl Later {
    /// `job` after `delay`, on a thread named `name`; `None` if the thread can't start.
    #[allow(clippy::disallowed_methods)]
    pub fn run(
        name: &'static str,
        delay: Duration,
        job: impl FnOnce() + Send + 'static,
    ) -> Option<Self> {
        let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
        let wait = move || {
            if matches!(
                cancelled.recv_timeout(delay),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                job();
            }
        };
        let due = std::time::Instant::now() + delay;
        std::thread::Builder::new().name(name.into()).spawn(wait).ok()?;
        Some(Self { _cancel: cancel, due })
    }

    /// When the job runs.
    pub fn due(&self) -> std::time::Instant {
        self.due
    }

    /// `job` after `delay` in `slot`, unless the wait there already ends sooner (two reasons
    /// for the same work: the first one due does it).
    pub fn sooner(
        slot: &mut Option<Self>,
        name: &'static str,
        delay: Duration,
        job: impl FnOnce() + Send + 'static,
    ) {
        let due = std::time::Instant::now() + delay;
        if slot.as_ref().is_none_or(|waiting| waiting.due > due) {
            *slot = Self::run(name, delay, job);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/work.rs"]
mod tests;
