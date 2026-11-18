//! A console's profiling, on its pool.

use super::*;

impl Inner {
    /// A profile kept (bounded) and shown: its tab (an unpinned plan's replaced before),
    /// its summary line in the Output — the problems counted, a link to the tab, where they
    /// are said (39h4: no longer a line each here). Its tab's id.
    pub(super) fn keep_profile(&self, shown: ProfileShown, plan: Vec<PlanNode>) -> ServiceId {
        let report = Arc::clone(&shown.report);
        {
            let (keep, bytes) = crate::settings::profile_limits();
            let mut stored = crate::profile::StoredProfile::new(
                Arc::clone(&report),
                shown.statement.clone(),
                shown.range.clone(),
            );
            stored.facts.clone_from(&shown.facts);
            stored.fingerprint = shown.fingerprint;
            stored.at = self.runtime.clock();
            stored.elapsed = shown.elapsed;
            stored.stats_asked =
                shown.facts.as_ref().is_some_and(|f| f.tables.iter().any(|t| t.stats.is_some()));
            self.profiles().push(stored, keep, bytes);
        }
        let mut set = PlanSet::new(shown.title, shown.statement, shown.range, plan);
        set.analyze = shown.analyze;
        set.rolled_back = shown.rolled_back;
        set.elapsed = shown.elapsed;
        set.profile = Some(report.profile.id);
        set.document_hash = shown.document_hash;
        set.table = shown.table;
        set.report = Some(Arc::clone(&report));
        set.facts = shown.facts;
        let id = set.id;
        self.log.push(
            self.line(LogKind::Info, profile_summary(&report))
                .with_link(ViewerAction::OpenLink(id.link())),
        );
        let mut results = self.results();
        results.plans.push(set);
        results.generation += 1;
        id
    }

    /// Profile `id`'s tables' statistics read on the console's connection (when the
    /// auxiliary queries are allowed, the driver has them and isn't limited), kept with its
    /// facts — the stored profile's and its tabs' —, its views told.
    pub(super) fn read_table_stats(self: &Arc<Self>, id: ProfileId) {
        let facts = self.profiles().get(id).and_then(|p| p.facts.clone());
        let tables: Vec<TableRef> = facts
            .as_ref()
            .map(|f| f.tables.iter().map(|t| t.table.clone()).collect())
            .unwrap_or_default();
        if tables.is_empty() || !crate::settings::profile_options(false).statement_history {
            self.done();
            return;
        }
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut session = match self.reuse() {
            Ok(session) => session,
            Err(e) => {
                tracing::debug!("a profile's tables' statistics weren't read: {e}");
                self.done();
                return;
            }
        };
        let caps = session.console();
        let allowed = caps.is_some_and(|c| !c.limited && c.profile.table_stats);
        let read = if allowed { session.table_stats(&tables, &cancel) } else { Ok(Vec::new()) };
        self.put_back(session);
        match read {
            Ok(stats) if !stats.is_empty() => {
                let mut merged = facts.as_deref().cloned().unwrap_or_default();
                for t in &mut merged.tables {
                    if let Some(s) = stats
                        .iter()
                        .find(|s| database::profile::rules::same_table(&s.table, &t.table))
                    {
                        t.stats = Some(s.clone());
                    }
                }
                let merged = Arc::new(merged);
                if let Some(p) = self.profiles().get_mut(id) {
                    p.facts = Some(Arc::clone(&merged));
                }
                let mut results = self.results();
                for plan in &mut results.plans {
                    if plan.profile == Some(id) {
                        plan.facts = Some(Arc::clone(&merged));
                        plan.generation += 1;
                    }
                }
            }
            Ok(_) => {}
            Err(e) => tracing::debug!("a profile's tables' statistics weren't read: {e}"),
        }
        self.changed();
        self.done();
    }

    pub(super) fn profile(
        self: &Arc<Self>,
        statement: &PlannedStatement,
        options: &ProfileOptions,
        connector: &Connector,
        source_name: &str,
        flavor: sql::Flavor,
        document: &str,
    ) {
        let cancel = self.cancel.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut session = match self.session(connector, source_name) {
            Ok(session) => session,
            Err(e) => {
                let why = message(&e);
                self.log.push(self.line(LogKind::Error, why.clone()));
                self.state().snapshot.last = Some(failed(why, 1));
                self.done();
                return;
            }
        };
        self.mark(statement, MarkState::Running);
        self.changed();
        let prefix = prefix(&self.state().snapshot.place);
        let analyze = options.depth.runs();
        let words = if analyze { "EXPLAIN ANALYZE" } else { "EXPLAIN" };
        let sent = statement.sent();
        let masked = sql::mask_secrets(sent, flavor).into_owned();
        self.log.push(
            self.line(LogKind::Statement, format!("{words} {masked}"))
                .with_prefix(prefix)
                .as_code("sql"),
        );
        self.results().cursors_closed();
        // Timed from the request (the connection, made or reused, isn't the profile's).
        let asked = self.runtime.now();
        let result = session.profile(sent, options, &cancel);
        let elapsed = self.runtime.now().saturating_duration_since(asked);
        let (last, shown) = match result {
            Ok(mut profile) => {
                self.mark(statement, MarkState::Done);
                // The runner's clock: the whole profile, the network included.
                profile.statement.wall_ms = Some(elapsed.as_secs_f64() * 1000.0);
                // The snapshots': what the engine measured, the same every run.
                #[cfg(any(test, feature = "snapshot"))]
                if self.runtime.is_frozen() {
                    crate::profile::sample::freeze_times(&mut profile);
                }
                profile.sum_io();
                // The analysis (39h3): what it needs read on the same connection, the rules.
                let rules_flavor = session.dialect().map_or(flavor, |d| d.flavor());
                let (analysis, recent) = {
                    let state = self.state();
                    (state.analysis.clone(), state.recent.clone())
                };
                let facts = crate::profile::analysis::gather(
                    &mut *session,
                    &profile,
                    sent,
                    rules_flavor,
                    options.statement_history,
                    analysis.notes.clone(),
                    &cancel,
                );
                let fingerprint = sql::fingerprint(sent, rules_flavor).hash;
                let report = database::profile::derive(profile);
                let findings = crate::profile::analysis::analyze(
                    &report,
                    &facts,
                    statement,
                    rules_flavor,
                    &recent,
                    analysis.production,
                );
                let report = crate::profile::analysis::with_findings(report, findings);
                let plan = report.profile.plan_nodes();
                let mut steps = 0;
                for node in &plan {
                    steps += write_plan(&self.log, &self.runtime, node, 0, flavor);
                }
                let rolled_back = report.profile.has_note(ProfileNote::RolledBack);
                let shown = ProfileShown {
                    title: profile_title(sent, flavor),
                    table: profile_table(sent, flavor),
                    report,
                    statement: masked,
                    range: statement.range.clone(),
                    analyze,
                    rolled_back: rolled_back || (analyze && statement.writes()),
                    elapsed,
                    document_hash: document
                        .get(statement.range.clone())
                        .map(ui_components::editor_toolbar::text_hash),
                    facts: Some(Arc::new(facts)),
                    fingerprint,
                };
                let id = self.keep_profile(shown, plan);
                let last = LastRun {
                    ok: true,
                    done: 1,
                    total: 1,
                    rows: None,
                    truncated: false,
                    affected: None,
                    elapsed,
                    error: None,
                    cancelled: false,
                    plan_steps: Some(steps),
                };
                (last, Some(id))
            }
            Err(DbError::Cancelled) => {
                self.mark(statement, MarkState::Failed);
                self.log.push(self.line(LogKind::Warning, i18n::t("console.cancelled")));
                let mut last = failed(i18n::t("console.cancelled"), 1);
                last.cancelled = true;
                (last, None)
            }
            Err(e) => {
                self.mark(statement, MarkState::Failed);
                // Its place in the user's text (the values written in taken back out).
                let place = error_place(&e, document, statement);
                let why = error_text(&e, place);
                let mut line = self.line(LogKind::Error, why.clone());
                let file = self.state().context.file.clone();
                if let Some(file) = file {
                    let at =
                        place.unwrap_or_else(|| TextPosition::of(document, statement.range.start));
                    line = line.with_link(ViewerAction::OpenFile {
                        path: file,
                        line: Some(at.line as u32 + 1),
                        column: Some(at.column as u32 + 1),
                    });
                }
                self.log.push(line);
                self.diagnose(&e, place, document, statement);
                (failed(why, 1), None)
            }
        };
        let mut state = self.state();
        state.snapshot.last = Some(last);
        state.reveal = Some(shown);
        drop(state);
        self.put_back(session);
        self.done();
    }
}
