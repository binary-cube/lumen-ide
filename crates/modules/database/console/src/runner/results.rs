//! What a console's runner offers on its results: reading, keeping, editing them.

use super::*;

impl Runner {
    /// The results' rows held (bytes), and how many results.
    pub fn result_memory(&self) -> (usize, usize) {
        let results = self.inner.results();
        (results.bytes(), results.sets.len())
    }

    /// Changes when a result comes or goes, is pinned, or its rows change.
    pub fn results_generation(&self) -> u64 {
        let results = self.inner.results();
        results.generation
            + results.sets.iter().map(|s| s.generation).sum::<u64>()
            + results.plans.iter().map(|p| p.generation).sum::<u64>()
    }

    /// The results' tabs, oldest first, then the plans'.
    pub fn result_tabs(&self) -> Vec<ResultTab> {
        let results = self.inner.results();
        let rows = results.sets.iter().map(|s| ResultTab {
            id: s.id,
            title: s.title.clone(),
            pinned: s.pinned,
            loading: s.loading,
            plan: false,
        });
        let plans = results.plans.iter().map(|p| ResultTab {
            id: p.id,
            title: p.title.clone(),
            pinned: p.pinned,
            loading: false,
            plan: true,
        });
        rows.chain(plans).collect()
    }

    /// Plan `id` as its tab shows it (its nodes shared).
    pub fn plan(&self, id: ServiceId) -> Option<crate::results::PlanSet> {
        self.inner.results().plan(id).cloned()
    }

    /// Result `id` as its view reads it.
    pub fn result(&self, id: ServiceId) -> Option<ResultView> {
        let results = self.inner.results();
        let s = results.get(id)?;
        let (chunks, starts, len) = s.rows.chunks();
        Some(ResultView {
            id,
            title: s.title.clone(),
            statement: s.statement.clone(),
            columns: s.columns.clone(),
            chunks,
            starts,
            len,
            more: s.more.clone(),
            rerunnable: s.rerunnable,
            view: s.view.clone(),
            execution: s.execution,
            fetching: s.fetching,
            pinned: s.pinned,
            dropped: s.dropped,
            loading: s.loading,
            error: s.error.clone(),
            cut: s.rows.cut(),
            cut_at: s.rows.cut_at().to_vec(),
            epoch: s.rows.epoch(),
            refused: s.shape.as_ref().err().cloned(),
            editing: s.editing.clone(),
            submitting: s.submitting,
            wholes: s.wholes.clone(),
            generation: s.generation,
        })
    }

    /// Result (or plan) `id`'s generation (`None`: it's gone).
    pub fn result_generation(&self, id: ServiceId) -> Option<u64> {
        let results = self.inner.results();
        results.get(id).map(|s| s.generation).or_else(|| results.plan(id).map(|p| p.generation))
    }

    /// Pins / unpins result `id` (a pinned one stays when the next run comes).
    pub fn pin(&self, id: ServiceId, pinned: bool) {
        {
            let mut results = self.inner.results();
            if let Some(plan) = results.plan_mut(id) {
                plan.pinned = pinned;
                plan.generation += 1;
            } else {
                let Some(set) = results.get_mut(id) else { return };
                set.pinned = pinned;
                set.touch();
            }
            results.generation += 1;
        }
        self.inner.changed();
    }

    /// Result `id`'s tab closed: its rows go at once (a cursor it held closed when the runner
    /// is free; the next statement closes it otherwise).
    pub fn close_result(&self, id: ServiceId) {
        let cursor = {
            let mut results = self.inner.results();
            if let Some(at) = results.plans.iter().position(|p| p.id == id) {
                results.plans.remove(at);
                results.generation += 1;
                drop(results);
                self.inner.changed();
                return;
            }
            let Some(at) = results.sets.iter().position(|s| s.id == id) else { return };
            let set = results.sets.remove(at);
            results.generation += 1;
            match set.more {
                More::Cursor(token) => Some(token),
                _ => None,
            }
        };
        if let Some(token) = cursor
            && self.start()
        {
            let inner = Arc::clone(&self.inner);
            self.inner.runtime.spawn(move || inner.close_cursor(&token));
        }
        self.inner.changed();
    }

    /// The Services window shows the console's Output next.
    pub fn show_output(&self) {
        self.inner.state().reveal = Some(None);
        self.inner.changed();
    }

    /// The node and tab the Services window should show now (a run ended), taken once:
    /// `Some(None)` is the Output.
    pub fn take_reveal(&self) -> Option<Option<ServiceId>> {
        self.inner.state().reveal.take()
    }

    /// Reads result `id`'s next page in the background (from its cursor, or running its
    /// pure query again from the next row; a write's rest is never read). Asked while the
    /// runner works, it's done once it's free. `false`: nothing to read.
    pub fn fetch_more(&self, id: ServiceId) -> bool {
        {
            let mut results = self.inner.results();
            let Some(set) = results.get_mut(id) else { return false };
            if !set.more.readable() || set.loading {
                return false;
            }
            set.loading = true;
            set.touch();
        }
        self.queue(Queued::Fetch(id))
    }

    /// Reads result `id` again from its start, filtered and ordered by the engine as `view`
    /// says (a pure query only; its rows let go come back too). `false`: it can't be.
    pub fn requery(&self, id: ServiceId, view: ServerView) -> bool {
        {
            let mut results = self.inner.results();
            let Some(set) = results.get_mut(id) else { return false };
            if !set.rerunnable {
                return false;
            }
            set.loading = true;
            set.error = None;
            set.touch();
        }
        self.queue(Queued::Requery(id, view))
    }

    /// Exports a result whose rest isn't here (a pure query run again, its rows written as
    /// they come) in the background. `None`: the runner works now, or it can't be.
    pub fn export(&self, request: ExportRequest) -> Option<Arc<ExportState>> {
        let rerunnable = self.inner.results().get(request.result).is_some_and(|s| s.rerunnable);
        if !rerunnable || !self.start() {
            return None;
        }
        let export = Arc::new(ExportState::default());
        let (inner, state) = (Arc::clone(&self.inner), Arc::clone(&export));
        self.inner.changed();
        self.inner.runtime.spawn(move || inner.export(&request, &state));
        Some(export)
    }

    /// Whether result `id`'s rows can be edited, found out in the background (its table's
    /// structure read on the console's connection), once: `false` when it was asked already,
    /// or its query says they can't be.
    pub fn describe(&self, id: ServiceId) -> bool {
        {
            let mut results = self.inner.results();
            let Some(set) = results.get_mut(id) else { return false };
            if set.editing != Editability::Unknown || set.shape.is_err() {
                return false;
            }
            set.editing = Editability::Reading;
            set.touch();
        }
        self.queue(Queued::Describe(id))
    }

    /// The whole value of a big cell of result `id` (kept cut: the row with `key`, result
    /// column `column`) read in the background for its editor. `false`: it can't be.
    pub fn read_whole(&self, id: ServiceId, key: RowKey, column: usize) -> bool {
        let known = {
            let results = self.inner.results();
            let Some(set) = results.get(id) else { return false };
            if !matches!(set.editing, Editability::Ready(_)) {
                return false;
            }
            set.wholes.iter().any(|(k, c, _)| *k == key && *c == column)
        };
        known || self.queue(Queued::Whole(id, key, column))
    }

    /// Result `id`'s changes submitted in the background, exactly as previewed
    /// (`job.script`): in `Tx: Auto` in a transaction of their own, committed at once; in
    /// `Manual` (or inside a transaction open) in the console's, under a savepoint — a
    /// failing one undoes only them —, waiting for its Commit. Its outcome is kept for its
    /// view ([`Self::take_submitted`]); applied, the result is read again (a pure query: never
    /// a write run again). `false`: the runner works now.
    pub fn submit(&self, job: SubmitJob) -> bool {
        if !self.start() {
            return false;
        }
        {
            let mut results = self.inner.results();
            if let Some(set) = results.get_mut(job.result) {
                set.submitting = true;
                set.submitted = None;
                set.touch();
            }
        }
        self.inner.changed();
        let inner = Arc::clone(&self.inner);
        self.inner.runtime.spawn(move || inner.submit(&job));
        true
    }

    /// How the last submit of result `id`'s changes ended, taken once.
    pub fn take_submitted(&self, id: ServiceId) -> Option<Result<Applied, DbError>> {
        let mut results = self.inner.results();
        let set = results.get_mut(id)?;
        let taken = set.submitted.take();
        if taken.is_some() {
            set.touch();
        }
        taken
    }
}
