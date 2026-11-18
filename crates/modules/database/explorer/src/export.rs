//! A whole result written out in the background: every row of a table (its order and
//! filter applied, not just the page shown) through a data extractor — the same text the
//! grid copies (CSV, TSV, JSON, SQL `INSERT`s / `UPDATE`s, Markdown, HTML, XML, the user's
//! templates…) — or as an Excel workbook ([`ExportWriter`]), a block of rows at a time, never
//! all of them in memory.
//!
//! An [`Export`] asks the source's worker for one block at a time through its own client
//! (so the grid's pages take turns with it on the pool `db`); each block that comes is
//! written by a job on the same pool, which asks for the next — nothing waits blocked,
//! nothing runs on the UI thread. A file is written beside its target as a hidden
//! temporary one, renamed over the target once complete; cancelled or failed, it's
//! removed and the target is left as it was. A small result can go to the clipboard
//! instead (at most [`CLIPBOARD_ROWS`] rows).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use database::{DbError, Filter, Order, Page, Rows, TableRef};
use ui_components::data_grid::GridColumn;
use ui_components::data_grid::export::{ExportFormat, ExportWriter};
use workbench_model::{Activities, ActivityKind, ActivityUnit, Outcome};

use crate::grid_source::{cell_ref, message};
use crate::runtime::Runtime;
use crate::worker::{Client, Request, Response, SessionWorker, Ticket};

/// Rows asked for at a time.
pub const CHUNK: u32 = 2000;

pub use ui_components::data_grid::export::{
    CLIPBOARD_ROWS, ExportOutcome, ExportSink as Sink, Target, format_of, temporary,
};

/// What's exported: which table, its columns (in the order shown), the rows' order and
/// filter, the format, where to.
#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub table: TableRef,
    pub columns: Vec<GridColumn>,
    pub order: Vec<Order>,
    pub filter: Option<Filter>,
    pub format: ExportFormat,
    /// How the source writes SQL (the SQL extractors'), and the columns they skip.
    pub sql: Option<Arc<dyn extractors::SqlStyle>>,
    pub options: extractors::Options,
    pub target: Target,
    /// The rows there are, when known (the progress shown against it).
    pub total: Option<u64>,
}

struct State {
    client: Option<Client>,
    /// The block asked and not come yet.
    ticket: Option<Ticket>,
    offset: u64,
    stream: ExportWriter,
    sink: Option<Sink>,
    started: bool,
    outcome: Option<ExportOutcome>,
}

struct Shared {
    request: ExportRequest,
    runtime: Runtime,
    state: Mutex<State>,
    cancelled: AtomicBool,
    rows: AtomicU64,
    /// Wakes the view (progress, the end).
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The Task Manager's record of it, and its identity there.
    activity: Option<(Arc<dyn ActivityLog>, u128)>,
}

/// Where an export's progress is kept for the Task Manager (the project's sessions').
pub trait ActivityLog: Send + Sync {
    fn activities(&self) -> &Activities;
    /// The project it's of.
    fn project(&self) -> Option<kernel::ProjectId>;
}

impl ActivityLog for crate::sessions::Sessions {
    fn activities(&self) -> &Activities {
        crate::sessions::Sessions::activities(self)
    }

    fn project(&self) -> Option<kernel::ProjectId> {
        Some(crate::sessions::Sessions::project(self))
    }
}

/// `shared` stopped: its next step ends it (the temporary file removed).
fn cancel(shared: &Arc<Shared>) {
    if !shared.cancelled.swap(true, Ordering::AcqRel) {
        let next = Arc::clone(shared);
        shared.runtime.spawn(move || step(&next));
    }
}

/// An export running (see the module's doc); dropped before its end (its view closed),
/// it's cancelled (its temporary file removed).
pub struct Export {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Export {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Export").field("target", &self.shared.request.target).finish()
    }
}

impl Export {
    /// Starts `request` on `worker`'s source (its jobs on `runtime`); `wake` is called
    /// as it goes and when it ends; `log` records it as an operation.
    pub fn start(
        worker: &SessionWorker,
        runtime: Runtime,
        request: ExportRequest,
        wake: Arc<dyn Fn() + Send + Sync>,
        log: Option<Arc<dyn ActivityLog>>,
    ) -> Self {
        let stream = writer(&request, &request.columns);
        let activity = log.map(|log| {
            let subject = match &request.target {
                Target::File(path) => kernel::paths::file_name(path),
                Target::Clipboard => request.table.name.clone(),
            };
            let id = log.activities().start(
                log.project(),
                ActivityKind::Export,
                subject,
                ActivityUnit::Rows,
                request.total,
            );
            (log, id)
        });
        let shared = Arc::new_cyclic(|weak: &Weak<Shared>| {
            let weak = weak.clone();
            let step_runtime = runtime.clone();
            // A block came: written by a job on the pool.
            let on_reply: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(shared) = weak.upgrade() {
                    step_runtime.spawn(move || step(&shared));
                }
            });
            Shared {
                request,
                runtime: runtime.clone(),
                state: Mutex::new(State {
                    client: Some(worker.client(on_reply)),
                    ticket: None,
                    offset: 0,
                    stream,
                    sink: None,
                    started: false,
                    outcome: None,
                }),
                cancelled: AtomicBool::new(false),
                rows: AtomicU64::new(0),
                wake,
                activity,
            }
        });
        // Its "Stop" in the Task Manager: cancelled as from its view.
        if let Some((log, id)) = &shared.activity {
            let weak = Arc::downgrade(&shared);
            log.activities().stoppable_with(*id, move || {
                if let Some(shared) = weak.upgrade() {
                    cancel(&shared);
                }
            });
        }
        let first = Arc::clone(&shared);
        runtime.spawn(move || step(&first));
        Self { shared }
    }

    /// An export shown midway, `rows` written, going nowhere (a snapshot of its progress:
    /// nothing is read or written).
    #[cfg(feature = "snapshot")]
    pub fn held(request: ExportRequest, rows: u64) -> Self {
        let stream = writer(&request, &[]);
        let shared = Shared {
            request,
            runtime: Runtime::new(|_| {}, std::time::Instant::now),
            state: Mutex::new(State {
                client: None,
                ticket: None,
                offset: 0,
                stream,
                sink: None,
                started: true,
                outcome: None,
            }),
            cancelled: AtomicBool::new(false),
            rows: AtomicU64::new(rows),
            wake: Arc::new(|| {}),
            activity: None,
        };
        Self { shared: Arc::new(shared) }
    }

    pub fn request(&self) -> &ExportRequest {
        &self.shared.request
    }

    /// The rows written so far.
    pub fn rows(&self) -> u64 {
        self.shared.rows.load(Ordering::Relaxed)
    }

    /// Stops it: the temporary file goes, the target stays as it was.
    pub fn cancel(&self) {
        cancel(&self.shared);
    }

    pub fn cancelling(&self) -> bool {
        self.shared.cancelled.load(Ordering::Acquire)
    }

    /// How it ended, once it has (taken once).
    pub fn take_outcome(&self) -> Option<ExportOutcome> {
        self.shared.state.lock().unwrap_or_else(PoisonError::into_inner).outcome.take()
    }

    /// It ended (its outcome may have been taken).
    pub fn finished(&self) -> bool {
        let state = self.shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.client.is_none()
    }
}

impl Drop for Export {
    fn drop(&mut self) {
        if !self.finished() {
            self.cancel();
        }
    }
}

/// One step: the file opened (the first time), the block that came written, the next
/// asked — or the end (complete, cancelled, failed).
fn step(shared: &Arc<Shared>) {
    let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
    if state.client.is_none() {
        return;
    }
    let request = &shared.request;
    // Cancelled before anything was written: nothing is made.
    if !state.started && shared.cancelled.load(Ordering::Acquire) {
        return end(shared, &mut state, ExportOutcome::Cancelled);
    }
    if !state.started {
        state.started = true;
        let sink = Sink::open(&request.target);
        match sink {
            Ok(mut sink) => {
                let begin = state.stream.begin();
                if let Err(why) =
                    begin.and_then(|b| sink.write_bytes(&b).map_err(|e| e.to_string()))
                {
                    sink.discard();
                    return end(shared, &mut state, ExportOutcome::Failed(why));
                }
                state.sink = Some(sink);
            }
            Err(e) => return end(shared, &mut state, ExportOutcome::Failed(e.to_string())),
        }
    }
    if shared.cancelled.load(Ordering::Acquire) {
        return end(shared, &mut state, ExportOutcome::Cancelled);
    }
    let replies = state.client.as_ref().map(Client::poll).unwrap_or_default();
    for reply in replies {
        if Some(reply.ticket) != state.ticket {
            continue;
        }
        state.ticket = None;
        match reply.result {
            Ok(Response::Page(rows)) => {
                if let Err(e) = write_block(shared, &mut state, &rows) {
                    return end(shared, &mut state, ExportOutcome::Failed(e.to_string()));
                }
                if rows.len() < CHUNK as usize {
                    return complete(shared, &mut state);
                }
                state.offset += rows.len() as u64;
            }
            Ok(_) => {}
            Err(DbError::Cancelled) if shared.cancelled.load(Ordering::Acquire) => {
                return end(shared, &mut state, ExportOutcome::Cancelled);
            }
            Err(e) => return end(shared, &mut state, ExportOutcome::Failed(message(&e))),
        }
    }
    if state.ticket.is_none() {
        let page = Page {
            offset: state.offset,
            limit: CHUNK,
            order: request.order.clone(),
            filter: request.filter.clone(),
        };
        let ask = Request::ReadPage { table: request.table.clone(), page, ahead: false };
        state.ticket = state.client.as_ref().map(|c| c.send(ask));
    }
}

/// A block's rows written (in the columns' order, by name).
fn write_block(shared: &Shared, state: &mut State, rows: &Rows) -> std::io::Result<()> {
    let request = &shared.request;
    if request.target == Target::Clipboard
        && state.stream.written() + rows.len() as u64 > CLIPBOARD_ROWS
    {
        return Err(std::io::Error::other(i18n::tr!(
            "database.export_clipboard_limit",
            count = CLIPBOARD_ROWS
        )));
    }
    let at: Vec<Option<usize>> = request
        .columns
        .iter()
        .map(|c| rows.columns.iter().position(|r| r.name == c.name))
        .collect();
    let block: Vec<Vec<_>> = rows
        .rows
        .iter()
        .map(|row| {
            at.iter()
                .map(|i| {
                    i.and_then(|i| row.get(i))
                        .map_or(ui_components::data_grid::CellRef::Null, cell_ref)
                })
                .collect()
        })
        .collect();
    let bytes = state.stream.rows(&block).map_err(std::io::Error::other)?;
    if let Some(sink) = &mut state.sink {
        sink.write_bytes(&bytes)?;
    }
    shared.rows.fetch_add(rows.len() as u64, Ordering::Relaxed);
    if let Some((log, id)) = &shared.activity {
        log.activities().advance(*id, rows.len() as u64);
    }
    (shared.wake)();
    Ok(())
}

/// Every row written: the file completed and renamed over its target.
fn complete(shared: &Shared, state: &mut State) {
    let finish = match state.stream.finish() {
        Ok(finish) => finish,
        Err(why) => return end(shared, state, ExportOutcome::Failed(why)),
    };
    let rows = state.stream.written();
    let text = state.stream.is_text();
    let Some(sink) = state.sink.take() else {
        return end(shared, state, ExportOutcome::Failed(String::new()));
    };
    let outcome = sink.complete(&finish, rows, text);
    end(shared, state, outcome);
}

/// It ended: what's left of the file removed (unless complete), the worker let go, the
/// operation ended, the view woken.
fn end(shared: &Shared, state: &mut State, outcome: ExportOutcome) {
    if let Some(sink) = state.sink.take() {
        sink.discard();
    }
    if let (Some(ticket), Some(client)) = (state.ticket.take(), &state.client) {
        client.cancel(ticket);
    }
    state.client = None;
    if let Some((log, id)) = &shared.activity {
        let ended = match &outcome {
            ExportOutcome::Written { .. } | ExportOutcome::Copied { .. } => Outcome::Done,
            ExportOutcome::Cancelled => Outcome::Stopped,
            ExportOutcome::Failed(why) => Outcome::Failed(why.clone()),
        };
        log.activities().end(*id, ended);
    }
    state.outcome = Some(outcome);
    (shared.wake)();
}

/// What writes `request`'s rows of `columns`.
fn writer(request: &ExportRequest, columns: &[GridColumn]) -> ExportWriter {
    ExportWriter::new(
        &request.format,
        &request.table.name,
        columns,
        request.sql.clone(),
        request.options,
    )
}

#[cfg(test)]
#[path = "../tests/unit/export.rs"]
mod tests;
