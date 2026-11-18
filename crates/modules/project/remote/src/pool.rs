//! A project's connections to its server, kept and shared: at most `limit` open at once
//! (servers refuse more connections per user), an idle one handed out again instead of a new
//! handshake (SSH, TLS), let go after a while idle (the server would drop it anyway). It is
//! itself a [`Connector`]: whatever asks for a session (an upload, a listing, a review's
//! workers) waits for a free one — nothing else changes for them.
//!
//! A session that failed for the network's sake (a transient error) isn't kept: the next one
//! is opened anew. One thread must not hold a session while asking for another (with a small
//! limit, two such threads would wait on each other): a session is let go before the next.
//!
//! A **cautious** pool (FTP: servers often limit the connections of an address — vsftpd's
//! `max_per_ip`, pure-ftpd's, ProFTPD's `MaxClientsPerHost` — and reset or refuse the extra
//! ones) starts with two and grows, up to its limit, only while the server accepts them; a
//! connection refused for that ([`refused_for_load`]) while others are open lowers it to
//! those (remembered for the server, for the next pool) and waits for one of them instead
//! of failing. [`Resilient`] is a session that, failed for the network's sake, is opened
//! again (waiting longer each time) and the operation tried again.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use remote::application::{Connector, Moved, RemoteFs};
use remote::properties::{Accounts, Algo, Capabilities, Properties, SetProperties};
use remote::{RemoteEntry, RemoteError, Retry};

/// An idle session is let go after this (servers drop idle connections after a minute or so).
const IDLE: Duration = Duration::from_secs(45);
/// A cautious pool's connections at first.
const CAUTIOUS_START: usize = 2;

/// The connections each server (`host:port`) accepted at most, learned when it refused more:
/// a new pool for it starts there.
static ACCEPTED: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

/// What a server accepted at most, if it refused more before.
fn accepted(server: &str) -> Option<usize> {
    ACCEPTED.lock().ok()?.iter().find(|(s, _)| s == server).map(|(_, n)| *n)
}

/// Whether `e` is a server refusing one more connection (too many at once, a reset, a
/// refusal) rather than a wrong password or a missing file.
pub fn refused_for_load(e: &RemoteError) -> bool {
    let text = match e {
        RemoteError::Connect(t) | RemoteError::Server(t) | RemoteError::Auth(t) => t,
        _ => return false,
    };
    let text = text.to_lowercase();
    ["reset", "refused", "421", "too many", "maximum number", "broken pipe", "aborted"]
        .iter()
        .any(|k| text.contains(k))
}

#[derive(Default)]
struct Slots {
    /// Sessions open: handed out or idle.
    open: usize,
    idle: Vec<(Box<dyn RemoteFs>, Instant)>,
}

pub struct Pool {
    connector: Arc<dyn Connector>,
    /// The most open at once (the connection's transfers' limits).
    max: usize,
    /// How many may be open now: `max`, or (cautious) as many as the server accepted so far.
    limit: AtomicUsize,
    /// Cautious: the server (`host:port`) whose connections are counted; `None`: `max` from
    /// the start.
    server: Option<String>,
    /// The server refused one more: the limit doesn't grow again.
    capped: AtomicBool,
    /// Lowered to this after a refusal, not told yet (0: nothing to tell).
    lowered: AtomicUsize,
    slots: Mutex<Slots>,
    freed: Condvar,
    /// How long a session may stay idle ([`IDLE`]; shorter in the tests).
    idle: Duration,
    /// The idle sessions' sweep is waiting (one at a time, ending once none is idle).
    sweeping: AtomicBool,
    /// Where it waits: this project's own (one thread, made when a session is given back,
    /// gone when idle), so another project's sweep never waits for this one's.
    sweeps: kernel::Pool,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool").field("max", &self.max).finish_non_exhaustive()
    }
}

impl Pool {
    pub fn new(connector: Arc<dyn Connector>, limit: usize) -> Arc<Self> {
        Self::make(connector, limit, None)
    }

    /// A cautious pool for `server` (`host:port`): two connections at first (or what the
    /// server accepted before), more only while it accepts them, at most `limit`.
    pub fn cautious(connector: Arc<dyn Connector>, limit: usize, server: String) -> Arc<Self> {
        Self::make(connector, limit, Some(server))
    }

    fn make(connector: Arc<dyn Connector>, limit: usize, server: Option<String>) -> Arc<Self> {
        let max = limit.max(1);
        let known = server.as_deref().and_then(accepted);
        let start = match (&server, known) {
            (Some(_), Some(n)) => n.clamp(1, max),
            (Some(_), None) => CAUTIOUS_START.min(max),
            (None, _) => max,
        };
        Arc::new(Self {
            connector,
            max,
            limit: AtomicUsize::new(start),
            capped: AtomicBool::new(known.is_some()),
            server,
            lowered: AtomicUsize::new(0),
            slots: Mutex::default(),
            freed: Condvar::new(),
            idle: IDLE,
            sweeping: AtomicBool::new(false),
            sweeps: kernel::Pool::new("remote-idle", 1, kernel::work::IDLE),
        })
    }

    /// The same, idle sessions let go after `idle` (the tests').
    #[cfg(test)]
    pub fn with_idle(connector: Arc<dyn Connector>, limit: usize, idle: Duration) -> Arc<Self> {
        let mut pool = Self::new(connector, limit);
        if let Some(p) = Arc::get_mut(&mut pool) {
            p.idle = idle;
        }
        pool
    }

    /// Sessions open now (handed out or idle).
    pub fn open(&self) -> usize {
        self.slots.lock().map_or(0, |s| s.open)
    }

    /// How many may be open now.
    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Relaxed)
    }

    /// Lowered since last asked (the server refused more): to how many — told once.
    pub fn take_lowered(&self) -> Option<usize> {
        Some(self.lowered.swap(0, Ordering::Relaxed)).filter(|n| *n > 0)
    }

    /// Lowered at some point to the current limit (for a failure's hint).
    pub fn was_lowered(&self) -> bool {
        self.capped.load(Ordering::Relaxed) && self.limit() < self.max
    }

    /// The server refused one more while `open` were open: no more than those from now on
    /// (remembered for the server).
    fn lower(&self, open: usize) {
        let to = open.max(1);
        self.capped.store(true, Ordering::Relaxed);
        if self.limit.fetch_min(to, Ordering::Relaxed) > to {
            self.lowered.store(to, Ordering::Relaxed);
            tracing::info!(to, "the server refused another connection: fewer at once");
        }
        if let (Some(server), Ok(mut known)) = (&self.server, ACCEPTED.lock()) {
            known.retain(|(s, _)| s != server);
            known.push((server.clone(), to));
        }
    }

    /// One more may be opened (cautious: grown by one while the server hasn't refused).
    fn room(&self, open: usize) -> bool {
        let limit = self.limit();
        if open < limit {
            return true;
        }
        if self.server.is_some() && !self.capped.load(Ordering::Relaxed) && limit < self.max {
            self.limit.store(limit + 1, Ordering::Relaxed);
            return true;
        }
        false
    }

    fn give_back(self: &Arc<Self>, session: Option<Box<dyn RemoteFs>>) {
        let kept = session.is_some();
        if let Ok(mut slots) = self.slots.lock() {
            match session {
                Some(session) => slots.idle.push((session, Instant::now())),
                None => slots.open = slots.open.saturating_sub(1),
            }
        }
        self.freed.notify_one();
        if kept {
            self.sweep_later();
        }
    }

    /// The idle sessions too old let go. When the next one is due (`None`: none idle).
    fn sweep(&self) -> Option<Instant> {
        let mut slots = self.slots.lock().ok()?;
        let before = slots.idle.len();
        let idle = self.idle;
        slots.idle.retain(|(_, since)| since.elapsed() < idle);
        slots.open -= before - slots.idle.len();
        slots.idle.iter().map(|(_, since)| *since + idle).min()
    }

    /// Lets the idle sessions go once they've been idle long enough, even if nothing asks
    /// for one again (the connection would stay open until the project closes otherwise):
    /// one job on its `remote-idle` pool (one thread), sleeping until the oldest is due,
    /// ending once none is idle (or the pool is gone).
    fn sweep_later(self: &Arc<Self>) {
        if self.sweeping.swap(true, Ordering::AcqRel) {
            return;
        }
        let pool: Weak<Self> = Arc::downgrade(self);
        self.sweeps.spawn(kernel::Priority::Background, None, move || {
            while let Some(next) = pool.upgrade().and_then(|p| p.sweep()) {
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
            if let Some(pool) = pool.upgrade() {
                pool.sweeping.store(false, Ordering::Release);
                // Given back while it was ending: swept too.
                if pool.sweep().is_some() {
                    pool.sweep_later();
                }
            }
        });
    }
}

/// The pool as a connector (its `Arc`).
#[derive(Debug, Clone)]
pub struct Pooled(pub Arc<Pool>);

impl Connector for Pooled {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        let pool = &self.0;
        let mut slots = pool.slots.lock().map_err(|_| RemoteError::Connect("pool".into()))?;
        loop {
            // The idle ones too old: let go.
            let before = slots.idle.len();
            slots.idle.retain(|(_, since)| since.elapsed() < pool.idle);
            slots.open -= before - slots.idle.len();
            if let Some((session, _)) = slots.idle.pop() {
                return Ok(Box::new(Lent { session: Some(session), pool: Arc::clone(pool) }));
            }
            if pool.room(slots.open) {
                slots.open += 1;
                drop(slots);
                match pool.connector.connect() {
                    Ok(session) => {
                        return Ok(Box::new(Lent {
                            session: Some(session),
                            pool: Arc::clone(pool),
                        }));
                    }
                    // Cautious, others open: the server takes no more — as many as are open
                    // from now on, and one of them waited for.
                    Err(e) if pool.server.is_some() && refused_for_load(&e) => {
                        pool.give_back(None);
                        slots =
                            pool.slots.lock().map_err(|_| RemoteError::Connect("pool".into()))?;
                        if slots.open == 0 {
                            // None of ours: someone else's (another window, the wizard's
                            // session closing) — the caller tries again later.
                            return Err(RemoteError::Connect(e.to_string()));
                        }
                        pool.lower(slots.open);
                        continue;
                    }
                    Err(e) => {
                        pool.give_back(None);
                        return Err(e);
                    }
                }
            }
            slots = pool.freed.wait(slots).map_err(|_| RemoteError::Connect("pool".into()))?;
        }
    }
}

/// A session lent by the pool: back to it when dropped (unless the network broke it).
struct Lent {
    session: Option<Box<dyn RemoteFs>>,
    pool: Arc<Pool>,
}

impl Drop for Lent {
    fn drop(&mut self) {
        self.pool.give_back(self.session.take());
    }
}

impl Lent {
    /// The session's answer; a network failure breaks it (not given back for reuse).
    fn through<T>(
        &mut self,
        f: impl FnOnce(&mut dyn RemoteFs) -> Result<T, RemoteError>,
    ) -> Result<T, RemoteError> {
        let session = self.session.as_mut().ok_or(RemoteError::Connect("closed".into()))?;
        let result = f(session.as_mut());
        if let Err(e) = &result
            && e.is_transient()
        {
            self.session = None;
        }
        result
    }
}

impl RemoteFs for Lent {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        self.through(|s| s.list(dir))
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        self.through(|s| s.read(path))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError> {
        self.through(|s| s.write(path, data))
    }
    fn remove(&mut self, path: &str) -> Result<(), RemoteError> {
        self.through(|s| s.remove(path))
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError> {
        self.through(|s| s.rename(from, to))
    }
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        self.through(|s| s.stat(path))
    }
    fn read_moving(&mut self, path: &str, moved: Moved<'_>) -> Result<Vec<u8>, RemoteError> {
        self.through(|s| s.read_moving(path, moved))
    }
    fn write_moving(
        &mut self,
        path: &str,
        data: &[u8],
        moved: Moved<'_>,
    ) -> Result<RemoteEntry, RemoteError> {
        self.through(|s| s.write_moving(path, data, moved))
    }
    fn home(&mut self) -> Result<String, RemoteError> {
        self.through(|s| s.home())
    }
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        self.through(|s| s.properties(paths))
    }
    fn accounts(&mut self) -> Result<Accounts, RemoteError> {
        self.through(|s| s.accounts())
    }
    fn capabilities(&mut self) -> Capabilities {
        self.session.as_mut().map(|s| s.capabilities()).unwrap_or_default()
    }
    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        self.through(|s| s.set_properties(path, set))
    }
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        self.through(|s| s.checksum(path, algo))
    }
    fn list_properties(&mut self, dir: &str) -> Result<Vec<Properties>, RemoteError> {
        self.through(|s| s.list_properties(dir))
    }
}

/// A session that, failed for the network's sake (a reset, a timeout), is opened again —
/// waiting longer each time, at most `retry.attempts` tries — and the operation tried again;
/// what it listed is told to `on_listed` (the tree fills while a review still lists).
pub struct Resilient<'a> {
    connector: &'a dyn Connector,
    session: Option<Box<dyn RemoteFs>>,
    retry: Retry,
    on_listed: Option<&'a OnListed<'a>>,
}

/// Told each folder listed (its path, its entries).
pub type OnListed<'a> = dyn Fn(&str, &[RemoteEntry]) + Sync + 'a;

impl std::fmt::Debug for Resilient<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resilient").field("retry", &self.retry).finish_non_exhaustive()
    }
}

impl<'a> Resilient<'a> {
    pub fn new(connector: &'a dyn Connector, retry: Retry) -> Self {
        Self { connector, session: None, retry, on_listed: None }
    }

    /// Each folder listed told to `on_listed`.
    pub fn listing_to(mut self, on_listed: &'a OnListed<'a>) -> Self {
        self.on_listed = Some(on_listed);
        self
    }

    fn with<T>(
        &mut self,
        mut op: impl FnMut(&mut dyn RemoteFs) -> Result<T, RemoteError>,
    ) -> Result<T, RemoteError> {
        let mut last = RemoteError::TimedOut;
        for attempt in 1..=self.retry.attempts.max(1) {
            let wait = self.retry.wait_before(attempt);
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            if self.session.is_none() {
                match self.connector.connect() {
                    Ok(fs) => self.session = Some(fs),
                    Err(e) if e.is_transient() => {
                        last = e;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            let Some(fs) = self.session.as_mut() else { continue };
            match op(fs.as_mut()) {
                Err(e) if e.is_transient() => {
                    tracing::info!(attempt, "a remote operation failed for the network: again");
                    self.session = None;
                    last = e;
                }
                other => return other,
            }
        }
        Err(last)
    }
}

impl RemoteFs for Resilient<'_> {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        let listed = self.with(|s| s.list(dir))?;
        if let Some(on_listed) = self.on_listed {
            on_listed(dir, &listed);
        }
        Ok(listed)
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        self.with(|s| s.read(path))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError> {
        self.with(|s| s.write(path, data))
    }
    fn remove(&mut self, path: &str) -> Result<(), RemoteError> {
        self.with(|s| s.remove(path))
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError> {
        self.with(|s| s.rename(from, to))
    }
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        self.with(|s| s.stat(path))
    }
    fn home(&mut self) -> Result<String, RemoteError> {
        self.with(|s| s.home())
    }
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        self.with(|s| s.properties(paths))
    }
    fn accounts(&mut self) -> Result<Accounts, RemoteError> {
        self.with(|s| s.accounts())
    }
    fn capabilities(&mut self) -> Capabilities {
        self.with(|s| Ok(s.capabilities())).unwrap_or_default()
    }
    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        self.with(|s| s.set_properties(path, set))
    }
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        self.with(|s| s.checksum(path, algo))
    }
    fn list_properties(&mut self, dir: &str) -> Result<Vec<Properties>, RemoteError> {
        self.with(|s| s.list_properties(dir))
    }
}

#[cfg(test)]
#[path = "../tests/unit/pool.rs"]
mod tests;
