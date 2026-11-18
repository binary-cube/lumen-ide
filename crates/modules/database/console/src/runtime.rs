//! Where a console's work runs and what time it is: the project's pools `db-console` (and
//! `db-schema-index` for the schema read for completion) and the system's
//! clocks in the app (the local time of day); by hand in the tests (each job run when the test says, the time set by
//! it), so what they check never depends on threads or timing.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// A job for the pool.
pub type Job = Box<dyn FnOnce() + Send>;

/// How jobs are run and the time read (cloning is cheap).
#[derive(Clone)]
pub struct Runtime {
    spawn: Arc<dyn Fn(Job) + Send + Sync>,
    /// Where a source's schema is read for completion (the pool `db-schema-index`).
    index: Arc<dyn Fn(Job) + Send + Sync>,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
    /// The time of day the Output's lines are stamped with.
    clock: Arc<dyn Fn() -> String + Send + Sync>,
    /// The time a history's entry is stamped with (milliseconds since the Unix epoch).
    wall: Arc<dyn Fn() -> i64 + Send + Sync>,
    /// What an engine measures itself (a profile's times) replaced by fixed figures: the
    /// snapshots' (the same picture every run).
    frozen: bool,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish_non_exhaustive()
    }
}

impl Runtime {
    /// The app's: jobs on a pool `db-console` of its own (threads made as work comes, gone
    /// when idle), the system's clocks. One a project (each project's consoles make theirs):
    /// another project's long queries never queue this one's.
    pub fn pool() -> Self {
        use kernel::work::{IDLE, Pool, Priority, default_limit};
        let consoles = Pool::new("db-console", default_limit(), IDLE);
        let index = Pool::new("db-schema-index", default_limit(), IDLE);
        let mut runtime = Self::new(
            move |job| consoles.spawn(Priority::Interactive, None, job),
            Instant::now,
            || clock_text(SystemTime::now()),
        );
        runtime.index = Arc::new(move |job| index.spawn(Priority::Interactive, None, job));
        runtime
    }

    pub fn new(
        spawn: impl Fn(Job) + Send + Sync + 'static,
        now: impl Fn() -> Instant + Send + Sync + 'static,
        clock: impl Fn() -> String + Send + Sync + 'static,
    ) -> Self {
        let spawn: Arc<dyn Fn(Job) + Send + Sync> = Arc::new(spawn);
        Self {
            index: Arc::clone(&spawn),
            spawn,
            now: Arc::new(now),
            clock: Arc::new(clock),
            wall: Arc::new(|| epoch_ms(SystemTime::now())),
            frozen: false,
        }
    }

    /// The same, the history's entries stamped by `wall` (the tests', the snapshots').
    pub fn with_wall(mut self, wall: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.wall = Arc::new(wall);
        self
    }

    /// The same, the engines' own measures frozen too (the snapshots').
    #[cfg(any(test, feature = "snapshot"))]
    pub fn frozen(mut self) -> Self {
        self.frozen = true;
        self
    }

    /// The engines' own measures are frozen.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        (self.spawn)(Box::new(job));
    }

    /// A schema index's work (reading a source's catalog): its own pool in the app, the
    /// same as [`Self::spawn`] elsewhere.
    pub fn spawn_index(&self, job: impl FnOnce() + Send + 'static) {
        (self.index)(Box::new(job));
    }

    pub fn now(&self) -> Instant {
        (self.now)()
    }

    /// The time of day, as the Output's lines show it.
    pub fn clock(&self) -> String {
        (self.clock)()
    }

    /// Now, as a history's entry is stamped (milliseconds since the Unix epoch).
    pub fn wall_ms(&self) -> i64 {
        (self.wall)()
    }

    /// An entry's time as the history shows it: the local time of day (`10:00:23`) when it's
    /// today's, else its day too (`2026-10-30 18:12`); in UTC when frozen (the snapshots':
    /// the same text on any machine).
    pub fn time_text(&self, at_ms: i64) -> String {
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(at_ms.max(0) as u64);
        let now =
            SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(self.wall_ms().max(0) as u64);
        if self.frozen {
            let (at, now): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
                (at.into(), now.into());
            return day_text(at.date_naive() == now.date_naive(), &at);
        }
        let (at, now): (chrono::DateTime<chrono::Local>, chrono::DateTime<chrono::Local>) =
            (at.into(), now.into());
        day_text(at.date_naive() == now.date_naive(), &at)
    }
}

fn day_text<Tz: chrono::TimeZone>(today: bool, at: &chrono::DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if today { at.format("%H:%M:%S").to_string() } else { at.format("%Y-%m-%d %H:%M").to_string() }
}

/// `at` in milliseconds since the Unix epoch.
pub fn epoch_ms(at: SystemTime) -> i64 {
    at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// `at` as the Output stamps its lines, in the local time of day: `2026-10-31 10:00:23.698`.
pub fn clock_text(at: SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = at.into();
    local.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// `at` as [`clock_text`] writes it, in UTC (the tests': the same text on any machine).
pub fn utc_text(at: SystemTime) -> String {
    let utc: chrono::DateTime<chrono::Utc> = at.into();
    utc.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

#[cfg(test)]
#[path = "../tests/unit/runtime.rs"]
mod tests;
