//! The start-up's phases, timed: each one logged once (`startup phase`, with the time since
//! the process's `main` began), so a slow start shows where it went (`RUST_LOG=info`).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// When `main` began (set once, first thing).
static STARTED: OnceLock<Instant> = OnceLock::new();

/// The start-up's clock starts (`main`'s first line); later calls keep the first moment.
pub fn begin() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

/// The time since `main` began.
pub fn elapsed() -> Duration {
    begin().elapsed()
}

/// A phase of the start-up is done: logged with the time since `main` began.
pub fn mark(phase: &'static str) {
    tracing::info!(phase, elapsed = ?elapsed(), "startup phase");
}

/// A phase of the start-up timed on its own: logged with how long it took and when it ended.
pub fn timed<R>(phase: &'static str, f: impl FnOnce() -> R) -> R {
    let start = Instant::now();
    let result = f();
    tracing::info!(phase, took = ?start.elapsed(), elapsed = ?elapsed(), "startup phase");
    result
}
