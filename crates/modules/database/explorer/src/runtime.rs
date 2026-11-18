//! Where the module's work runs and what time it is: the project's pool `db` and the
//! system's clock in the app; by hand in the tests (each job run when the test says, the time set by
//! it), so what they check never depends on threads or timing.

use std::sync::Arc;
use std::time::Instant;

/// A job for the pool.
pub type Job = Box<dyn FnOnce() + Send>;

/// How jobs are run and the time read (cloning is cheap).
#[derive(Clone)]
pub struct Runtime {
    spawn: Arc<dyn Fn(Job) + Send + Sync>,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish_non_exhaustive()
    }
}

impl Runtime {
    /// The app's: jobs on a pool `db` of its own (threads made as work comes, gone when
    /// idle), the system's clock. One a project (each project's sources make theirs): a
    /// server that doesn't answer in another project never queues this one's work.
    pub fn pool() -> Self {
        use kernel::work::{IDLE, Pool, Priority, default_limit};
        let pool = Pool::new("db", default_limit(), IDLE);
        Self::new(move |job| pool.spawn(Priority::Interactive, None, job), Instant::now)
    }

    pub fn new(
        spawn: impl Fn(Job) + Send + Sync + 'static,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Self {
        Self { spawn: Arc::new(spawn), now: Arc::new(now) }
    }

    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        (self.spawn)(Box::new(job));
    }

    pub fn now(&self) -> Instant {
        (self.now)()
    }
}
