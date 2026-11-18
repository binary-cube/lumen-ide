//! A table read and changed through its source's worker: [`database::TableRepository`]
//! for every driver (an SQL engine, a CSV file, a later one), never blocking the view.
//! Pages are cached within a memory limit ([`crate::cache`]), the next one read ahead in
//! the background once a full page came; a commit — here, or in another view of the same
//! table (`DataChanged` on the project's bus) — lets everything go to be read again.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use database::transaction::{apply_script, check_editable};
use database::{
    ChangeSet, DbError, Fetch, Filter, Order, Page, RepositoryEvent, Rows, Structure, TableRef,
    TableRepository,
};
use kernel::{EventBus, Purge, Subscription};

use crate::cache::{PageCache, PageKey};
use crate::worker::{Client, Request, Response, SessionWorker, Source, Ticket};

/// What a request waiting for its reply was for.
#[derive(Debug)]
enum Asked {
    Structure,
    Count(Option<Filter>),
    Page { key: PageKey, page: Page, ahead: bool },
    Apply,
}

/// A table of a source, through its worker (see the module's doc).
pub struct SessionRepository {
    client: Client,
    source: Source,
    table: TableRef,
    structure: Option<Fetch<Arc<Structure>>>,
    count: Option<(Option<Filter>, Fetch<u64>)>,
    /// The order and the filter the cached pages are of.
    view: Option<(Vec<Order>, Option<Filter>)>,
    pages: PageCache,
    /// The pages that failed (asked again after a refresh).
    failed: Vec<(PageKey, DbError)>,
    pending: HashMap<Ticket, Asked>,
    events: Vec<RepositoryEvent>,
    /// A commit changed the table (said on the project's bus): read again at the next poll.
    stale: Arc<AtomicBool>,
    _changed: Option<Subscription>,
}

impl std::fmt::Debug for SessionRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionRepository").field("table", &self.table).finish_non_exhaustive()
    }
}

impl SessionRepository {
    /// `table` of `worker`'s source; `wake` called when something comes; a commit said on
    /// `events` (the project's bus) refreshes it. Pages are kept up to `limit` bytes.
    pub fn new(
        worker: &SessionWorker,
        table: TableRef,
        wake: Arc<dyn Fn() + Send + Sync>,
        events: Option<&EventBus>,
        limit: usize,
    ) -> Self {
        let stale = Arc::new(AtomicBool::new(false));
        let changed = events.map(|bus| {
            let (stale, wake, source, table) =
                (Arc::clone(&stale), Arc::clone(&wake), worker.source().id, table.clone());
            bus.subscribe(move |e: &database::events::DataChanged| {
                if e.source == source && e.table == table {
                    stale.store(true, Ordering::Release);
                    wake();
                }
            })
        });
        Self {
            client: worker.client(wake),
            source: worker.source().clone(),
            table,
            structure: None,
            count: None,
            view: None,
            pages: PageCache::new(limit),
            failed: Vec::new(),
            pending: HashMap::new(),
            events: Vec::new(),
            stale,
            _changed: changed,
        }
    }

    pub fn table(&self) -> &TableRef {
        &self.table
    }

    /// The bytes its cached pages hold.
    pub fn memory(&self) -> usize {
        self.pages.bytes()
    }

    /// Requests sent and not answered yet.
    pub fn waiting(&self) -> usize {
        self.pending.len()
    }

    fn ask(&mut self, request: Request, asked: Asked) {
        let ticket = self.client.send(request);
        self.pending.insert(ticket, asked);
    }

    /// The tickets of the pages asked (read ahead or not).
    fn page_tickets(&self, ahead: Option<bool>) -> Vec<Ticket> {
        self.pending
            .iter()
            .filter(
                |(_, a)| matches!(a, Asked::Page { ahead: x, .. } if ahead.is_none_or(|w| w == *x)),
            )
            .map(|(t, _)| *t)
            .collect()
    }

    fn drop_tickets(&mut self, tickets: Vec<Ticket>) {
        for ticket in tickets {
            self.pending.remove(&ticket);
            self.client.cancel(ticket);
        }
    }

    fn page_pending(&self, key: PageKey) -> Option<(Ticket, bool)> {
        self.pending.iter().find_map(|(t, a)| match a {
            Asked::Page { key: k, ahead, .. } if *k == key => Some((*t, *ahead)),
            _ => None,
        })
    }

    /// The page after `page` read ahead, if it isn't here or asked.
    fn read_ahead(&mut self, page: &Page) {
        let next = Page { offset: page.offset + u64::from(page.limit), ..page.clone() };
        let key = PageKey { offset: next.offset, limit: next.limit };
        if self.pages.get(key).is_some() || self.page_pending(key).is_some() {
            return;
        }
        // One page ahead at a time: the worker keeps only the latest anyway.
        let older = self.page_tickets(Some(true));
        self.drop_tickets(older);
        let request =
            Request::ReadPage { table: self.table.clone(), page: next.clone(), ahead: true };
        self.ask(request, Asked::Page { key, page: next, ahead: true });
    }

    fn received(&mut self, asked: Asked, result: Result<Response, DbError>) -> bool {
        let mut refresh = false;
        match (asked, result) {
            // Replaced by a newer request, or dropped: nothing to say.
            (Asked::Structure, Err(DbError::Cancelled)) => self.structure = None,
            (Asked::Count(_), Err(DbError::Cancelled)) => self.count = None,
            (Asked::Page { .. }, Err(DbError::Cancelled)) => {}
            (Asked::Structure, result) => {
                self.structure = Some(match result {
                    Ok(Response::Structure(s)) => Fetch::Ready(Arc::new(s)),
                    Ok(other) => Fetch::Failed(unexpected(&other)),
                    Err(e) => Fetch::Failed(e),
                });
                self.events.push(RepositoryEvent::Structure);
            }
            (Asked::Count(filter), result) => {
                let fetch = match result {
                    Ok(Response::Count(n)) => Fetch::Ready(n),
                    Ok(other) => Fetch::Failed(unexpected(&other)),
                    Err(e) => Fetch::Failed(e),
                };
                if self.count.as_ref().is_some_and(|(f, _)| *f == filter) {
                    self.count = Some((filter, fetch));
                    self.events.push(RepositoryEvent::Count);
                }
            }
            (Asked::Page { key, page, ahead }, result) => {
                match result {
                    Ok(Response::Page(rows)) => {
                        let full = rows.len() >= page.limit as usize;
                        self.pages.insert(key, Arc::new(rows));
                        if full && !ahead {
                            self.read_ahead(&page);
                        }
                    }
                    Ok(other) => self.failed.push((key, unexpected(&other))),
                    Err(e) => self.failed.push((key, e)),
                }
                if !ahead {
                    self.events.push(RepositoryEvent::Page(page));
                }
            }
            (Asked::Apply, result) => {
                let result = result.and_then(|r| match r {
                    Response::Applied(applied) => Ok(applied),
                    other => Err(unexpected(&other)),
                });
                refresh = result.is_ok();
                self.events.push(RepositoryEvent::Applied(result));
            }
        }
        refresh
    }
}

fn unexpected(response: &Response) -> DbError {
    DbError::Query(format!("an unexpected reply: {response:?}"))
}

impl TableRepository for SessionRepository {
    fn count(&mut self, filter: Option<&Filter>) -> Fetch<u64> {
        if let Some((f, fetch)) = &self.count
            && f.as_ref() == filter
        {
            return fetch.clone();
        }
        // Another filter: the previous total isn't wanted any more.
        let older: Vec<Ticket> = self
            .pending
            .iter()
            .filter(|(_, a)| matches!(a, Asked::Count(_)))
            .map(|(t, _)| *t)
            .collect();
        self.drop_tickets(older);
        let filter = filter.cloned();
        self.count = Some((filter.clone(), Fetch::Pending));
        self.ask(
            Request::Count { table: self.table.clone(), filter: filter.clone() },
            Asked::Count(filter),
        );
        Fetch::Pending
    }

    fn page(&mut self, page: &Page) -> Fetch<Arc<Rows>> {
        let view = (page.order.clone(), page.filter.clone());
        if self.view.as_ref() != Some(&view) {
            // Another order or filter: the pages kept are of no use.
            self.pages.purge();
            self.failed.clear();
            let older = self.page_tickets(None);
            self.drop_tickets(older);
            self.view = Some(view);
        }
        let key = PageKey { offset: page.offset, limit: page.limit };
        if let Some(rows) = self.pages.get(key) {
            return Fetch::Ready(rows);
        }
        if let Some((_, e)) = self.failed.iter().find(|(k, _)| *k == key) {
            return Fetch::Failed(e.clone());
        }
        match self.page_pending(key) {
            Some((_, false)) => return Fetch::Pending,
            // Read ahead, and wanted now: asked again, in its turn (not after the rest).
            Some((ticket, true)) => self.drop_tickets(vec![ticket]),
            None => {}
        }
        let request =
            Request::ReadPage { table: self.table.clone(), page: page.clone(), ahead: false };
        self.ask(request, Asked::Page { key, page: page.clone(), ahead: false });
        Fetch::Pending
    }

    fn structure(&mut self) -> Fetch<Arc<Structure>> {
        if let Some(fetch) = &self.structure {
            return fetch.clone();
        }
        self.structure = Some(Fetch::Pending);
        self.ask(Request::Structure(self.table.clone()), Asked::Structure);
        Fetch::Pending
    }

    fn preview(&self, changes: &ChangeSet) -> Result<Vec<String>, DbError> {
        match &self.source.dialect {
            Some(dialect) => {
                // As it runs: with the structure (read before anything is edited).
                let structure = match &self.structure {
                    Some(Fetch::Ready(structure)) => Some(structure.as_ref()),
                    _ => None,
                };
                Ok(apply_script(dialect.as_ref(), changes, structure)?.statements())
            }
            None => Ok(Vec::new()),
        }
    }

    fn apply(&mut self, changes: ChangeSet) {
        if let Some(Fetch::Ready(structure)) = &self.structure
            && let Err(e) =
                check_editable(structure, &self.source.capabilities, self.source.read_only)
        {
            self.events.push(RepositoryEvent::Applied(Err(e)));
            return;
        }
        self.ask(Request::Apply(changes), Asked::Apply);
    }

    fn refresh(&mut self) {
        // What was asked is of no use now — but a commit's outcome is still awaited.
        let older: Vec<Ticket> = self
            .pending
            .iter()
            .filter(|(_, a)| !matches!(a, Asked::Apply))
            .map(|(t, _)| *t)
            .collect();
        self.drop_tickets(older);
        self.structure = None;
        self.count = None;
        self.view = None;
        self.pages.purge();
        self.failed.clear();
        self.events.push(RepositoryEvent::Refreshed);
    }

    fn poll(&mut self) -> Vec<RepositoryEvent> {
        let stale = self.stale.swap(false, Ordering::AcqRel);
        let mut refresh = false;
        for reply in self.client.poll() {
            // Not waited for any more (refreshed, replaced): dropped.
            if let Some(asked) = self.pending.remove(&reply.ticket) {
                refresh |= self.received(asked, reply.result);
            }
        }
        if stale || refresh {
            self.refresh();
        }
        std::mem::take(&mut self.events)
    }

    fn memory(&self) -> usize {
        self.pages.bytes()
    }
}

impl Purge for SessionRepository {
    /// The pages go ("Clear caches"); asked again, they're read again.
    fn purge(&mut self) {
        self.pages.purge();
    }
}

#[cfg(test)]
#[path = "../tests/unit/repository.rs"]
mod tests;
