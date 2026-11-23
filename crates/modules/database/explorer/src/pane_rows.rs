//! A table's rows read for a pane over its grid (a chart): in pages on the source's worker,
//! in the order and filter shown, up to a limit, each page kept as it came (its rows moved,
//! not copied) and lent as `charts_model::Tabular` ([`database::lent::Lent`]). Read in the
//! background, the view polled when a page comes; dropped (another table, the pane closed),
//! what's asked is let go.

use std::sync::{Arc, Mutex, PoisonError, Weak};

use database::{Filter, Order, Page, TableRef, Value};

use crate::grid_source::message;
use crate::runtime::Runtime;
use crate::worker::{Client, Reply, Request, Response, SessionWorker, Ticket};

/// Rows a page asks for.
const PAGE: u32 = 2000;

/// A table's rows as lent: pages of rows.
pub type LentPages = database::lent::Lent<Vec<Value>>;

/// What's been read so far: the pages (shared), how many rows, whether it's done, whether
/// the table holds more than was read, and their version.
#[derive(Debug, Clone)]
pub struct Read {
    pub pages: Vec<Arc<[Vec<Value>]>>,
    pub rows: usize,
    pub done: bool,
    pub more: bool,
    pub version: u64,
}

/// What's being read.
#[derive(Debug, Clone)]
pub struct ReadAsk {
    pub table: TableRef,
    pub order: Vec<Order>,
    pub filter: Option<Filter>,
    pub limit: usize,
}

#[derive(Default)]
struct State {
    /// The worker's client: taken out of the lock to be called (never called under it).
    client: Option<Arc<Client>>,
    ticket: Option<Ticket>,
    /// A step is asking for the next page (another one doesn't ask it too).
    asking: bool,
    /// Replies that came while a page was being asked (before its ticket was kept): taken
    /// again once it is (one may be its).
    held: Vec<Reply>,
    pages: Vec<Arc<[Vec<Value>]>>,
    rows: usize,
    /// The table has no more, or the limit was reached.
    done: bool,
    /// The table holds more than the limit.
    more: bool,
    failed: Option<String>,
    /// Bumped with each page (the view lends them again).
    version: u64,
}

struct Shared {
    ask: ReadAsk,
    state: Mutex<State>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// A reading of a table's rows (see the module's doc).
pub struct PaneRows {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for PaneRows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneRows").field("table", &self.shared.ask.table).finish_non_exhaustive()
    }
}

impl PaneRows {
    /// `ask` read on `worker`'s source (its steps on `runtime`), `wake` called as pages come.
    pub fn start(
        worker: &SessionWorker,
        runtime: Runtime,
        ask: ReadAsk,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let shared = Arc::new_cyclic(|weak: &Weak<Shared>| {
            let weak = weak.clone();
            let step_runtime = runtime.clone();
            let on_reply: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(shared) = weak.upgrade() {
                    step_runtime.spawn(move || step(&shared));
                }
            });
            Shared {
                ask,
                state: Mutex::new(State {
                    client: Some(Arc::new(worker.client(on_reply))),
                    ..State::default()
                }),
                wake,
            }
        });
        let first = Arc::clone(&shared);
        runtime.spawn(move || step(&first));
        Self { shared }
    }

    pub fn ask(&self) -> &ReadAsk {
        &self.shared.ask
    }

    /// What's been read so far.
    pub fn read(&self) -> Read {
        let s = self.shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        Read {
            pages: s.pages.clone(),
            rows: s.rows,
            done: s.done,
            more: s.more,
            version: s.version,
        }
    }

    /// Why the reading failed, if it did (taken once).
    pub fn take_failure(&self) -> Option<String> {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner).failed.take()
    }
}

impl Drop for PaneRows {
    /// What's asked let go: no page read for nobody.
    fn drop(&mut self) {
        // Taken out under the lock, cancelled after it.
        let taken = {
            let mut s = self.shared.state.lock().unwrap_or_else(PoisonError::into_inner);
            (s.client.take(), s.ticket.take())
        };
        if let (Some(client), Some(ticket)) = taken {
            client.cancel(ticket);
        }
    }
}

/// One step: the page that came kept, the next asked — or the end. The lock only moves
/// the state in and out: the client is polled and asked outside it.
fn step(shared: &Arc<Shared>) {
    let client = {
        let s = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(client) = &s.client else { return };
        Arc::clone(client)
    };
    let polled = client.poll();
    let ask = &shared.ask;
    let request = {
        let mut s = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut replies = std::mem::take(&mut s.held);
        replies.extend(polled);
        for reply in replies {
            if Some(reply.ticket) != s.ticket {
                // Maybe the page being asked's (its ticket not kept yet): held for then.
                if s.asking {
                    s.held.push(reply);
                }
                continue;
            }
            s.ticket = None;
            match reply.result {
                Ok(Response::Page(page)) => {
                    let came = page.rows.len();
                    let room = ask.limit.saturating_sub(s.rows);
                    let mut rows = page.rows;
                    rows.truncate(room);
                    s.rows += rows.len();
                    if !rows.is_empty() {
                        s.pages.push(Arc::from(rows));
                    }
                    s.version += 1;
                    let full = s.rows >= ask.limit;
                    if came < PAGE as usize || full {
                        s.done = true;
                        // More left: rows of this page cut off at the limit, or a full page
                        // ending at it (the next may have more).
                        s.more = came > room || (full && came == PAGE as usize);
                        s.client = None;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    s.failed = Some(message(&e));
                    s.done = true;
                    s.client = None;
                }
            }
        }
        let next = !s.done && s.ticket.is_none() && !s.asking && s.client.is_some();
        s.asking |= next;
        next.then(|| {
            let page = Page {
                offset: s.rows as u64,
                limit: PAGE,
                order: ask.order.clone(),
                filter: ask.filter.clone(),
            };
            Request::ReadPage { table: ask.table.clone(), page, ahead: false }
        })
    };
    if let Some(request) = request {
        let ticket = client.send(request);
        let mut s = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.asking = false;
        // Let go meanwhile (closed): what was just asked isn't kept, it's cancelled.
        let kept = s.client.is_some();
        if kept {
            s.ticket = Some(ticket);
        }
        let again = kept && !s.held.is_empty();
        drop(s);
        if !kept {
            client.cancel(ticket);
        }
        // Its reply may have come meanwhile: taken now.
        if again {
            step(shared);
            return;
        }
    }
    (shared.wake)();
}

#[cfg(test)]
#[path = "../tests/unit/pane_rows.rs"]
mod tests;
