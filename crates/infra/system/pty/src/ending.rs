//! A session let go ends its program for sure (Unix). Dropping the PTY hangs up on its
//! program (`SIGHUP`), then its reader waits for it to end; a program that catches the
//! hangup and goes on (an agent busy closing its helpers) would hold that reader, its PTY
//! and all its memory for good. So the program and what it started are taken at once, given
//! [`HANGUP_GRACE`] to end by themselves, and only if the program itself still runs then
//! are they — with what it started meanwhile — asked to end and, after [`STOP_GRACE`], made
//! to, on a thread of their own. Jobs the user detached from it go with it then; once the
//! program has ended by itself, nothing it left is touched. On Windows, closing the pseudo
//! console ends what's attached to it and nothing waits on the program.

use std::time::Duration;

use process::descendants::Descendants;

/// How long a program hung up on has to end by itself (a shell takes milliseconds).
pub const HANGUP_GRACE: Duration = Duration::from_secs(2);
/// How long it has once asked to end (`SIGTERM`) before it's made to (`SIGKILL`).
pub const STOP_GRACE: Duration = Duration::from_secs(3);
/// The seconds a start time may be off the clock read around the spawn (both round down).
const START_SLACK: u64 = 1;

/// When a session's program started: between these two readings of the clock (seconds since
/// the epoch), taken around its spawn. A program under its id that started outside them is
/// another one (its id reused), never touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Born {
    pub after: u64,
    pub before: u64,
}

impl Born {
    fn holds(self, start: u64) -> bool {
        start.saturating_add(START_SLACK) >= self.after
            && start <= self.before.saturating_add(START_SLACK)
    }
}

/// Seconds since the epoch now (a program's start as the system's table says it).
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Program `pid`, born within `born`, just hung up on, ended for sure (see the module's
/// doc). Returns at once.
pub(crate) fn see_ended(pid: u32, born: Born) {
    let spawned = std::thread::Builder::new()
        .name("pty-end".into())
        .spawn(move || end_after(pid, born, (HANGUP_GRACE, STOP_GRACE)));
    if let Err(error) = spawned {
        tracing::warn!(%error, "can't watch a terminal's program end");
    }
}

/// Program `pid` and its tree, taken now; if the program (born within `born`) still runs
/// after `hangup`, it and its tree then and before are asked to end, then made to after
/// `grace`. `true` if it had to be stopped.
pub(crate) fn end_after(pid: u32, born: Born, (hangup, grace): (Duration, Duration)) -> bool {
    let taken = Descendants::with_root(pid);
    let Some(root) = taken.found().first().cloned() else { return false };
    if !born.holds(root.start) {
        return false;
    }
    std::thread::sleep(hangup);
    // Only the program itself looked at (one process, not the whole table).
    if taken.first().running().is_empty() {
        return false;
    }
    // What it started meanwhile too, if the id still names it.
    let now = Descendants::with_root(pid);
    let same = now.found().first().is_some_and(|f| f.start == root.start);
    let program = if same { taken.joined(now) } else { taken };
    let forced = program.stop(grace);
    tracing::info!(
        program = %root.name,
        stopped = program.len(),
        forced,
        "a terminal's program went on after its hangup: stopped"
    );
    true
}

#[cfg(test)]
#[path = "../tests/unit/ending.rs"]
mod tests;
