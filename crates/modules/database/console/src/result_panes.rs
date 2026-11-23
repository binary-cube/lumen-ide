//! The panes a result's tab shows under its grid: the shared host
//! (`ui_components::result_pane::PaneHost`: buttons, split, input, kept state) with what's
//! the console's — its rows lent as they come ([`crate::lent_rows`]) and all of them fetched
//! when a pane asks, a page at a time, up to the limit it asks.
//! Which module a pane is this never knows.

use std::path::PathBuf;
use std::sync::Arc;

use ui_components::data_grid::GridColumn;
use ui_components::result_pane::{Held, Lent, PaneHost, PaneRequest};

use crate::lent_rows;
use crate::runtime::Runtime;

/// The folder the panes' states are kept in, in the project's own.
const STATES_DIR: &str = "database/panes";

/// A result's panes (see the module's doc).
#[derive(Debug)]
pub struct Panes {
    pub host: PaneHost,
    /// All the rows are being fetched for a pane, up to this many.
    fetching: Option<usize>,
}

impl Panes {
    /// The panes offered now, their states read for `statement` from `root`'s project.
    pub fn new(root: Option<PathBuf>, statement: &str, runtime: &Runtime) -> Self {
        let folder = root.map(|r| r.join(kernel::brand::PROJECT_DIR).join(STATES_DIR));
        let spawn = |job: Box<dyn FnOnce() + Send>| runtime.spawn(job);
        Self { host: PaneHost::new(folder, statement, &spawn), fetching: None }
    }

    /// The host's poll, the rows lent anew when they changed, all of them fetched while
    /// asked: `true` to draw again, and what else the pane asks (a "Save As", a message) for
    /// the view.
    pub fn poll(
        &mut self,
        view: &crate::runner::ResultView,
        columns: &[GridColumn],
        runner: &crate::runner::Runner,
        runtime: &Runtime,
    ) -> (bool, Option<PaneRequest>) {
        let spawn = |job: Box<dyn FnOnce() + Send>| runtime.spawn(job);
        let (mut changed, request) = self.host.poll(&spawn);
        let request = match request {
            Some(PaneRequest::AllRows { limit }) => {
                self.fetching = Some(limit);
                self.host.lend_again();
                None
            }
            other => other,
        };
        let more = view.more.readable();
        let rows = usize::try_from(view.len).unwrap_or(usize::MAX);
        if let Some(limit) = self.fetching {
            if !more || rows >= limit {
                self.fetching = None;
                self.host.lend_again();
            } else if !view.loading {
                runner.fetch_more(view.id);
            }
        }
        let held = Held { rows, more, fetching: self.fetching.is_some(), ..Held::default() };
        // A version of the rows: which result, its rows let go and read again, how many.
        let version = view.generation ^ view.epoch.rotate_left(24) ^ view.len.rotate_left(48);
        changed |=
            self.host.lend(version, || Arc::new(lent_rows::lent(view, columns)) as Lent, held);
        (changed, request)
    }

    /// Waiting for a pane's work or for its rows.
    pub fn busy(&self) -> bool {
        self.fetching.is_some() || self.host.busy()
    }
}
