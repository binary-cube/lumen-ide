//! A driver's end of the pipe, for drivers written in Rust: any `database` [`Driver`]
//! served on a reader and a writer (its stdin and stdout) — the handshake, one session,
//! each request answered in turn, a `cancel` reaching the request it names while it runs;
//! a console's rows (protocol 2) sent as they come, in `$/rows` notifications of at most
//! [`CHUNK_ROWS`] rows or about [`CHUNK_BYTES`] each, before the request's answer. It ends
//! at `exit` or when its input closes. A driver in another language does the same
//! by the protocol's description (`docs/plan/03-sistem-module.md`).

use std::collections::HashSet;
use std::io::{self, BufReader, Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};

use database::{Cancel, Collect, DbError, Driver, Flow, RowSink, Secret, Session};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use zeroize::Zeroize;

pub use crate::driver::{CHUNK_BYTES, CHUNK_ROWS};
use crate::dto;
use crate::wire::{self, ErrorObject, code, method};

/// What the reader hands the requests' loop: a request's bytes, or the end.
enum Next {
    Request(Vec<u8>),
    End,
}

/// The request running and the cancels come for those waiting.
#[derive(Default)]
struct Running {
    now: Option<(u64, Cancel)>,
    cancelled: HashSet<u64>,
}

/// Serves `driver` (described by `description`, speaking the protocol's versions
/// `versions`) on `input` and `output` until `exit` or the input's end.
pub fn serve(
    driver: Arc<dyn Driver>,
    description: dto::Description,
    versions: dto::Versions,
    input: impl Read + Send + 'static,
    mut output: impl Write,
) -> io::Result<()> {
    let running = Arc::new(Mutex::new(Running::default()));
    let (tx, rx) = mpsc::channel();
    let watching = Arc::clone(&running);
    std::thread::Builder::new()
        .name("driver-input".to_owned())
        .spawn(move || read_requests(BufReader::new(input), &watching, &tx))?;
    let mut session: Option<Box<dyn Session>> = None;
    // The version chosen at the handshake (none yet: the first's rules).
    let mut protocol = versions.min.min(1);
    while let Ok(Next::Request(mut body)) = rx.recv() {
        let mut served = Served {
            driver: &driver,
            description: &description,
            versions,
            session: &mut session,
            protocol: &mut protocol,
            output: &mut output,
        };
        let answer = answer(&mut served, &running, &body);
        // The request's bytes wiped (a password may be in them).
        body.zeroize();
        if let Some(answer) = answer {
            wire::write_frame(&mut output, &answer)?;
        }
    }
    drop(session);
    Ok(())
}

/// Reads the frames: a `cancel` reaches the request it names at once (running, or still
/// to come), the others go to the loop in order.
fn read_requests(mut input: impl io::BufRead, running: &Mutex<Running>, tx: &mpsc::Sender<Next>) {
    while let Ok(Some(mut body)) = wire::read_frame(&mut input) {
        let (cancel, exit) = match serde_json::from_slice::<wire::Incoming<'_>>(&body) {
            Ok(m) if m.method == Some(method::CANCEL) => {
                let id =
                    m.params.and_then(|p| serde_json::from_str::<dto::CancelParams>(p.get()).ok());
                (id.map(|p| p.id), false)
            }
            Ok(m) => (None, m.method == Some(method::EXIT)),
            // Answered by the loop (a parse error).
            Err(_) => (None, false),
        };
        if exit {
            body.zeroize();
            break;
        }
        if let Some(id) = cancel {
            let mut running = running.lock().unwrap_or_else(PoisonError::into_inner);
            match &running.now {
                Some((now, cancel)) if *now == id => cancel.cancel(),
                _ => {
                    running.cancelled.insert(id);
                }
            }
            continue;
        }
        if tx.send(Next::Request(body)).is_err() {
            break;
        }
    }
    let _ = tx.send(Next::End);
}

fn params<P: DeserializeOwned>(params: Option<&RawValue>) -> Result<P, ErrorObject> {
    let text = params.map_or("null", RawValue::get);
    serde_json::from_str(text).map_err(|e| ErrorObject::new(code::INVALID_PARAMS, e.to_string()))
}

/// What answering a request needs: the driver, its session, the version spoken and where
/// its notifications are written.
struct Served<'a> {
    driver: &'a Arc<dyn Driver>,
    description: &'a dto::Description,
    versions: dto::Versions,
    session: &'a mut Option<Box<dyn Session>>,
    protocol: &'a mut u32,
    output: &'a mut dyn Write,
}

fn result<R: Serialize>(
    value: Result<R, DbError>,
    protocol: u32,
) -> Result<serde_json::Value, ErrorObject> {
    match value {
        Ok(value) => serde_json::to_value(value)
            .map_err(|e| ErrorObject::new(code::INTERNAL, format!("an answer not written: {e}"))),
        Err(e) => Err(ErrorObject::from_db(&e, protocol)),
    }
}

/// The answer to one message (none for a notification).
fn answer(served: &mut Served<'_>, running: &Mutex<Running>, body: &[u8]) -> Option<Vec<u8>> {
    let message = match serde_json::from_slice::<wire::Incoming<'_>>(body) {
        Ok(message) => message,
        Err(e) => {
            let error = ErrorObject::new(code::PARSE, e.to_string());
            return Some(wire::answer::<()>(None, Err(&error)));
        }
    };
    let Some(name) = message.method else {
        let error = ErrorObject::new(code::INVALID_REQUEST, "not a request");
        return Some(wire::answer::<()>(message.id, Err(&error)));
    };
    // A notification it doesn't know: nothing to answer.
    let id = message.id?;
    let cancel = Cancel::default();
    {
        let mut running = running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.cancelled.remove(&id) {
            cancel.cancel();
        }
        // Requests run in the order sent: a cancel for an earlier one came too late.
        running.cancelled.retain(|c| *c > id);
        running.now = Some((id, cancel.clone()));
    }
    let outcome = handle(served, id, name, message.params, &cancel);
    running.lock().unwrap_or_else(PoisonError::into_inner).now = None;
    Some(match outcome {
        Ok(value) => wire::answer(Some(id), Ok(&value)),
        Err(error) => wire::answer::<()>(Some(id), Err(&error)),
    })
}

fn handle(
    served: &mut Served<'_>,
    id: u64,
    name: &str,
    raw: Option<&RawValue>,
    cancel: &Cancel,
) -> Result<serde_json::Value, ErrorObject> {
    let protocol = *served.protocol;
    if cancel.is_cancelled() {
        return Err(ErrorObject::from_db(&DbError::Cancelled, protocol));
    }
    match name {
        method::INITIALIZE => {
            let asked: dto::Initialize = params(raw)?;
            let versions = served.versions;
            match versions.common(asked.protocol) {
                Some(chosen) => {
                    *served.protocol = chosen;
                    let initialized =
                        dto::Initialized { protocol: chosen, driver: served.description.clone() };
                    result(Ok(initialized), chosen)
                }
                None => {
                    let mut error = ErrorObject::new(
                        code::PROTOCOL,
                        format!(
                            "this driver speaks protocol {}–{}, the editor {}–{}",
                            versions.min, versions.max, asked.protocol.min, asked.protocol.max
                        ),
                    );
                    error.data = serde_json::to_value(versions).ok();
                    Err(error)
                }
            }
        }
        method::CONNECT => {
            if served.session.is_some() {
                return Err(ErrorObject::new(code::NOT_CONNECTED, "already connected"));
            }
            let mut asked: dto::Connect = params(raw)?;
            let secret = Secret::from(std::mem::take(&mut asked.password));
            let target: database::Target = std::mem::take(&mut asked.target).into();
            let opened = served
                .driver
                .connect(&target, &secret)
                .map_err(|e| ErrorObject::from_db(&e, protocol))?;
            let connected = dto::Connected { server: opened.server(), version: None };
            *served.session = Some(opened);
            result(Ok(connected), protocol)
        }
        _ if protocol < 2 && method::SINCE_2.contains(&name) => Err(ErrorObject::new(
            code::METHOD_NOT_FOUND,
            format!("`{name}` comes with protocol 2 (this session speaks {protocol})"),
        )),
        _ if protocol < 3 && method::SINCE_3.contains(&name) => Err(ErrorObject::new(
            code::METHOD_NOT_FOUND,
            format!("`{name}` comes with protocol 3 (this session speaks {protocol})"),
        )),
        _ => {
            let Some(session) = served.session.as_mut() else {
                return Err(ErrorObject::new(code::NOT_CONNECTED, "not connected"));
            };
            let context = Context { id, protocol, cancel };
            requested(session.as_mut(), name, raw, &context, &mut *served.output)
        }
    }
}

/// A request's own: its id (its rows' notifications name it), the version spoken, its
/// cancel.
struct Context<'a> {
    id: u64,
    protocol: u32,
    cancel: &'a Cancel,
}

/// A sink sending its rows as `$/rows` notifications: a set's columns with its first rows
/// (alone, for a set without any), at most [`CHUNK_ROWS`] rows or about [`CHUNK_BYTES`] a
/// message. A write failing (the editor gone) stops the statement.
struct Streamed<'a> {
    id: u64,
    output: &'a mut dyn Write,
    columns: Option<Vec<dto::Column>>,
    rows: Vec<Vec<dto::Value>>,
    bytes: usize,
    failed: bool,
}

impl<'a> Streamed<'a> {
    fn new(id: u64, output: &'a mut dyn Write) -> Self {
        Self { id, output, columns: None, rows: Vec::new(), bytes: 0, failed: false }
    }

    fn flush(&mut self) {
        if self.failed || (self.columns.is_none() && self.rows.is_empty()) {
            return;
        }
        let chunk = dto::RowsChunk {
            id: self.id,
            columns: self.columns.take(),
            rows: std::mem::take(&mut self.rows),
        };
        self.bytes = 0;
        let body = wire::request(None, method::ROWS, &chunk);
        if wire::write_frame(self.output, &body).is_err() {
            self.failed = true;
        }
    }
}

/// About how many bytes a value takes in JSON.
fn size_of(value: &database::Value) -> usize {
    use database::Value as V;
    12 + match value {
        V::Null | V::Bool(_) | V::Int(_) | V::Float(_) => 12,
        V::Text(t) | V::Json(t) | V::Other(t) => t.len(),
        V::Bytes(b) => b.len().div_ceil(3) * 4,
    }
}

impl RowSink for Streamed<'_> {
    fn columns(&mut self, columns: &[database::Column]) {
        self.flush();
        self.columns = Some(columns.iter().cloned().map(Into::into).collect());
    }

    fn row(&mut self, values: Vec<database::Value>) -> Flow {
        self.bytes += values.iter().map(size_of).sum::<usize>();
        self.rows.push(values.into_iter().map(Into::into).collect());
        if self.rows.len() >= CHUNK_ROWS || self.bytes >= CHUNK_BYTES {
            self.flush();
        }
        if self.failed { Flow::Stop } else { Flow::Continue }
    }
}

/// A run's or a fetch's outcome, once its last rows went.
fn ran(
    session: &dyn Session,
    outcome: Result<database::RunOutcome, DbError>,
    mut streamed: Streamed<'_>,
    protocol: u32,
) -> Result<serde_json::Value, ErrorObject> {
    streamed.flush();
    if streamed.failed {
        return Err(ErrorObject::new(code::INTERNAL, "the rows couldn't be sent"));
    }
    result(
        outcome.map(|o| dto::RunOutcome {
            in_transaction: Some(session.in_transaction()),
            ..dto::RunOutcome::from(o)
        }),
        protocol,
    )
}

/// A request to the open session.
fn requested(
    session: &mut dyn Session,
    name: &str,
    raw: Option<&RawValue>,
    context: &Context<'_>,
    output: &mut dyn Write,
) -> Result<serde_json::Value, ErrorObject> {
    let (cancel, protocol) = (context.cancel, context.protocol);
    macro_rules! answered {
        ($value:expr $(,)?) => {
            result($value, protocol)
        };
    }
    match name {
        method::RUN => {
            let asked: dto::Run = params(raw)?;
            let options = database::RunOptions {
                fetch_limit: asked.fetch_limit,
                keep_cursor: asked.keep_cursor,
            };
            let mut streamed = Streamed::new(context.id, output);
            let outcome = session.run(&asked.sql, &options, &mut streamed, cancel);
            ran(session, outcome, streamed, protocol)
        }
        method::FETCH => {
            let asked: dto::Fetch = params(raw)?;
            let mut streamed = Streamed::new(context.id, output);
            let cursor = database::CursorToken(asked.cursor);
            let outcome = session.fetch(&cursor, asked.rows, &mut streamed, cancel);
            ran(session, outcome, streamed, protocol)
        }
        method::CLOSE_CURSOR => {
            let asked: dto::CursorParams = params(raw)?;
            serialized(session.close_cursor(&database::CursorToken(asked.cursor)), protocol)
        }
        method::EXPLAIN => {
            let asked: dto::Explain = params(raw)?;
            serialized(
                session
                    .explain(&asked.sql, asked.analyze, cancel)
                    .map(|plan| plan.into_iter().map(dto::PlanNode::from).collect::<Vec<_>>()),
                protocol,
            )
        }
        method::PROFILE => {
            let asked: dto::ProfileParams = params(raw)?;
            let options: database::profile::ProfileOptions = asked.options.into();
            serialized(
                session.profile(&asked.sql, &options, cancel).map(|p| dto::Profile::from(&p)),
                protocol,
            )
        }
        method::TABLE_STATS => {
            let asked: dto::TableStatsParams = params(raw)?;
            let tables: Vec<database::TableRef> =
                asked.tables.into_iter().map(Into::into).collect();
            serialized(
                session
                    .table_stats(&tables, cancel)
                    .map(|all| all.iter().map(dto::TableStats::from).collect::<Vec<_>>()),
                protocol,
            )
        }
        method::BEGIN => {
            let asked: dto::Begin = params(raw)?;
            serialized(session.begin(&asked.into()), protocol)
        }
        method::COMMIT => serialized(session.commit(), protocol),
        method::ROLLBACK => serialized(session.rollback(), protocol),
        method::SET_SCHEMA => {
            let asked: dto::SchemaParams = params(raw)?;
            serialized(session.set_schema(&asked.schema), protocol)
        }
        method::CURRENT => serialized(session.current().map(dto::Place::from), protocol),
        method::DATABASES => serialized(session.databases(), protocol),
        method::CATALOG_COLUMNS => {
            let asked: dto::CatalogParams = params(raw)?;
            serialized(
                session
                    .catalog_columns(asked.schema.as_deref(), cancel)
                    .map(|all| all.into_iter().map(dto::TableColumns::from).collect::<Vec<_>>()),
                protocol,
            )
        }
        method::ROUTINES => {
            let asked: dto::CatalogParams = params(raw)?;
            serialized(
                session
                    .routines(asked.schema.as_deref(), cancel)
                    .map(|all| all.into_iter().map(dto::Routine::from).collect::<Vec<_>>()),
                protocol,
            )
        }
        method::CHILDREN => {
            let asked: dto::PathParams = params(raw)?;
            answered!(
                session
                    .children(&asked.path)
                    .map(|nodes| nodes.into_iter().map(dto::Node::from).collect::<Vec<_>>()),
            )
        }
        method::TABLES => {
            let asked: dto::PathParams = params(raw)?;
            answered!(
                session
                    .tables(&asked.path)
                    .map(|tables| tables.into_iter().map(dto::TableInfo::from).collect::<Vec<_>>()),
            )
        }
        method::STRUCTURE => {
            let asked: dto::TableParams = params(raw)?;
            answered!(session.structure(&asked.table.into()).map(dto::Structure::from))
        }
        method::READ => {
            let asked: dto::Read = params(raw)?;
            let mut rows = Collect::default();
            let read = session.read(&asked.table.into(), &asked.page.into(), &mut rows, cancel);
            answered!(read.and_then(|outcome| {
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
                Ok(rows_of(rows, outcome))
            }))
        }
        method::COUNT => {
            let asked: dto::Count = params(raw)?;
            let filter: Option<database::Filter> = asked.filter.map(Into::into);
            answered!(session.count(&asked.table.into(), filter.as_ref(), cancel))
        }
        method::EXECUTE => {
            let asked: dto::Execute = params(raw)?;
            let mut rows = Collect {
                limit: asked.max_rows.map(|n| usize::try_from(n).unwrap_or(usize::MAX)),
                ..Collect::default()
            };
            let ran = session.execute(&asked.sql, &mut rows, cancel);
            answered!(ran.map(|outcome| rows_of(rows, outcome)))
        }
        method::APPLY => {
            let asked: dto::Apply = params(raw)?;
            answered!(session.apply(&asked.changes.into()).map(dto::Applied::from))
        }
        method::ALTER => {
            let asked: dto::Alter = params(raw)?;
            let changes: Vec<database::SchemaChange> =
                asked.changes.into_iter().map(Into::into).collect();
            answered!(session.alter(&changes).map(dto::Applied::from))
        }
        method::PLAN_ALTER => {
            let asked: dto::Alter = params(raw)?;
            let changes: Vec<database::SchemaChange> =
                asked.changes.into_iter().map(Into::into).collect();
            answered!(session.plan_alter(&changes).map(|plan| plan.map(dto::Script::from)))
        }
        method::REBUILD_SOURCE => {
            let asked: dto::TableParams = params(raw)?;
            answered!(session.rebuild_source(&asked.table.into()).map(dto::RebuildSource::from))
        }
        other => Err(ErrorObject::new(code::METHOD_NOT_FOUND, format!("no method `{other}`"))),
    }
}

fn serialized<R: Serialize>(
    value: Result<R, DbError>,
    protocol: u32,
) -> Result<serde_json::Value, ErrorObject> {
    result(value, protocol)
}

fn rows_of(rows: Collect, outcome: database::Outcome) -> dto::Rows {
    dto::Rows {
        columns: rows.columns.into_iter().map(Into::into).collect(),
        rows: rows.rows.into_iter().map(|row| row.into_iter().map(Into::into).collect()).collect(),
        keys: rows.keys.into_iter().map(Into::into).collect(),
        affected: outcome.affected,
        stopped: outcome.stopped,
    }
}

#[cfg(test)]
#[path = "../tests/unit/server.rs"]
mod tests;
