//! A console's work on results already there, on its pool: pages, exports, edits.

use super::*;

impl Inner {
    /// Result `id`'s next page: from its cursor, or its query run again over its result from
    /// the next row.
    pub(super) fn fetch_more(self: &Arc<Self>, id: ServiceId) {
        let Some((statement, more, view, from)) = self
            .results()
            .get(id)
            .map(|s| (s.statement.clone(), s.more.clone(), s.view.clone(), s.rows.len()))
        else {
            self.done();
            return;
        };
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut session = match self.reuse() {
            Ok(session) => session,
            Err(e) => return self.result_failed(id, &e),
        };
        let limit = self.state().settings.fetch_limit;
        let at = self.runtime.now();
        let mut sink = Collecting::new(self, Destination::Append(id), &statement, 0..0);
        sink.current = Some(id);
        sink.sets.push(id);
        let result = match &more {
            More::Cursor(token) => session.fetch(token, limit, &mut sink, &cancel),
            _ => {
                if !matches!(more, More::Rerun) {
                    drop(sink);
                    self.put_back(session);
                    return self.result_ended(id);
                }
                self.results().cursors_closed();
                match self.wrapped(&statement, &view, Some(limit.saturating_add(1)), from) {
                    Ok(text) => {
                        let options = RunOptions { fetch_limit: limit, keep_cursor: false };
                        session.run(&text, &options, &mut sink, &cancel)
                    }
                    Err(e) => Err(e),
                }
            }
        };
        let total = self.runtime.now().saturating_duration_since(at);
        let execution = sink.first.map_or(total, |f| f.saturating_duration_since(at));
        sink.flush();
        drop(sink);
        self.put_back(session);
        match result {
            Ok(outcome) => {
                let rerun = matches!(more, More::Rerun);
                {
                    let mut results = self.results();
                    if let Some(set) = results.get_mut(id) {
                        set.rows.seal();
                        set.more = match outcome.more {
                            Some(token) => More::Cursor(token),
                            None if outcome.truncated && rerun => More::Rerun,
                            None if outcome.truncated => More::Lost,
                            None => More::None,
                        };
                        set.loading = false;
                        set.error = None;
                        set.touch();
                    }
                }
                self.log.push(self.line(
                    LogKind::Result,
                    i18n::tr!(
                        "console.rows_retrieved",
                        count = outcome.rows,
                        from = from + 1,
                        total = millis(total),
                        exec = millis(execution),
                        fetch = millis(total.saturating_sub(execution))
                    ),
                ));
                self.done();
            }
            Err(e) => self.result_failed(id, &e),
        }
    }

    /// Result `id` read again from its start, filtered and ordered by the engine as `view`
    /// says (its first page).
    pub(super) fn requery(self: &Arc<Self>, id: ServiceId, view: &ServerView) {
        let Some(statement) = self.results().get(id).map(|s| s.statement.clone()) else {
            self.done();
            return;
        };
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut session = match self.reuse() {
            Ok(session) => session,
            Err(e) => return self.result_failed(id, &e),
        };
        self.results().cursors_closed();
        let limit = self.state().settings.fetch_limit;
        let prefix = prefix(&self.state().snapshot.place);
        let text = match self.wrapped(&statement, view, Some(limit.saturating_add(1)), 0) {
            Ok(text) => text,
            Err(e) => {
                self.put_back(session);
                return self.result_failed(id, &e);
            }
        };
        self.log
            .push(self.line(LogKind::Statement, text.clone()).with_prefix(prefix).as_code("sql"));
        let at = self.runtime.now();
        let mut sink = Collecting::new(self, Destination::Replace(id), &statement, 0..0);
        let options = RunOptions { fetch_limit: limit, keep_cursor: false };
        let result = session.run(&text, &options, &mut sink, &cancel);
        let total = self.runtime.now().saturating_duration_since(at);
        let execution = sink.first.map_or(total, |f| f.saturating_duration_since(at));
        sink.flush();
        drop(sink);
        self.put_back(session);
        match result {
            Ok(outcome) => {
                {
                    let mut results = self.results();
                    if let Some(set) = results.get_mut(id) {
                        // No row came: none to keep of the old ones either.
                        if outcome.rows == 0 {
                            set.rows.clear();
                            set.dropped = false;
                        }
                        set.rows.seal();
                        set.view = view.clone();
                        set.more = if outcome.truncated { More::Rerun } else { More::None };
                        set.execution = execution;
                        set.fetching = total.saturating_sub(execution);
                        set.loading = false;
                        set.error = None;
                        set.touch();
                    }
                }
                self.log.push(self.line(
                    LogKind::Result,
                    i18n::tr!(
                        "console.rows_retrieved",
                        count = outcome.rows,
                        from = 1,
                        total = millis(total),
                        exec = millis(execution),
                        fetch = millis(total.saturating_sub(execution))
                    ),
                ));
                self.done();
            }
            Err(e) => self.result_failed(id, &e),
        }
    }

    /// `statement` wrapped by the session's dialect (its next page, its order and filter).
    pub(super) fn wrapped(
        &self,
        statement: &str,
        view: &ServerView,
        limit: Option<u32>,
        offset: u64,
    ) -> Result<String, DbError> {
        let (dialect, flavor) = {
            let state = self.state();
            (state.dialect.clone(), state.settings.flavor)
        };
        // The session's own, else its source's (a driver's sessions may leave it to it).
        let dialect: Arc<dyn Dialect> =
            dialect.unwrap_or_else(|| database::console::dialect_for(flavor));
        dialect.select_over(statement, view.filter.as_ref(), &view.order, limit, offset)
    }

    /// A result's read failed: said in the Output and on its tab.
    pub(super) fn result_failed(self: &Arc<Self>, id: ServiceId, e: &DbError) {
        let why = message(e);
        self.log.push(self.line(LogKind::Error, why.clone()));
        if let Some(set) = self.results().get_mut(id) {
            set.loading = false;
            set.error = Some(why);
            set.touch();
        }
        self.done();
    }

    pub(super) fn result_ended(self: &Arc<Self>, id: ServiceId) {
        if let Some(set) = self.results().get_mut(id) {
            set.loading = false;
            set.touch();
        }
        self.done();
    }

    /// Every row of a pure query's result (filtered and ordered as `request.view` says),
    /// run again and written as they come — never all of them in memory.
    pub(super) fn export(self: &Arc<Self>, request: &ExportRequest, export: &ExportState) {
        let end = |outcome: ExportOutcome| {
            *export.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        };
        let Some((statement, title)) =
            self.results().get(request.result).map(|s| (s.statement.clone(), s.title.clone()))
        else {
            end(ExportOutcome::Failed(i18n::t("console.result_gone")));
            self.done();
            return;
        };
        let mut session = match self.reuse() {
            Ok(session) => session,
            Err(e) => {
                end(ExportOutcome::Failed(message(&e)));
                self.done();
                return;
            }
        };
        self.results().cursors_closed();
        let text = match self.wrapped(&statement, &request.view, None, 0) {
            Ok(text) => text,
            Err(e) => {
                self.put_back(session);
                end(ExportOutcome::Failed(message(&e)));
                self.done();
                return;
            }
        };
        let mut sink = match ExportSink::open(&request.target) {
            Ok(sink) => sink,
            Err(e) => {
                self.put_back(session);
                end(ExportOutcome::Failed(e.to_string()));
                self.done();
                return;
            }
        };
        let table = title.rsplit('.').next().unwrap_or("result").to_owned();
        let mut writer = Writing {
            stream: ExportWriter::new(
                &request.format,
                &table,
                &request.columns,
                request.sql.clone(),
                request.options,
            ),
            sink: &mut sink,
            wanted: request.columns.iter().map(|c| c.name.clone()).collect(),
            at: Vec::new(),
            block: Vec::new(),
            error: None,
            rows: &export.rows,
            wake: Arc::clone(self),
        };
        match writer.stream.begin() {
            Ok(begin) => {
                if let Err(e) = writer.sink.write_bytes(&begin) {
                    writer.error = Some(e.to_string());
                }
            }
            Err(why) => writer.error = Some(why),
        }
        let options = RunOptions { fetch_limit: u32::MAX, keep_cursor: false };
        let result = session.run(&text, &options, &mut writer, &export.cancel);
        writer.write_block();
        let text = writer.stream.is_text();
        let (mut error, written) = (writer.error.take(), writer.stream.written());
        let finish = match writer.stream.finish() {
            Ok(finish) => finish,
            Err(why) => {
                error = error.or(Some(why));
                Vec::new()
            }
        };
        drop(writer);
        self.put_back(session);
        let outcome = match (result, error) {
            (_, Some(why)) => {
                sink.discard();
                ExportOutcome::Failed(why)
            }
            (Err(DbError::Cancelled), None) => {
                sink.discard();
                ExportOutcome::Cancelled
            }
            (Ok(outcome), None) if outcome.stopped && export.cancel.is_cancelled() => {
                sink.discard();
                ExportOutcome::Cancelled
            }
            (Err(e), None) => {
                sink.discard();
                ExportOutcome::Failed(message(&e))
            }
            (Ok(_), None) => sink.complete(&finish, written, text),
        };
        end(outcome);
        self.done();
    }

    pub(super) fn close_cursor(self: &Arc<Self>, token: &CursorToken) {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(mut session) = session {
            if let Err(e) = session.close_cursor(token) {
                tracing::debug!("a console's cursor wasn't closed: {e}");
            }
            self.put_back(session);
        }
        self.done();
    }

    /// Result `id`'s table's structure read on the console's connection: whether its rows
    /// can be edited (its columns the table's, its key among them).
    pub(super) fn describe(self: &Arc<Self>, id: ServiceId) {
        let Some((shape, columns)) = self.results().get(id).and_then(|s| {
            let shape = s.shape.as_ref().ok()?.clone();
            Some((shape, s.columns.clone()))
        }) else {
            self.done();
            return;
        };
        let editing = match self.reuse() {
            Ok(mut session) => {
                let read = session.structure(&shape.table);
                self.put_back(session);
                match read {
                    Ok(structure) => {
                        match database::edits::editable_result(&shape, &columns, structure) {
                            Ok(editable) => Editability::Ready(Arc::new(editable)),
                            Err(why) => Editability::Refused(why),
                        }
                    }
                    Err(e) => Editability::Failed(message(&e)),
                }
            }
            Err(e) => Editability::Failed(message(&e)),
        };
        if let Some(set) = self.results().get_mut(id) {
            set.editing = editing;
            set.touch();
        }
        self.done();
    }

    /// The whole value of result `id`'s big cell (the row with `key`, result column
    /// `column`) read from its table by the row's key, kept for its editor.
    pub(super) fn read_whole(self: &Arc<Self>, id: ServiceId, key: &RowKey, column: usize) {
        let editable = self.results().get(id).and_then(|s| match &s.editing {
            Editability::Ready(e) => Some(Arc::clone(e)),
            _ => None,
        });
        let Some(editable) = editable else {
            self.done();
            return;
        };
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let read = self.reuse().and_then(|mut session| {
            let dialect = session
                .dialect()
                .unwrap_or_else(|| database::console::dialect_for(self.state().settings.flavor));
            let read = whole_query(dialect.as_ref(), &editable, key, column).and_then(|sql| {
                let mut first = FirstValue::default();
                session.execute(&sql, &mut first, &cancel)?;
                first.value.ok_or_else(|| DbError::Conflict(i18n::t("database.why_row_changed")))
            });
            self.put_back(session);
            read
        });
        let value = read.map(Arc::new).map_err(|e| message(&e));
        if let Some(set) = self.results().get_mut(id) {
            set.wholes.retain(|(k, c, _)| !(k == key && *c == column));
            set.wholes.push((key.clone(), column, value));
            set.touch();
        }
        self.done();
    }

    /// A result's changes submitted (see [`Runner::submit`]), said in the Output.
    pub(super) fn submit(self: &Arc<Self>, job: &SubmitJob) {
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut session = match self.reuse() {
            Ok(session) => session,
            Err(e) => return self.submitted(job, Err(e), false),
        };
        self.results().cursors_closed();
        let flavor = self.state().settings.flavor;
        let engine = session.dialect().map_or(flavor, |d| d.flavor());
        let prefix = prefix(&self.state().snapshot.place);
        for step in &job.script.steps {
            let masked = sql::mask_secrets(&step.sql, flavor).into_owned();
            self.log.push(
                self.line(LogKind::Statement, masked).with_prefix(prefix.clone()).as_code("sql"),
            );
        }
        // Manual: they open the console's transaction (as a first write does).
        let mut began = false;
        if job.tx == TxMode::Manual && !session.in_transaction() {
            let options = TxOptions { isolation: job.isolation, read_only: false };
            if let Err(e) = session.begin(&options) {
                self.put_back(session);
                return self.submitted(job, Err(e), false);
            }
            began = true;
        }
        let inside = session.in_transaction();
        let script = if inside {
            transaction::nested(&job.script, EDIT_SAVEPOINT)
        } else {
            job.script.clone()
        };
        let log = &self.log;
        let result = transaction::run(session.as_mut(), &script, &cancel, &mut |sql, e| {
            log.push(LogLine::new(
                self.runtime.clock(),
                LogKind::Warning,
                format!("{}: {}", sql::mask_secrets(sql, flavor), message(e)),
            ));
        });
        // Nothing of theirs is left in a transaction they opened.
        if result.is_err()
            && began
            && let Err(e) = session.rollback()
        {
            self.log.push(self.line(LogKind::Warning, message(&e)));
        }
        if result.is_ok() {
            // The console's transaction counts them (its Commit makes them permanent).
            let mut state = self.state();
            let mut tx = state.snapshot.tx;
            for (step, kind) in job.script.steps.iter().zip(&job.kinds) {
                let index = state.executed;
                state.executed += 1;
                tx = tx.on_statement(index, *kind, true, job.tx, engine);
                if inside {
                    state.tx_statements.push(sql::mask_secrets(&step.sql, flavor).into_owned());
                }
            }
            let excess = state.tx_statements.len().saturating_sub(HISTORY);
            state.tx_statements.drain(..excess);
            drop(state);
            let index = self.state().executed;
            self.set_tx(tx.follow(index, session.in_transaction()));
            // Their table's open grids told: now, or once the transaction is committed.
            let table =
                vec![(resolved(job.table.clone(), &self.state().snapshot.place, engine), false)];
            if session.in_transaction() {
                self.state().uncommitted.extend(table);
            } else {
                self.announce(&table);
            }
        }
        self.put_back(session);
        let ok = result.is_ok();
        self.submitted(job, result, ok);
    }

    /// A submit ended: said in the Output, kept for its view; applied, its result read again
    /// (its pure query, as it was last sorted and filtered).
    pub(super) fn submitted(
        self: &Arc<Self>,
        job: &SubmitJob,
        result: Result<Applied, DbError>,
        ok: bool,
    ) {
        let text = match &result {
            Ok(applied) => {
                let committed = !self.state().snapshot.tx.is_open();
                let key =
                    if committed { "console.edits_committed" } else { "console.edits_pending" };
                self.log.push(self.line(LogKind::Result, i18n::tr!(key, count = applied.affected)));
                None
            }
            Err(DbError::Conflict(why)) => Some(i18n::tr!("database.conflict", why = why.as_str())),
            Err(e) => Some(i18n::tr!("database.submit_failed", why = message(e))),
        };
        if let Some(text) = text {
            self.log.push(self.line(LogKind::Error, text));
        }
        let again = {
            let mut results = self.results();
            let Some(set) = results.get_mut(job.result) else {
                drop(results);
                self.done();
                return;
            };
            set.submitting = false;
            set.submitted = Some(result);
            set.wholes.clear();
            let again = (ok && set.rerunnable).then(|| set.view.clone());
            if again.is_some() {
                set.loading = true;
            }
            set.touch();
            again
        };
        self.changed();
        match again {
            Some(view) => self.requery(job.result, &view),
            None => self.done(),
        }
    }
}
