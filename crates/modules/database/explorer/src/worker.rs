//! One open data source as an actor: its [`Session`] owned by a [`SessionWorker`], whose
//! requests wait in a mailbox and run one at a time on the shared pool `db` (one job per
//! request, so the sessions of other sources take turns on the pool). A view talks to it
//! through its own [`Client`]: what it asks gets a [`Ticket`], the replies wait in the
//! client's queue, in the order asked, and the view is woken to take them at its `poll`.
//!
//! A page asked again for the same table (by the same client, read ahead or not) replaces
//! the one still waiting — while scrolling fast, only the latest is read, and it waits
//! behind what was asked before it (a commit), never ahead. Pages read ahead go after
//! everything else. Any request can be cancelled: one waiting is dropped, one running is
//! told to stop ([`database::Cancel`]). The session is opened by the first request (and
//! again after it was closed: idle, or a lost connection), closed by [`Request::Close`] or
//! [`SessionWorker::close_now`].
//!
//! What the user waits for goes first: the requests in the order asked, then the totals
//! ([`Request::Count`]), then the pages read ahead, then the lists' background counts
//! ([`Request::RowCount`]). An SQL source counts on a lane of its own — a second session
//! of the same source, opened by its first count and closed with the first —, so a
//! `COUNT(*)` that takes long never holds a page back; a source that writes no SQL (a CSV
//! file, whose session holds its index) counts on its one session, after the rest.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use database::transaction::AlterPreview;
use database::{
    Applied, Capabilities, ChangeSet, DataSourceId, DbError, Dialect, Filter, Page, Rows,
    SchemaChange, Script, Session, Structure, TableInfo, TableRef,
};
use kernel::EventBus;

use crate::runtime::Runtime;

/// A request's identity, for its reply and to cancel it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ticket(u64);

/// What a session is asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// The tables and views under `path` (`[]`: the top).
    Tables(Vec<String>),
    /// The databases of its server it may open, and the one its session is on.
    Databases,
    Structure(TableRef),
    /// The rows `filter` keeps (a grid's total).
    Count {
        table: TableRef,
        filter: Option<Filter>,
    },
    /// A table's exact rows for a list, in the background: after everything else, on the
    /// counting lane where the source has one.
    RowCount(TableRef),
    /// A page; `ahead`: read before it's shown (after everything else).
    ReadPage {
        table: TableRef,
        page: Page,
        ahead: bool,
    },
    /// Row changes, in one transaction.
    Apply(ChangeSet),
    /// Structure changes, in one transaction (a table rebuilt where the engine needs it).
    Alter(Vec<SchemaChange>),
    /// What structure changes would run, without running them: the script (an SQL
    /// source's; none for one that writes no SQL) and what its renames rewrite.
    PlanAlter(Vec<SchemaChange>),
    /// Structure changes as they were shown ([`Request::PlanAlter`]'s script): planned
    /// again, and refused if that's no longer exactly what was shown (the structure
    /// changed meanwhile), else run in one transaction.
    AlterAsShown {
        changes: Vec<SchemaChange>,
        shown: Option<Script>,
    },
    /// A catalog statement (CREATE DATABASE, CREATE SCHEMA) run outside a transaction.
    CatalogDdl(String),
    /// The session is closed (after what was asked before); the next request opens it.
    Close,
}

/// What a request gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Tables(Vec<TableInfo>),
    /// Every database it may open (sorted), and the one its session is on (`None`: it
    /// doesn't say).
    Databases {
        names: Vec<String>,
        current: Option<String>,
    },
    Structure(Structure),
    Count(u64),
    Page(Rows),
    Applied(Applied),
    /// What [`Request::PlanAlter`] would run (none: a source that writes no SQL).
    Plan(Option<AlterPreview>),
    /// A catalog statement ran.
    CatalogDone,
    Closed,
}

/// A request's reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub ticket: Ticket,
    pub result: Result<Response, DbError>,
}

/// Opens a source's session (on the pool's thread).
pub type Connect = Arc<dyn Fn() -> Result<Box<dyn Session>, DbError> + Send + Sync>;

/// What a worker knows of its source: how to open it and what it may do.
#[derive(Clone)]
pub struct Source {
    pub id: DataSourceId,
    pub connect: Connect,
    /// How its SQL is written (`None`: it writes none, its session applies changes itself).
    pub dialect: Option<Arc<dyn Dialect>>,
    pub capabilities: Capabilities,
    /// Nothing written through it.
    pub read_only: bool,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source").field("id", &self.id).finish_non_exhaustive()
    }
}

/// Called when the session opens (the idle watch starts then).
pub type OnConnect = Arc<dyn Fn() + Send + Sync>;

/// Called with the structure changes a commit made, before they're said on the bus (what
/// follows them, the grids' kept layouts, is ready when the views read again).
pub type OnAltered = Arc<dyn Fn(&[SchemaChange]) + Send + Sync>;

/// A client's replies, and how its view is woken.
struct Inbox {
    id: u64,
    replies: Mutex<VecDeque<Reply>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Inbox {
    fn push(&self, reply: Reply) {
        self.replies.lock().unwrap_or_else(PoisonError::into_inner).push_back(reply);
        (self.wake)();
    }
}

struct Queued {
    ticket: Ticket,
    request: Request,
    inbox: Arc<Inbox>,
}

struct Running {
    ticket: Ticket,
    inbox: u64,
    cancel: database::Cancel,
}

/// Where a request runs: the source's session, or its counting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneId {
    Main,
    Counts,
}

/// A session and its mailbox, one request running at a time.
#[derive(Default)]
struct Lane {
    /// Open, and not lent to the request running.
    session: Option<Box<dyn Session>>,
    /// Open: here, or lent to the request running.
    connected: bool,
    mailbox: VecDeque<Queued>,
    running: Option<Running>,
    /// A job is on the pool (or running).
    scheduled: bool,
    /// The session goes once the request running ends.
    close_after: bool,
}

impl Lane {
    fn idle(&self) -> bool {
        self.running.is_none() && self.mailbox.is_empty()
    }

    /// The next to run: the first of the most urgent kind (see the module's doc).
    fn next(&self) -> Option<usize> {
        self.mailbox
            .iter()
            .enumerate()
            .min_by_key(|(at, q)| (urgency(&q.request), *at))
            .map(|(at, _)| at)
    }
}

/// How long a request may wait for others: what the user waits for (0), a total (1), a
/// page read ahead (2), a list's background count (3).
fn urgency(request: &Request) -> u8 {
    match request {
        Request::Count { .. } => 1,
        Request::ReadPage { ahead: true, .. } => 2,
        Request::RowCount(_) => 3,
        _ => 0,
    }
}

struct State {
    main: Lane,
    /// The counts' own session (an SQL source's).
    counts: Lane,
    /// When it was last asked something or answered.
    used: Instant,
    next: u64,
}

impl State {
    fn lane(&mut self, id: LaneId) -> &mut Lane {
        match id {
            LaneId::Main => &mut self.main,
            LaneId::Counts => &mut self.counts,
        }
    }
}

struct Shared {
    source: Source,
    runtime: Runtime,
    /// The project's bus: what a commit changed is said there.
    events: Option<EventBus>,
    on_connect: Option<OnConnect>,
    on_altered: Mutex<Option<OnAltered>>,
    /// The server's name and version, as its session said when it last opened.
    server: Mutex<Option<String>>,
    state: Mutex<State>,
    /// Where what the user waits on is listed as operations (the project's).
    tracking: Mutex<Option<Tracking>>,
}

/// The operations a worker lists what the user waits on in: a grid's page, its total, a
/// commit (the Task Manager's, the question before closing).
#[derive(Debug, Clone)]
pub struct Tracking {
    pub activities: Arc<workbench_model::Activities>,
    pub project: kernel::ProjectId,
    /// The source's name, as shown.
    pub source: String,
}

/// What ends quicker than this isn't kept among the operations (a page read while
/// scrolling): only what one could have waited on.
const KEPT_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// What `request` is, as an operation (`None`: not one the user waits on — the lists, a
/// page read ahead, the background counts).
fn operation(request: &Request) -> Option<String> {
    let table = |t: &TableRef| match &t.schema {
        Some(schema) => format!("{schema}.{}", t.name),
        None => t.name.clone(),
    };
    Some(match request {
        Request::ReadPage { table: t, ahead: false, .. } => {
            i18n::tr!("database.op_read", table = table(t))
        }
        Request::Count { table: t, .. } => i18n::tr!("database.op_count", table = table(t)),
        Request::Apply(_) => i18n::t("database.op_apply"),
        Request::Alter(_) | Request::AlterAsShown { .. } => i18n::t("database.op_alter"),
        _ => return None,
    })
}

/// One source's session and its mailbox (see the module's doc). Cloning is cheap: the
/// clones are the same worker.
#[derive(Clone)]
pub struct SessionWorker {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for SessionWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionWorker").field("source", &self.shared.source).finish_non_exhaustive()
    }
}

static INBOXES: AtomicU64 = AtomicU64::new(1);

impl SessionWorker {
    /// A worker for `source`; nothing opens until the first request. A commit is said on
    /// `events` (the project's bus).
    pub fn new(
        source: Source,
        runtime: Runtime,
        events: Option<EventBus>,
        on_connect: Option<OnConnect>,
    ) -> Self {
        let state =
            State { main: Lane::default(), counts: Lane::default(), used: runtime.now(), next: 1 };
        let shared = Shared {
            source,
            runtime,
            events,
            on_connect,
            on_altered: Mutex::new(None),
            server: Mutex::new(None),
            state: Mutex::new(state),
            tracking: Mutex::new(None),
        };
        Self { shared: Arc::new(shared) }
    }

    pub fn source(&self) -> &Source {
        &self.shared.source
    }

    /// What the user waits on listed in `tracking`'s operations, from now.
    pub fn track(&self, tracking: Tracking) {
        *self.shared.tracking.lock().unwrap_or_else(PoisonError::into_inner) = Some(tracking);
    }

    /// `request` starting: its operation (stopped as cancelling it does), if it's one.
    fn begin(&self, request: &Request, cancel: &database::Cancel) -> Option<(Tracking, u128)> {
        let subject = operation(request)?;
        let tracking =
            self.shared.tracking.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
        let unit = workbench_model::ActivityUnit::Rows;
        let id = tracking.activities.start(
            Some(tracking.project),
            workbench_model::ActivityKind::Query,
            subject,
            unit,
            None,
        );
        tracking.activities.describe(id, tracking.source.clone(), None);
        let cancel = cancel.clone();
        tracking.activities.stoppable_with(id, move || cancel.cancel());
        Some((tracking, id))
    }

    /// Its operation ended as `result` says; one that took less than [`KEPT_AFTER`] and
    /// went well isn't kept.
    fn finish(
        &self,
        begun: Option<(Tracking, u128)>,
        result: &Result<Response, DbError>,
        took: std::time::Duration,
    ) {
        let Some((tracking, id)) = begun else { return };
        let outcome = match result {
            Ok(Response::Page(rows)) => {
                tracking.activities.advance(id, rows.rows.len() as u64);
                workbench_model::Outcome::Done
            }
            Ok(Response::Count(n)) => {
                tracking.activities.advance(id, *n);
                workbench_model::Outcome::Done
            }
            Ok(_) => workbench_model::Outcome::Done,
            Err(DbError::Cancelled) => workbench_model::Outcome::Stopped,
            Err(e) => workbench_model::Outcome::Failed(e.to_string()),
        };
        if outcome == workbench_model::Outcome::Done && took < KEPT_AFTER {
            tracking.activities.discard(id);
        } else {
            tracking.activities.end(id, outcome);
        }
    }

    /// What's called with the structure changes each commit makes (see [`OnAltered`]).
    pub fn on_altered(&self, hook: OnAltered) {
        *self.shared.on_altered.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// A client: its own replies, `wake` called when one comes (from the pool's thread).
    pub fn client(&self, wake: Arc<dyn Fn() + Send + Sync>) -> Client {
        let inbox =
            Inbox { id: INBOXES.fetch_add(1, Ordering::Relaxed), replies: Mutex::default(), wake };
        Client { worker: self.clone(), inbox: Arc::new(inbox) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The server's name and version (`PostgreSQL 16.4`), once a session opened.
    pub fn server(&self) -> Option<String> {
        self.shared.server.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The session is open (or its counting one).
    pub fn connected(&self) -> bool {
        let state = self.lock();
        state.main.connected || state.counts.connected
    }

    /// The counting lane's session is open.
    pub fn counting_connected(&self) -> bool {
        self.lock().counts.connected
    }

    /// Something waits or runs.
    pub fn busy(&self) -> bool {
        let state = self.lock();
        !state.main.idle() || !state.counts.idle()
    }

    /// Requests waiting (the running ones not counted).
    pub fn waiting(&self) -> usize {
        let state = self.lock();
        state.main.mailbox.len() + state.counts.mailbox.len()
    }

    /// Since when an open session has been unused (`None`: closed, or at work).
    pub fn idle_since(&self) -> Option<Instant> {
        let state = self.lock();
        let open = state.main.connected || state.counts.connected;
        let idle = open && state.main.idle() && state.counts.idle();
        idle.then_some(state.used)
    }

    /// Where `request` runs: an SQL source's counts on their own session.
    fn lane_of(&self, request: &Request) -> LaneId {
        let count = matches!(request, Request::Count { .. } | Request::RowCount(_));
        if count && self.shared.source.dialect.is_some() { LaneId::Counts } else { LaneId::Main }
    }

    fn send(&self, inbox: &Arc<Inbox>, request: Request) -> Ticket {
        let mut superseded = Vec::new();
        let lane = self.lane_of(&request);
        let (ticket, start) = {
            let mut state = self.lock();
            let ticket = Ticket(state.next);
            state.next += 1;
            state.used = self.shared.runtime.now();
            let state = state.lane(lane);
            if let Request::ReadPage { table, ahead, .. } = &request {
                // The same client's page of the same table still waiting: replaced (and
                // the new one waits at the end: behind what was asked before it).
                let same = |q: &Queued| {
                    q.inbox.id == inbox.id
                        && matches!(&q.request, Request::ReadPage { table: t, ahead: a, .. }
                            if t == table && a == ahead)
                };
                let mut kept = VecDeque::with_capacity(state.mailbox.len());
                for queued in std::mem::take(&mut state.mailbox) {
                    if same(&queued) {
                        superseded.push(queued.ticket)
                    } else {
                        kept.push_back(queued)
                    }
                }
                state.mailbox = kept;
            }
            state.mailbox.push_back(Queued { ticket, request, inbox: Arc::clone(inbox) });
            let start = !std::mem::replace(&mut state.scheduled, true);
            (ticket, start)
        };
        for old in superseded {
            inbox.push(Reply { ticket: old, result: Err(DbError::Cancelled) });
        }
        if start {
            self.schedule(lane);
        }
        ticket
    }

    /// Cancels `ticket` (of the client `inbox`): dropped if it waits (its reply:
    /// cancelled), told to stop if it runs.
    fn cancel(&self, inbox: &Arc<Inbox>, ticket: Ticket) {
        let dropped = {
            let mut state = self.lock();
            let mut dropped = false;
            for lane in [LaneId::Main, LaneId::Counts] {
                let state = state.lane(lane);
                if let Some(running) = &state.running
                    && running.ticket == ticket
                    && running.inbox == inbox.id
                {
                    running.cancel.cancel();
                } else {
                    let before = state.mailbox.len();
                    state.mailbox.retain(|q| !(q.ticket == ticket && q.inbox.id == inbox.id));
                    dropped |= state.mailbox.len() != before;
                }
            }
            dropped
        };
        if dropped {
            inbox.push(Reply { ticket, result: Err(DbError::Cancelled) });
        }
    }

    /// A client went: what it asked is dropped, what runs for it is told to stop (no
    /// reply: nobody waits for it).
    fn forget(&self, inbox: u64) {
        let mut state = self.lock();
        for lane in [LaneId::Main, LaneId::Counts] {
            let state = state.lane(lane);
            state.mailbox.retain(|q| q.inbox.id != inbox);
            if let Some(running) = &state.running
                && running.inbox == inbox
            {
                running.cancel.cancel();
            }
        }
    }

    /// Closes the session now: what waits is cancelled, what runs is told to stop (the
    /// session goes when it ends). The next request opens it again.
    pub fn close_now(&self) {
        let (dropped, sessions, was, now) = {
            let mut state = self.lock();
            let was = state.main.connected || state.counts.connected;
            let mut dropped: Vec<Queued> = Vec::new();
            let mut sessions = Vec::new();
            for lane in [LaneId::Main, LaneId::Counts] {
                let state = state.lane(lane);
                dropped.extend(state.mailbox.drain(..));
                if let Some(running) = &state.running {
                    running.cancel.cancel();
                    state.close_after = true;
                } else {
                    state.connected = false;
                }
                sessions.extend(state.session.take());
            }
            let now = state.main.connected || state.counts.connected;
            (dropped, sessions, was, now)
        };
        if was != now {
            self.tell_connection(now, false);
        }
        // Closed outside the lock and off the caller's thread (the app's, often): a server may
        // take its time to say goodbye.
        if !sessions.is_empty() {
            self.shared.runtime.spawn(move || drop(sessions));
        }
        for queued in dropped {
            queued.inbox.push(Reply { ticket: queued.ticket, result: Err(DbError::Cancelled) });
        }
    }

    fn schedule(&self, lane: LaneId) {
        let worker = self.clone();
        self.shared.runtime.spawn(move || worker.step(lane));
    }

    /// Runs `lane`'s next request (one per job), then hands the pool the next one.
    fn step(&self, lane: LaneId) {
        let (queued, session, cancel) = {
            let mut state = self.lock();
            let state = state.lane(lane);
            // What the user waits for first (see the module's doc).
            let next = state.next();
            let Some(queued) = next.and_then(|at| state.mailbox.remove(at)) else {
                state.scheduled = false;
                return;
            };
            let cancel = database::Cancel::default();
            state.running = Some(Running {
                ticket: queued.ticket,
                inbox: queued.inbox.id,
                cancel: cancel.clone(),
            });
            (queued, state.session.take(), cancel)
        };

        let begun = self.begin(&queued.request, &cancel);
        let started = self.shared.runtime.now();
        let (result, session, opened) = self.run(&queued.request, session, &cancel);
        let took = self.shared.runtime.now().saturating_duration_since(started);
        self.finish(begun, &result, took);
        if opened && let Some(on_connect) = &self.shared.on_connect {
            on_connect();
        }
        if let Ok(response) = &result {
            self.announce(&queued.request, response);
        }
        // It couldn't open (the server unreachable, the login refused), not a query refused.
        let result_failed = matches!(result, Err(DbError::Connect(_) | DbError::Auth(_)));
        // The reply first: the next request's can't overtake it.
        queued.inbox.push(Reply { ticket: queued.ticket, result });

        let failed = lane == LaneId::Main && session.is_none() && result_failed;
        let (more, closed, was, now) = {
            let mut state = self.lock();
            let was = state.main.connected || state.counts.connected;
            state.used = self.shared.runtime.now();
            let mut closed = Vec::new();
            // Closing the source closes its counting session too (the one counting now
            // once its count ends).
            if queued.request == Request::Close {
                let counts = &mut state.counts;
                if counts.running.is_some() {
                    counts.close_after = true;
                } else {
                    counts.connected = false;
                    closed.extend(counts.session.take());
                }
            }
            let more = {
                let state = state.lane(lane);
                state.running = None;
                if std::mem::take(&mut state.close_after) || session.is_none() {
                    state.connected = false;
                    closed.extend(session);
                } else {
                    state.connected = true;
                    state.session = session;
                }
                let more = !state.mailbox.is_empty();
                if !more {
                    state.scheduled = false;
                }
                more
            };
            let now = state.main.connected || state.counts.connected;
            (more, closed, was, now)
        };
        drop(closed);
        // Told after the lock: what hears it may ask the worker again.
        if was != now || failed {
            self.tell_connection(now, failed);
        }
        if more {
            self.schedule(lane);
        }
    }

    /// Runs `request` on the session (opened first if it isn't): the result, the session
    /// to keep (`None`: closed, or lost), and whether it was opened now.
    fn run(
        &self,
        request: &Request,
        session: Option<Box<dyn Session>>,
        cancel: &database::Cancel,
    ) -> (Result<Response, DbError>, Option<Box<dyn Session>>, bool) {
        if *request == Request::Close {
            return (Ok(Response::Closed), None, false);
        }
        let opened = session.is_none();
        let mut session = match session {
            Some(session) => session,
            None => match (self.shared.source.connect)() {
                Ok(session) => {
                    let server = Some(session.server()).filter(|s| !s.is_empty());
                    *self.shared.server.lock().unwrap_or_else(PoisonError::into_inner) = server;
                    session
                }
                Err(e) => return (Err(e), None, false),
            },
        };
        let source = &self.shared.source;
        // A driver that panics loses its session, not the worker.
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::execute::run(session.as_mut(), request, source, cancel)
        }));
        match ran {
            // A lost connection, or a driver's program that stopped: opened (started)
            // again by the next request.
            Ok(Err(e @ (DbError::Connect(_) | DbError::Driver(_)))) => (Err(e), None, opened),
            Ok(result) => (result, Some(session), opened),
            Err(_) => {
                tracing::error!(source = %source.id, "a database driver failed");
                (Err(DbError::Query("the driver failed".to_owned())), None, opened)
            }
        }
    }

    /// A commit changed tables: said on the project's bus (their other views read again).
    fn announce(&self, request: &Request, response: &Response) {
        if !matches!(response, Response::Applied(_)) {
            return;
        }
        if let Request::Alter(changes) | Request::AlterAsShown { changes, .. } = request {
            let hook =
                self.shared.on_altered.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(hook) = hook {
                hook(changes);
            }
        }
        let Some(events) = &self.shared.events else { return };
        let source = self.shared.source.id;
        for (table, structure) in crate::execute::changed_tables(request) {
            events.publish(&database::events::DataChanged { source, table, structure });
        }
    }

    /// Its session opened or closed (`failed`: it couldn't open), said on the project's bus.
    fn tell_connection(&self, open: bool, failed: bool) {
        let Some(events) = &self.shared.events else { return };
        let source = self.shared.source.id;
        events.publish(&database::events::ConnectionChanged { source, open, failed });
    }
}

/// A view's side of a worker: what it asks, its replies (see the module's doc). Dropped,
/// what it asked and is still waiting is dropped too.
pub struct Client {
    worker: SessionWorker,
    inbox: Arc<Inbox>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("inbox", &self.inbox.id).finish_non_exhaustive()
    }
}

impl Client {
    pub fn send(&self, request: Request) -> Ticket {
        self.worker.send(&self.inbox, request)
    }

    pub fn cancel(&self, ticket: Ticket) {
        self.worker.cancel(&self.inbox, ticket);
    }

    /// The replies come since the last call, in the order asked.
    pub fn poll(&self) -> Vec<Reply> {
        self.inbox.replies.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect()
    }

    pub fn worker(&self) -> &SessionWorker {
        &self.worker
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.worker.forget(self.inbox.id);
    }
}

#[cfg(test)]
#[path = "../tests/unit/worker.rs"]
mod tests;
