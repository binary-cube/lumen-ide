//! Statements run for the console: plain or through a cursor, a page fetched, the rows
//! streamed to the sink; the notices, the history, a profile's undoing, work guarded by a
//! savepoint.

use std::sync::PoisonError;

use database::profile::{Profile, ProfileNote};
use database::run::{Limited, Overflow};
use database::sql::{Flavor, StatementKind};
use database::{
    Cancel, Collect, Column, CursorToken, DbError, Flow, Notice, Outcome, RowSink, RunOptions,
    RunOutcome, Value,
};
use futures_util::StreamExt;
use tokio_postgres::{NoTls, SimpleQueryMessage};

use crate::{
    CANCEL_CHECK, OnStop, OpenCursor, PageSink, PgSession, Transport, console, error, profile,
    read_only_refusal, stats, value,
};

impl PgSession {
    pub(crate) fn flavor(&self) -> Flavor {
        Flavor::Postgres { version: Some(self.version) }
    }

    /// Statements run for their effect alone.
    pub(crate) fn simple(&self, sql: &str) -> Result<(), DbError> {
        self.runtime.block_on(self.client.batch_execute(sql)).map_err(error)
    }

    /// A setting's value (`SHOW name`), `None` if it can't be read.
    pub(crate) fn show(&self, name: &str) -> Option<String> {
        let mut rows = Collect::default();
        self.stream(&format!("SHOW {name}"), None, &mut rows, &Cancel::default(), OnStop::Cancel)
            .ok()?;
        rows.rows.first().and_then(|r| r.first()).map(Value::display)
    }

    /// A profile's run undone: its savepoint inside the user's transaction, or its own
    /// transaction (a failure leaves nothing more to undo).
    pub(crate) fn undo_profile(&mut self) {
        if self.in_tx {
            let point = profile::SAVEPOINT;
            let _ = self.simple(&format!("ROLLBACK TO SAVEPOINT {point}"));
            let _ = self.simple(&format!("RELEASE SAVEPOINT {point}"));
        } else {
            let _ = self.simple("ROLLBACK");
        }
    }

    /// `work` run as is, or inside the user's transaction under [`stats::SAVEPOINT`]
    /// (undone if it fails, so an auxiliary query never aborts the user's transaction).
    pub(crate) fn guarded<T>(
        &mut self,
        work: impl FnOnce(&Self) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        if !self.in_tx {
            return work(self);
        }
        let point = stats::SAVEPOINT;
        self.simple(&format!("SAVEPOINT {point}"))?;
        let result = work(self);
        if result.is_err() {
            let _ = self.simple(&format!("ROLLBACK TO SAVEPOINT {point}"));
        }
        let _ = self.simple(&format!("RELEASE SAVEPOINT {point}"));
        result
    }

    /// Where `pg_stat_statements` is installed (`None`: it isn't, or it can't be told),
    /// read once.
    pub(crate) fn statement_stats_schema(&mut self) -> Option<String> {
        if let Some(known) = &self.statement_stats {
            return known.clone();
        }
        let found = self.guarded(|s| s.catalog(stats::EXTENSION_SQL, &[]));
        let schema = match found {
            Ok(rows) => rows.into_iter().next().and_then(|r| r.get(1).cloned().flatten()),
            Err(e) => {
                tracing::debug!(error = %e, "pg_stat_statements wasn't looked for");
                None
            }
        };
        self.statement_stats = Some(schema.clone());
        schema
    }

    /// The server's history of query `id` (`pg_stat_statements` in `schema`); a failure
    /// leaves it unknown (the extension not loaded noted as missing).
    pub(crate) fn read_history(
        &mut self,
        schema: &str,
        id: i64,
        profile: &mut Profile,
    ) -> Option<database::profile::StatementStats> {
        let sql = stats::history_sql(self.version, schema)?;
        let id = id.to_string();
        match self.guarded(|s| s.catalog(&sql, &[id.as_str()])) {
            Ok(rows) => rows.first().and_then(|r| stats::history_row(r)),
            Err(e) => {
                let text = e.to_string();
                if text.contains("shared_preload_libraries") {
                    profile.note(ProfileNote::NoStatementStats);
                }
                tracing::debug!(%text, "the query's history wasn't read");
                None
            }
        }
    }

    pub(crate) fn take_notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect()
    }

    /// The columns' types of `query` (one statement, prepared; `None`: it can't be).
    pub(crate) fn types(
        &self,
        query: &str,
        cancel: &Cancel,
    ) -> Result<Option<Vec<Column>>, DbError> {
        let (client, token, transport) = (&self.client, &self.cancel, &self.transport);
        // Preparing waits for the table's locks too: cancelled as a run is.
        let prepared = self.runtime.block_on(async move {
            let prepare = client.prepare(query);
            futures_util::pin_mut!(prepare);
            let mut tick = tokio::time::interval(CANCEL_CHECK);
            let mut cancelled = false;
            loop {
                tokio::select! {
                    prepared = &mut prepare => return prepared.ok(),
                    _ = tick.tick() => {
                        if cancel.is_cancelled() && !cancelled {
                            cancelled = true;
                            let _ = match transport {
                                Transport::Plain => token.cancel_query(NoTls).await,
                                Transport::Tls(tls) => token.cancel_query(tls.clone()).await,
                            };
                        }
                    }
                }
            }
        });
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        Ok(prepared.map(|s| {
            s.columns()
                .iter()
                .map(|c| Column {
                    name: c.name().to_owned(),
                    type_name: c.type_().name().to_owned(),
                })
                .collect()
        }))
    }

    /// The console's cursor closed (its own transaction with it); a failure leaves it gone
    /// all the same (an aborted transaction closed it).
    pub(crate) fn release_cursor(&mut self) {
        if let Some(cursor) = self.cursor.take() {
            let _ = self.simple(&console::close(&cursor.name));
            if cursor.own_tx {
                let _ = self.simple("COMMIT");
            }
        }
    }

    /// `read` from a cursor: declared (in a read-only transaction of its own outside the
    /// user's), its first page fetched. `None`: it wasn't for a cursor after all (a write
    /// refused in the read-only transaction, before any row): run it plainly.
    pub(crate) fn run_cursor(
        &mut self,
        read: &console::Statement<'_>,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Option<RunOutcome>, DbError> {
        let own_tx = !self.in_tx;
        let types = self.types(read.body, cancel)?;
        if own_tx {
            self.simple("BEGIN READ ONLY")?;
        }
        let name = console::cursor_name(self.next_cursor);
        self.next_cursor += 1;
        let (declare, prefix) = console::declare(&name, read.body);
        if let Err(e) = self.simple(&declare) {
            if own_tx {
                let _ = self.simple("ROLLBACK");
                if read_only_refusal(&e) {
                    return Ok(None);
                }
            }
            return Err(console::shifted(e, prefix, read.leading));
        }
        self.cursor =
            Some(OpenCursor { name, columns: types.unwrap_or_default(), pending: None, own_tx });
        match self.fetch_page(options.fetch_limit, sink, cancel) {
            Ok(outcome) => Ok(Some(outcome)),
            Err((e, 0)) if own_tx && read_only_refusal(&e) => Ok(None),
            Err((e, _)) => Err(e),
        }
    }

    /// The cursor's next `rows` rows to `sink` (the one kept from before first), one more
    /// read and kept to know the rest; closed once none are left. Failing: the error and
    /// the rows given before (the cursor gone, its own transaction rolled back).
    pub(crate) fn fetch_page(
        &mut self,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, (DbError, u64)> {
        let Some(cursor) = self.cursor.as_mut() else {
            return Err((DbError::Query("no cursor open".to_owned()), 0));
        };
        let mut page = PageSink {
            sink,
            announced: false,
            limit: u64::from(rows.max(1)),
            delivered: 0,
            read: 0,
            pending: None,
            stopped: false,
        };
        if !cursor.columns.is_empty() {
            page.columns(&cursor.columns);
        }
        if let Some(row) = cursor.pending.take() {
            page.deliver(row);
        }
        let need = page.limit - page.delivered + 1;
        let (name, types) = (cursor.name.clone(), cursor.columns.clone());
        let types = (!types.is_empty()).then_some(types);
        let result =
            self.stream(&console::fetch(&name, need), types, &mut page, cancel, OnStop::Drain);
        let (delivered, read, pending, stopped) =
            (page.delivered, page.read, page.pending, page.stopped);
        if let Err(e) = result {
            if let Some(cursor) = self.cursor.take()
                && cursor.own_tx
            {
                let _ = self.simple("ROLLBACK");
            }
            return Err((e, delivered));
        }
        let left = read >= need && !stopped;
        if left {
            if let Some(cursor) = self.cursor.as_mut() {
                cursor.pending = pending;
            }
        } else {
            self.release_cursor();
        }
        Ok(RunOutcome {
            affected: None,
            rows: delivered,
            truncated: left,
            more: left.then_some(CursorToken(name)),
            stopped,
            notices: Vec::new(),
        })
    }

    /// `statement` streamed as it is (no cursor): a single query stopped one row past the
    /// page, anything else read to its end, the rows past the page dropped.
    pub(crate) fn run_plain(
        &mut self,
        statement: &str,
        read: &console::Statement<'_>,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let query = read.kind == StatementKind::Query && !read.several;
        let types = if read.several { None } else { self.types(statement, cancel)? };
        if !query {
            self.cursors.clear();
        }
        let overflow = if query { Overflow::Stop } else { Overflow::Drop };
        let mut limited = Limited::new(sink, options.fetch_limit, overflow);
        let outcome = self.stream(statement, types, &mut limited, cancel, OnStop::Cancel)?;
        Ok(RunOutcome {
            affected: outcome.affected,
            rows: limited.delivered,
            truncated: limited.truncated,
            more: None,
            stopped: limited.stopped,
            notices: Vec::new(),
        })
    }

    /// Runs `query` through the simple protocol (every value as text, typed by `types` when
    /// they fit the set), its rows to `sink`; `cancel` asks the server to stop, and so does
    /// the sink's stop with [`OnStop::Cancel`].
    pub(crate) fn stream(
        &self,
        query: &str,
        types: Option<Vec<Column>>,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
        on_stop: OnStop,
    ) -> Result<Outcome, DbError> {
        let (client, token, transport) = (&self.client, &self.cancel, &self.transport);
        self.runtime.block_on(async move {
            let stream = client.simple_query_raw(query).await.map_err(error)?;
            futures_util::pin_mut!(stream);
            let mut outcome = Outcome::default();
            let mut columns: Option<Vec<Column>> = None;
            let mut done_set = false;
            let mut tick = tokio::time::interval(CANCEL_CHECK);
            let mut cancelled = false;
            loop {
                let message = tokio::select! {
                    message = stream.next() => message,
                    _ = tick.tick() => {
                        if cancel.is_cancelled() && !cancelled {
                            cancelled = true;
                            let _ = match transport {
                                Transport::Plain => token.cancel_query(NoTls).await,
                                Transport::Tls(tls) => token.cancel_query(tls.clone()).await,
                            };
                        }
                        continue;
                    }
                };
                match message {
                    None => break,
                    Some(Err(e)) => {
                        let e = error(e);
                        return if cancelled || e == DbError::Cancelled {
                            Err(DbError::Cancelled)
                        } else {
                            Err(e)
                        };
                    }
                    Some(Ok(SimpleQueryMessage::Row(row))) => {
                        // A new result set: its columns first (typed when prepared).
                        if columns.is_none() || done_set {
                            let named: Vec<Column> = row
                                .columns()
                                .iter()
                                .map(|c| Column {
                                    name: c.name().to_owned(),
                                    type_name: String::new(),
                                })
                                .collect();
                            let typed = types
                                .clone()
                                .filter(|t| t.len() == named.len() && columns.is_none());
                            let set = typed.unwrap_or(named);
                            sink.columns(&set);
                            columns = Some(set);
                            done_set = false;
                        }
                        let set = columns.as_deref().unwrap_or_default();
                        let values = (0..row.len())
                            .map(|i| {
                                value(set.get(i).map_or("", |c| c.type_name.as_str()), row.get(i))
                            })
                            .collect();
                        outcome.rows += 1;
                        if sink.row(values) == Flow::Stop && on_stop == OnStop::Cancel {
                            // Enough: the server stops working on the rest.
                            outcome.stopped = true;
                            let _ = match transport {
                                Transport::Plain => token.cancel_query(NoTls).await,
                                Transport::Tls(tls) => token.cancel_query(tls.clone()).await,
                            };
                            break;
                        }
                    }
                    Some(Ok(SimpleQueryMessage::CommandComplete(count))) => {
                        outcome.affected = Some(count);
                        done_set = true;
                    }
                    Some(Ok(_)) => {}
                }
            }
            // A query without rows (DDL, an UPDATE): its columns all the same (none).
            if columns.is_none() {
                sink.columns(&types.unwrap_or_default());
            }
            Ok(outcome)
        })
    }
}
