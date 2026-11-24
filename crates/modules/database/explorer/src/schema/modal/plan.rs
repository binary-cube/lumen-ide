//! The plan asked of the source's worker as the form changes (only the latest kept), what
//! it would lose, and the run.

use super::*;

impl SchemaView {
    /// Asks the worker what the form's changes would run, when they changed (the plan of
    /// changes no longer shown isn't waited for); and the structure of each table a key
    /// points to, once.
    pub fn replan(&mut self) {
        let Some(client) = &self.client else { return };
        if let Some(form) = &self.form {
            for row in &form.keys {
                if let Some(target) = &row.target
                    && !self.structures.contains_key(target)
                    && !self.structure_tickets.values().any(|t| t == target)
                    && form.ctx.tables.iter().any(|i| i.table == *target)
                {
                    let ticket = client.send(Request::Structure(target.clone()));
                    self.structure_tickets.insert(ticket, target.clone());
                }
            }
        }
        let Ok(changes) = self.changes() else {
            if let Some((_, ticket)) = self.asked.take() {
                client.cancel(ticket);
            }
            return;
        };
        let same = |c: &Vec<SchemaChange>| *c == changes;
        if self.asked.as_ref().is_some_and(|(c, _)| same(c))
            || self.planned.as_ref().is_some_and(|(c, _)| same(c))
        {
            return;
        }
        if let Some((_, ticket)) = self.asked.take() {
            client.cancel(ticket);
        }
        let ticket = client.send(Request::PlanAlter(changes.clone()));
        self.asked = Some((changes, ticket));
        self.confirmed = false;
    }

    /// What's lost, to confirm: the data dropped and what a rebuild doesn't make again.
    pub fn dangers(&self) -> Vec<Danger> {
        let (Ok(changes), Some(form)) = (self.changes(), &self.form) else { return Vec::new() };
        let ctx = &form.ctx;
        let mut dangers = schema_dangers(&changes, &|t| ctx.rows_of(t), &|t| {
            ctx.structure.as_ref().filter(|s| s.table == *t).cloned()
        });
        if let Some(Ok(Some(preview))) = self.plan() {
            dangers.extend(script_dangers(&preview.script));
        }
        dangers
    }

    /// Everything is there to submit: the form complete, its plan come back fine, what's
    /// lost confirmed, nothing running.
    pub fn can_submit(&self) -> bool {
        self.running.is_none()
            && matches!(self.plan(), Some(Ok(_)))
            && (self.confirmed || self.dangers().is_empty())
    }

    /// Runs exactly what was shown (when it can).
    pub fn submit(&mut self) -> bool {
        if !self.can_submit() {
            return false;
        }
        let (Ok(changes), Some(Ok(plan)), Some(client)) =
            (self.changes(), self.plan().cloned(), &self.client)
        else {
            return false;
        };
        let shown = plan.map(|p| p.script);
        self.running = Some(client.send(Request::AlterAsShown { changes, shown }));
        self.failed = None;
        self.popup = None;
        true
    }

    pub fn cancel(&mut self) {
        self.action = Some(ViewerAction::Close);
    }

    /// The worker's replies: plans, structures, the run's outcome.
    pub(super) fn take_replies(&mut self) -> bool {
        let Some(client) = &self.client else { return false };
        let replies = client.poll();
        let mut changed = !replies.is_empty();
        for reply in replies {
            if let Some(table) = self.structure_tickets.remove(&reply.ticket) {
                if let Ok(Response::Structure(s)) = reply.result {
                    self.structures.insert(table, s);
                    self.fill_references();
                }
                continue;
            }
            if self.asked.as_ref().is_some_and(|(_, t)| *t == reply.ticket) {
                let Some((changes, _)) = self.asked.take() else { continue };
                match reply.result {
                    Err(DbError::Cancelled) => {}
                    Ok(Response::Plan(plan)) => self.planned = Some((changes, Ok(plan))),
                    Ok(_) => {}
                    Err(e) => self.planned = Some((changes, Err(e))),
                }
                continue;
            }
            if self.running == Some(reply.ticket) {
                self.running = None;
                match reply.result {
                    Ok(Response::Applied(applied)) => {
                        let changes = self.changes().unwrap_or_default();
                        if let Some(request) = &self.request {
                            request.done(Done { changes, applied });
                        }
                        self.action = Some(ViewerAction::Close);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        self.failed = Some(tr!("schema.failed", why = message(&e)));
                        // Planned again: the structure may have changed.
                        self.planned = None;
                        changed = true;
                    }
                }
            }
        }
        self.replan();
        changed
    }

    /// A key's columns pointed to, when none were chosen: its table's primary key.
    pub(super) fn fill_references(&mut self) {
        let Some(form) = &mut self.form else { return };
        for row in &mut form.keys {
            if !row.references.is_empty() {
                continue;
            }
            if let Some(s) = row.target.as_ref().and_then(|t| self.structures.get(t)) {
                row.references = s.primary_key();
            }
        }
    }
}
