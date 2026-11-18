//! The tables list's row counts. Never in the way of the rows: the list first shows the
//! engine's estimates, as cheap as the list itself (PostgreSQL's `reltuples` or its
//! statistics' live rows, MySQL's `TABLE_ROWS`, SQLite's `sqlite_stat1`), marked `~`; a
//! table is counted exactly ([`Request::RowCount`]: after everything else, on the source's
//! counting session where it has one) only when that's cheap (a small SQLite file: all of
//! them), or asked — the table under the mouse, not a huge one —, at most [`IN_FLIGHT`] at
//! once; the table shown takes its grid's own total. What's asked is let go when the
//! tables are listed again, and with the view (its client).

use std::collections::{HashMap, VecDeque};

use database::{TableInfo, TableKind};

use super::paint::short_count;
use crate::grid_source::COUNT_ON_REQUEST;
use crate::worker::{Client, Reply, Request, Response, Ticket};

/// Exact counts asked at once (the rest wait here, in order).
pub const IN_FLIGHT: usize = 2;

/// A file-based source up to this size counts all its tables exactly (cheap).
pub const CHEAP_FILE: u64 = 32 * 1024 * 1024;

/// A table's rows as the list shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListCount {
    pub rows: u64,
    /// Counted (else the engine's estimate).
    pub exact: bool,
    /// As drawn (`~29.6k`, `73`): made once, not every frame.
    pub label: String,
}

impl ListCount {
    pub fn estimate(rows: u64) -> Self {
        Self { rows, exact: false, label: format!("~{}", short_count(rows)) }
    }

    pub fn exact(rows: u64) -> Self {
        Self { rows, exact: true, label: short_count(rows) }
    }
}

/// The list's counts (see the module's doc).
#[derive(Debug, Default)]
pub struct ListCounts {
    shown: Vec<Option<ListCount>>,
    /// Tables to count exactly, in order (the one asked for first).
    wanted: VecDeque<usize>,
    /// Counts asked and not answered, by ticket.
    asked: HashMap<Ticket, usize>,
}

impl ListCounts {
    /// `tables` listed (again): their estimates shown, what was asked let go; every table
    /// counted exactly when `all` (cheap).
    pub fn reset(&mut self, client: Option<&Client>, tables: &[TableInfo], all: bool) {
        self.cancel(client);
        self.shown = tables.iter().map(|t| t.rows.map(ListCount::estimate)).collect();
        if all {
            self.wanted = (0..tables.len()).collect();
        }
        self.pump(client, tables);
    }

    /// What `client` asked and nothing came of yet, let go.
    pub fn cancel(&mut self, client: Option<&Client>) {
        self.wanted.clear();
        for (ticket, _) in self.asked.drain() {
            if let Some(client) = client {
                client.cancel(ticket);
            }
        }
    }

    pub fn get(&self, i: usize) -> Option<&ListCount> {
        self.shown.get(i).and_then(Option::as_ref)
    }

    /// The tables counted exactly, with their places (what a structure change warns of:
    /// never an estimate).
    pub fn exact(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.shown
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_ref().filter(|c| c.exact).map(|c| (i, c.rows)))
    }

    /// Counts asked and not answered yet.
    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.asked.len()
    }

    /// Tables waiting to be counted (not asked yet).
    #[cfg(test)]
    pub fn waiting(&self) -> usize {
        self.wanted.len()
    }

    /// Table `i` counted exactly, before those waiting (the one under the mouse); not one
    /// already counted or asked, nor a huge one (`COUNT(*)` takes long: its estimate
    /// stays).
    pub fn want(&mut self, i: usize, client: Option<&Client>, tables: &[TableInfo]) -> bool {
        let Some(info) = tables.get(i) else { return false };
        let counted = self.get(i).is_some_and(|c| c.exact);
        let huge = info.rows.is_some_and(|n| n > COUNT_ON_REQUEST);
        let view = info.kind == TableKind::View;
        if counted || huge || view || self.asked.values().any(|&a| a == i) {
            return false;
        }
        self.wanted.retain(|&w| w != i);
        self.wanted.push_front(i);
        self.pump(client, tables);
        true
    }

    /// Table `i`'s exact rows known otherwise (its grid counted them): shown, not asked.
    pub fn set_exact(&mut self, i: usize, rows: u64) -> bool {
        let Some(slot) = self.shown.get_mut(i) else { return false };
        if slot.as_ref().is_some_and(|c| c.exact && c.rows == rows) {
            return false;
        }
        *slot = Some(ListCount::exact(rows));
        self.wanted.retain(|&w| w != i);
        true
    }

    /// A reply, if it's one of ours (`Some(changed)`); the next ones asked.
    pub fn take(
        &mut self,
        reply: &Reply,
        client: Option<&Client>,
        tables: &[TableInfo],
    ) -> Option<bool> {
        let i = self.asked.remove(&reply.ticket)?;
        let changed = match &reply.result {
            Ok(Response::Count(n)) => self.set_exact(i, *n),
            // Refused, failed, cancelled: the estimate stays.
            _ => false,
        };
        self.pump(client, tables);
        Some(changed)
    }

    /// Asks the next ones, up to [`IN_FLIGHT`] at once.
    fn pump(&mut self, client: Option<&Client>, tables: &[TableInfo]) {
        let Some(client) = client else { return };
        while self.asked.len() < IN_FLIGHT {
            let Some(i) = self.wanted.pop_front() else { break };
            let Some(info) = tables.get(i) else { continue };
            if self.get(i).is_some_and(|c| c.exact) || info.kind == TableKind::View {
                // A view's rows are its query's: counted only by its grid.
                continue;
            }
            let ticket = client.send(Request::RowCount(info.table.clone()));
            self.asked.insert(ticket, i);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view/counts.rs"]
mod tests;
