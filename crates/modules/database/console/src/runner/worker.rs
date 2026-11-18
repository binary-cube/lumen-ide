//! A console's runs on its pool: its connection, the statements, what they leave.

use super::*;

impl Inner {
    pub(super) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn profiles(&self) -> MutexGuard<'_, crate::profile::Profiles> {
        self.profiles.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn results(&self) -> MutexGuard<'_, Results> {
        self.results.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its state changed: the bar draws it again.
    pub(super) fn changed(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.wake();
    }

    /// The transaction's state from now: a transaction opened or ended is told on the
    /// project's bus (`TransactionChanged`).
    pub(super) fn set_tx(&self, tx: TxState) {
        let told = {
            let mut state = self.state();
            let was = state.snapshot.tx.is_open();
            state.snapshot.tx = tx;
            state.snapshot.in_transaction = tx.is_open();
            if tx.is_open() && !was {
                state.snapshot.tx_since = Some(self.runtime.now());
                state.long_told = false;
            } else if !tx.is_open() {
                state.snapshot.tx_since = None;
                state.tx_statements.clear();
            }
            (was != tx.is_open()).then(|| state.context.events.clone()).flatten()
        };
        if let Some(events) = told {
            events.publish(&database::events::TransactionChanged {
                console: self.console,
                source: self.source,
                open: tx.is_open(),
            });
        }
    }

    pub(super) fn wake(&self) {
        let wakes: Vec<Waker> = self
            .wakes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, w)| Arc::clone(w))
            .collect();
        for wake in wakes {
            wake();
        }
    }

    pub(super) fn line(&self, kind: LogKind, text: impl Into<String>) -> LogLine {
        LogLine::new(self.runtime.clock(), kind, text)
    }

    /// The connection, made if it isn't (said in the Output).
    pub(super) fn session(
        &self,
        connector: &Connector,
        source_name: &str,
    ) -> Result<Box<dyn Session>, DbError> {
        if let Some(session) = self.session.lock().unwrap_or_else(PoisonError::into_inner).take() {
            return Ok(session);
        }
        let mut session = connector()?;
        let limited = session.console().is_some_and(|c| c.limited);
        let place = session.current().unwrap_or_default();
        {
            let mut state = self.state();
            state.snapshot.connected = true;
            state.snapshot.place = place;
            state.snapshot.limited = limited;
            state.schema = None;
            state.dialect = session.dialect();
        }
        self.log
            .push(self.line(LogKind::Info, i18n::tr!("console.connected", source = source_name)));
        if limited {
            self.log.push(self.line(LogKind::Warning, i18n::t("console.limited_detail")));
        }
        Ok(session)
    }

    /// The connection again for a result's job (made with what the last run had).
    pub(super) fn reuse(&self) -> Result<Box<dyn Session>, DbError> {
        let (connector, name) = {
            let state = self.state();
            (state.connector.clone(), state.source_name.clone())
        };
        let Some(connector) = connector else {
            return Err(DbError::Unsupported(i18n::t("console.not_connected")));
        };
        self.session(&connector, &name)
    }

    /// The connection given back after a job (closed instead if the console went meanwhile).
    pub(super) fn put_back(&self, mut session: Box<dyn Session>) {
        let place = session.current().unwrap_or_default();
        let in_transaction = session.in_transaction();
        let (closed, tx) = {
            let mut state = self.state();
            state.snapshot.place = place;
            state.last_used = self.runtime.now();
            // The engine has the last word on whether one is open.
            (state.closed, state.snapshot.tx.follow(state.executed, in_transaction))
        };
        self.set_tx(tx);
        if closed {
            drop(session);
            self.state().snapshot.connected = false;
            self.set_tx(TxState::Idle);
            self.results().cursors_closed();
            return;
        }
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = Some(session);
    }

    /// The job ended: not busy, nothing running; what waited for it started; an open
    /// connection watched from now (closed once idle).
    pub(super) fn done(self: &Arc<Self>) {
        // The run's operation ends as the run did (its rows, if it brought some).
        let ended = {
            let mut state = self.state();
            state.activity.take().map(|id| (id, state.snapshot.last.clone()))
        };
        if let Some((id, last)) = ended {
            let outcome = match &last {
                Some(last) if last.cancelled => workbench_model::Outcome::Stopped,
                Some(LastRun { ok: false, error, .. }) => {
                    workbench_model::Outcome::Failed(error.clone().unwrap_or_default())
                }
                _ => workbench_model::Outcome::Done,
            };
            if let Some(rows) = last.and_then(|l| l.rows) {
                QUERIES.advance(id, rows);
            }
            QUERIES.end(id, outcome);
        }
        let (watched, queued, close) = {
            let mut state = self.state();
            state.snapshot.busy = false;
            state.snapshot.running = None;
            // Its bars gone meanwhile: the connection closes now.
            let close = state.closed && self.leases.load(Ordering::Acquire) == 0;
            let queued = state.queued.pop_front();
            if queued.is_some() {
                state.snapshot.busy = true;
            }
            let watched =
                (state.idle.is_some() && state.snapshot.connected) || state.snapshot.tx.is_open();
            let close = close && queued.is_none();
            (watched, queued, close)
        };
        if close {
            self.disconnect();
        }
        if let Some(queued) = queued {
            *self.cancel.lock().unwrap_or_else(PoisonError::into_inner) = Cancel::default();
            let inner = Arc::clone(self);
            self.runtime.spawn(move || inner.run_queued(queued));
        }
        self.changed();
        if watched {
            crate::consoles::watch();
        }
    }

    /// `statement`'s mark set, unless its run isn't the console's text.
    pub(super) fn mark_unless(
        &self,
        request: &RunRequest,
        statement: &PlannedStatement,
        state: MarkState,
    ) {
        if !request.detached {
            self.mark(statement, state);
        }
    }

    /// `statement`'s run recorded in its source's history: as written (its parameters' names,
    /// never their values; its secrets masked), where it ran, how long, its rows, how it ended.
    pub(super) fn remember(
        &self,
        statement: &PlannedStatement,
        result: &Result<database::RunOutcome, DbError>,
        brought: bool,
        took: Duration,
        cancel: &Cancel,
        flavor: sql::Flavor,
    ) {
        use database::history::{HistoryEntry, Outcome};
        let (history, place) = {
            let state = self.state();
            (state.context.history.clone(), state.snapshot.place.clone())
        };
        let Some(history) = history else { return };
        let styles = crate::settings::param_styles(flavor);
        let mut entry = HistoryEntry::new(
            self.source,
            &statement.text,
            flavor,
            &styles,
            self.runtime.wall_ms(),
        );
        entry.console = Some(self.console);
        entry.database = place.database;
        entry.schema = place.schema;
        entry.duration_ms = Some(took.as_millis() as u64);
        let (outcome, rows) = match result {
            Ok(o) if o.stopped && cancel.is_cancelled() => (Outcome::Cancelled, None),
            Ok(o) => (Outcome::Ok, if brought { Some(o.rows) } else { o.affected }),
            Err(DbError::Cancelled) => (Outcome::Cancelled, None),
            Err(_) => (Outcome::Failed, None),
        };
        entry.outcome = outcome;
        entry.rows = rows;
        history.record(entry);
    }

    pub(super) fn mark(&self, statement: &PlannedStatement, state: MarkState) {
        let mut s = self.state();
        let mark = StatementMark {
            range: statement.range.clone(),
            hash: text_hash(&statement.text),
            state,
        };
        match s.snapshot.marks.iter_mut().find(|m| m.range == mark.range) {
            Some(found) => *found = mark,
            None => s.snapshot.marks.push(mark),
        }
        s.snapshot.marks_generation += 1;
    }

    /// The title of a result of `statement`: `database.schema.table` for a query of one
    /// table (the schema the connection is in when it doesn't name one), else none.
    pub(super) fn title_of(&self, statement: &str, flavor: sql::Flavor) -> Option<String> {
        let tokens = sql::lex(statement, flavor);
        let table = sql::query_table(statement, &tokens, flavor)?;
        let place = self.state().snapshot.place.clone();
        let schema = table.schema.or(place.schema);
        let parts: Vec<String> =
            [place.database, schema, Some(table.name)].into_iter().flatten().collect();
        Some(parts.join("."))
    }

    pub(super) fn run(self: &Arc<Self>, request: &RunRequest) {
        let total = request.statements.len();
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let started = self.runtime.now();
        let flavor = request.settings.flavor;
        let mut last = LastRun {
            ok: true,
            done: 0,
            total,
            rows: None,
            truncated: false,
            affected: None,
            elapsed: Duration::ZERO,
            error: None,
            cancelled: false,
            plan_steps: None,
        };
        let mut session = match self.session(&request.connector, &request.source_name) {
            Ok(session) => session,
            Err(e) => {
                let why = message(&e);
                self.log.push(self.line(LogKind::Error, why.clone()));
                if let Some(first) = request.statements.first()
                    && !request.detached
                {
                    self.mark(first, MarkState::Failed);
                }
                last.ok = false;
                last.error = Some(why);
                let mut state = self.state();
                state.snapshot.last = Some(last);
                state.reveal = Some(None);
                drop(state);
                self.done();
                return;
            }
        };
        let cursors = session.console().is_some_and(|c| c.cursors);
        // Only the last query of the run keeps its cursor: the next statement closes it.
        let keeps = request.statements.iter().rposition(|s| s.kind.is_query());
        // The schema chosen, set once (and again when another is).
        self.follow_schema(session.as_mut(), request.settings.schema.as_deref());
        let mut ddl = 0;
        let mut shown: Option<ServiceId> = None;
        for (i, statement) in request.statements.iter().enumerate() {
            if cancel.is_cancelled() {
                last.cancelled = true;
                break;
            }
            {
                let mut state = self.state();
                if let Some(running) = &mut state.snapshot.running {
                    running.index = i;
                }
            }
            if !request.detached {
                self.mark(statement, MarkState::Running);
            }
            self.changed();
            let prefix = prefix(&self.state().snapshot.place);
            // What's sent: its parameters' values written in (shown as sent, passwords hidden).
            let sent = statement.sent();
            let masked = sql::mask_secrets(sent, flavor).into_owned();
            self.log.push(
                self.line(LogKind::Statement, masked.clone()).with_prefix(prefix).as_code("sql"),
            );
            // Manual: the first statement that writes opens the transaction.
            if request.settings.tx == TxMode::Manual
                && statement.writes()
                && !matches!(statement.kind, StatementKind::Transaction(_))
                && !session.in_transaction()
            {
                let options = TxOptions { isolation: request.settings.isolation, read_only: false };
                if let Err(e) = session.begin(&options) {
                    let why = message(&e);
                    self.log.push(self.line(LogKind::Error, why.clone()));
                    self.mark_unless(request, statement, MarkState::Failed);
                    last.ok = false;
                    last.error = Some(why);
                    break;
                }
            }
            // Whatever a cursor kept is read by running its query again from now.
            self.results().cursors_closed();
            let at = self.runtime.now();
            let tokens = sql::lex(sent, flavor);
            let mut sink =
                Collecting::new(self, Destination::New, &masked, statement.range.clone());
            sink.title = self.title_of(sent, flavor);
            sink.rerunnable = sql::rerunnable(sent, &tokens, flavor);
            sink.shape = Some(sql::query_shape(sent, &tokens, flavor));
            let options = RunOptions {
                fetch_limit: request.settings.fetch_limit,
                keep_cursor: cursors && keeps == Some(i),
            };
            let result = session.run(sent, &options, &mut sink, &cancel);
            // The transaction followed: what ran, then the engine's word.
            {
                let ok = result.is_ok();
                let (tx, index) = {
                    let mut state = self.state();
                    let index = state.executed;
                    state.executed += 1;
                    let tx = state.snapshot.tx.on_statement(
                        index,
                        statement.kind,
                        ok,
                        request.settings.tx,
                        session.dialect().map_or(flavor, |d| d.flavor()),
                    );
                    (tx, index)
                };
                let tx = tx.follow(index, session.in_transaction());
                self.set_tx(tx);
                if tx.is_open() && statement.writes() && ok {
                    let mut state = self.state();
                    state.tx_statements.push(masked.clone());
                    let excess = state.tx_statements.len().saturating_sub(HISTORY);
                    state.tx_statements.drain(..excess);
                }
            }
            let end = self.runtime.now();
            let total_time = end.saturating_duration_since(at);
            let execution = sink.first.map_or(total_time, |f| f.saturating_duration_since(at));
            let fetching = total_time.saturating_sub(execution);
            sink.flush();
            let (sets, rerunnable) = (std::mem::take(&mut sink.sets), sink.rerunnable);
            drop(sink);
            self.remember(statement, &result, !sets.is_empty(), total_time, &cancel, flavor);
            match result {
                Ok(outcome) => {
                    for notice in &outcome.notices {
                        let text = match &notice.detail {
                            Some(detail) => {
                                format!("{}: {} ({detail})", notice.severity, notice.message)
                            }
                            None => format!("{}: {}", notice.severity, notice.message),
                        };
                        self.log.push(self.line(LogKind::Notice, text));
                    }
                    let cancelled = outcome.stopped && cancel.is_cancelled();
                    self.finish_sets(&sets, execution, fetching, |last_set| {
                        if !last_set {
                            More::None
                        } else if let Some(token) = outcome.more.clone() {
                            More::Cursor(token)
                        } else if outcome.truncated {
                            if rerunnable { More::Rerun } else { More::Lost }
                        } else {
                            More::None
                        }
                    });
                    if let Some(id) = sets.last() {
                        shown = Some(*id);
                    }
                    self.mark_unless(
                        request,
                        statement,
                        if cancelled { MarkState::Failed } else { MarkState::Done },
                    );
                    last.done = i + 1;
                    last.affected = outcome.affected;
                    last.truncated = outcome.truncated;
                    last.rows = (!sets.is_empty()).then_some(outcome.rows);
                    if statement.kind.touches_schema() {
                        ddl += 1;
                        // What it made (a temporary table the schema's own connection won't
                        // see): never an unknown table in this console.
                        let made = sql::created_tables(
                            &statement.text,
                            &sql::lex(&statement.text, flavor),
                            flavor,
                        );
                        if !made.is_empty() {
                            let mut state = self.state();
                            for t in made {
                                if !state.created.contains(&t) {
                                    state.created.push(t);
                                }
                            }
                        }
                    }
                    if statement
                        .text
                        .trim_start()
                        .get(..6)
                        .is_some_and(|w| w.eq_ignore_ascii_case("ATTACH"))
                    {
                        self.state().attached = true;
                    }
                    let text = if cancelled {
                        last.cancelled = true;
                        i18n::t("console.cancelled")
                    } else if !sets.is_empty() {
                        i18n::tr!(
                            "console.rows_retrieved",
                            count = outcome.rows,
                            from = 1,
                            total = millis(total_time),
                            exec = millis(execution),
                            fetch = millis(fetching)
                        )
                    } else if let Some(n) = outcome.affected {
                        i18n::tr!("console.rows_affected", count = n, total = millis(total_time))
                    } else {
                        i18n::tr!("console.completed", total = millis(total_time))
                    };
                    self.log.push(self.line(LogKind::Result, text));
                    if outcome.truncated
                        && sets.last().is_some()
                        && !rerunnable
                        && outcome.more.is_none()
                    {
                        self.log.push(self.line(LogKind::Warning, i18n::t("console.rows_left")));
                    }
                    if statement.writes() && !cancelled {
                        self.changed_tables(sent, &tokens, flavor, session.as_ref());
                    }
                    if cancelled {
                        break;
                    }
                }
                Err(DbError::Cancelled) => {
                    self.finish_sets(&sets, execution, fetching, |_| More::None);
                    self.mark_unless(request, statement, MarkState::Failed);
                    self.log.push(self.line(LogKind::Warning, i18n::t("console.cancelled")));
                    last.cancelled = true;
                    break;
                }
                Err(e) => {
                    self.finish_sets(&sets, execution, fetching, |_| More::None);
                    self.mark_unless(request, statement, MarkState::Failed);
                    let place = error_place(&e, &request.document, statement);
                    let why = error_text(&e, place);
                    let mut line = self.line(LogKind::Error, why.clone());
                    let file = self.state().context.file.clone().filter(|_| !request.detached);
                    if let Some(file) = file {
                        let at = place.unwrap_or_else(|| {
                            TextPosition::of(&request.document, statement.range.start)
                        });
                        line = line.with_link(ViewerAction::OpenFile {
                            path: file,
                            line: Some(at.line as u32 + 1),
                            column: Some(at.column as u32 + 1),
                        });
                    }
                    self.log.push(line);
                    if !request.detached {
                        self.diagnose(&e, place, &request.document, statement);
                    }
                    last.ok = false;
                    last.error = Some(why);
                    last.done = i;
                    if request.settings.stop_on_error || total == 1 {
                        break;
                    }
                }
            }
        }
        last.elapsed = self.runtime.now().saturating_duration_since(started);
        let elapsed = last.elapsed;
        let ok = last.ok;
        {
            let mut state = self.state();
            state.snapshot.last = Some(last);
            // The Services window shows the last result (or the Output: an error, nothing
            // brought).
            state.reveal = Some(if ok { shown } else { None });
        }
        self.put_back(session);
        self.ran(ddl, elapsed, ok);
        self.done();
    }

    /// The sets a statement brought, ended: sealed, their times, the rest of the last one.
    pub(super) fn finish_sets(
        &self,
        sets: &[ServiceId],
        execution: Duration,
        fetching: Duration,
        more: impl Fn(bool) -> More,
    ) {
        let mut results = self.results();
        for (i, id) in sets.iter().enumerate() {
            if let Some(set) = results.get_mut(*id) {
                set.rows.seal();
                set.execution = execution;
                set.fetching = fetching;
                set.more = more(i + 1 == sets.len());
                set.loading = false;
                set.touch();
            }
        }
        results.generation += 1;
        let limit = self.state().settings.result_memory;
        results.trim(limit, sets.last().copied());
    }

    /// The tables `statement` wrote: their open grids told (now, or once the transaction it
    /// ran in is committed).
    pub(super) fn changed_tables(
        &self,
        statement: &str,
        tokens: &[sql::Token],
        flavor: sql::Flavor,
        session: &dyn Session,
    ) {
        let place = self.state().snapshot.place.clone();
        // The engine's own rules (SQLite's `main` is no schema of the grids').
        let engine = session.dialect().map_or(flavor, |d| d.flavor());
        let tables: Vec<(TableRef, bool)> = sql::written_tables(statement, tokens, flavor)
            .into_iter()
            .map(|(table, structure)| (resolved(table, &place, engine), structure))
            .collect();
        if tables.is_empty() {
            return;
        }
        if session.in_transaction() {
            self.state().uncommitted.extend(tables);
        } else {
            self.announce(&tables);
        }
    }

    /// `DataChanged` for each of `tables` on the project's bus.
    pub(super) fn announce(&self, tables: &[(TableRef, bool)]) {
        let Some(events) = self.state().context.events.clone() else { return };
        for (table, structure) in tables {
            events.publish(&database::events::DataChanged {
                source: self.source,
                table: table.clone(),
                structure: *structure,
            });
        }
    }

    /// A run ended: its schema's change said on the project's bus; a long one ended while the Services
    /// window is hidden, told.
    pub(super) fn ran(&self, ddl: usize, elapsed: Duration, ok: bool) {
        let context = self.state().context.clone();
        if let Some(events) = &context.events {
            // Its schema changed: what's known of it (completion's index) is read again.
            if ddl > 0 {
                events.publish(&database::events::SchemaInvalidated { source: self.source });
            }
        }
        let hidden = context.project.is_some_and(|p| !ui_components::services::shown(p));
        if elapsed >= LONG_RUN && hidden {
            let title = if ok {
                i18n::tr!(
                    "console.long_run_done",
                    name = context.name.as_str(),
                    time = millis(elapsed)
                )
            } else {
                i18n::tr!(
                    "console.long_run_failed",
                    name = context.name.as_str(),
                    time = millis(elapsed)
                )
            };
            let notification =
                if ok { Notification::success(title) } else { Notification::error(title) };
            let services = workbench_model::TOGGLE_SERVICES;
            context.notifier.show(if workbench_model::presence::command(services) {
                notification.with_action(i18n::t("console.show_services"), services)
            } else {
                notification
            });
        }
    }

    /// The error underlined in the console's text (its place, else its statement's first
    /// line), through the project's diagnostics.
    pub(super) fn diagnose(
        &self,
        e: &DbError,
        place: Option<TextPosition>,
        document: &str,
        statement: &PlannedStatement,
    ) {
        let (events, relative) = {
            let state = self.state();
            (state.context.events.clone(), state.context.relative.clone())
        };
        let (Some(events), Some(relative)) = (events, relative) else { return };
        let at = place.unwrap_or_else(|| TextPosition::of(document, statement.range.start));
        let (text, code) = match e {
            DbError::Sql(error) => (error.message.clone(), error.sqlstate.clone()),
            other => (message(other), None),
        };
        let diagnostic = workbench_model::Diagnostic {
            path: relative.clone(),
            line: at.line as u32,
            column: place.map(|p| p.column as u32),
            end: None,
            severity: workbench_model::Severity::Error,
            message: kernel::text::one_line(&text).into_owned(),
            source: i18n::t("console.diagnostics_source"),
            rule: code,
        };
        events.publish(&workbench_model::DiagnosticsFound {
            source: DIAGNOSTICS.to_owned(),
            scope: workbench_model::DiagnosticsScope::File(relative),
            diagnostics: vec![diagnostic],
        });
        let mut state = self.state();
        state.diagnosed = true;
        state.snapshot.error_mark = Some((statement.range.clone(), text_hash(&statement.text)));
    }

    /// The error underlined goes (a new run, its statement edited).
    pub(super) fn clear_diagnostics(&self) {
        let (events, relative) = {
            let mut state = self.state();
            state.snapshot.error_mark = None;
            state.snapshot.focus = None;
            if !std::mem::take(&mut state.diagnosed) {
                return;
            }
            (state.context.events.clone(), state.context.relative.clone())
        };
        let (Some(events), Some(relative)) = (events, relative) else { return };
        events.publish(&workbench_model::DiagnosticsFound {
            source: DIAGNOSTICS.to_owned(),
            scope: workbench_model::DiagnosticsScope::File(relative),
            diagnostics: Vec::new(),
        });
    }

    /// The connection set to `schema` when it isn't yet (a failure said, the run going on
    /// in the connection's own).
    pub(super) fn follow_schema(&self, session: &mut dyn Session, schema: Option<&str>) {
        let Some(schema) = schema else { return };
        if self.state().schema.as_deref() == Some(schema) {
            return;
        }
        match session.set_schema(schema) {
            Ok(()) => self.state().schema = Some(schema.to_owned()),
            Err(e) => self.log.push(self.line(LogKind::Warning, message(&e))),
        }
    }

    /// What waited for the runner: a result's next page, or its rows read again.
    pub(super) fn run_queued(self: &Arc<Self>, queued: Queued) {
        match queued {
            Queued::Fetch(id) => self.fetch_more(id),
            Queued::Requery(id, view) => self.requery(id, &view),
            Queued::TableStats(id) => self.read_table_stats(id),
            Queued::Describe(id) => self.describe(id),
            Queued::Whole(id, key, column) => self.read_whole(id, &key, column),
        }
    }

    pub(super) fn list_places(self: &Arc<Self>, connector: &Connector, source_name: &str) {
        let mut session = match self.session(connector, source_name) {
            Ok(session) => session,
            Err(e) => {
                let why = message(&e);
                self.log.push(self.line(LogKind::Error, why.clone()));
                self.state().snapshot.last = Some(failed(why, 0));
                self.done();
                return;
            }
        };
        let databases = session.databases().unwrap_or_default();
        let schemas: Vec<String> = session
            .children(&[])
            .map(|nodes| nodes.into_iter().filter(|n| n.kind == "schema").map(|n| n.name).collect())
            .unwrap_or_default();
        self.state().snapshot.places = Some(Places { databases, schemas });
        self.put_back(session);
        self.done();
    }

    pub(super) fn finish_transaction(self: &Arc<Self>, commit: bool) {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner).take();
        let Some(mut session) = session else {
            self.state().uncommitted.clear();
            self.set_tx(TxState::Idle);
            self.done();
            return;
        };
        self.results().cursors_closed();
        let result = if commit { session.commit() } else { session.rollback() };
        let written = std::mem::take(&mut self.state().uncommitted);
        match result {
            Ok(()) => {
                self.set_tx(TxState::Idle);
                let key = if commit { "console.tx_committed" } else { "console.tx_rolled_back" };
                self.log.push(self.line(LogKind::Info, i18n::t(key)));
                // What it wrote is seen by the others now.
                if commit {
                    self.announce(&written);
                }
            }
            Err(e) => {
                let why = message(&e);
                self.log.push(self.line(LogKind::Error, why.clone()));
                // Said aloud too: the console's tab may be gone already (closed on Commit).
                let context = self.state().context.clone();
                let key = if commit { "console.commit_failed" } else { "console.rollback_failed" };
                context.notifier.show(Notification::error(i18n::tr!(
                    key,
                    name = context.name.as_str(),
                    why = why
                )));
            }
        }
        self.put_back(session);
        self.done();
    }

    /// Closes the connection in the background (what runs stopped first).
    pub(super) fn disconnect(self: &Arc<Self>) {
        self.cancel.lock().unwrap_or_else(PoisonError::into_inner).cancel();
        {
            let mut state = self.state();
            // A job working (or about to: a commit asked as its tab closes) closes it once
            // it's done with it.
            if state.snapshot.busy {
                state.closed = state.closed || self.leases.load(Ordering::Acquire) == 0;
                return;
            }
        }
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner).take();
        {
            let mut state = self.state();
            if session.is_some() {
                state.snapshot.connected = false;
                state.snapshot.places = None;
                state.schema = None;
                state.uncommitted.clear();
            }
        }
        if let Some(session) = session {
            self.set_tx(TxState::Idle);
            self.results().cursors_closed();
            // Closing may wait for the server: never on the app's thread.
            self.runtime.spawn(move || drop(session));
            self.changed();
        }
    }
}
