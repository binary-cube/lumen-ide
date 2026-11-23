//! The panes a table's data shows under its grid: the shared host
//! (`ui_components::result_pane::PaneHost`) with what's the explorer's — the table's rows
//! read in the background for the pane once it's open ([`crate::pane_rows`]: the order and
//! filter shown, the first page at first, all of them up to the limit the pane asks), and
//! its state kept in the project with the table's name. Which module a pane is this never
//! knows.

use ui_components::data_grid::GridColumn;
use ui_components::result_pane::{Held, Lent, PaneHost, PaneRequest};

use kernel::Rect;

use super::*;
use crate::pane_rows::{LentPages, PaneRows, ReadAsk};

/// The rows read at first for a pane (a page; all of them when it asks).
const FIRST_ROWS: usize = 500;
/// The folder the panes' states are kept in, in the project's own.
const STATES_DIR: &str = "database/panes";

/// A table's panes.
#[derive(Debug)]
pub(crate) struct TablePanes {
    pub host: PaneHost,
    reading: Option<PaneRows>,
    /// The room its grid and its pane share, drawn last.
    pub whole: Rect,
}

impl DataView {
    /// A pane takes the keys and the text now (its list open).
    pub(super) fn pane_captures(&self) -> bool {
        self.tab == Tab::Data && self.panes.as_ref().is_some_and(|panes| panes.host.captures())
    }

    /// The panes over the table just shown (their states read for it).
    pub(super) fn make_panes(&mut self) {
        let (Some(open), Some(info)) = (&self.open, self.table()) else {
            self.panes = None;
            return;
        };
        let folder = Some(open.sessions.root().join(kernel::brand::PROJECT_DIR).join(STATES_DIR));
        let shown = format!(
            "{}\u{1f}{}",
            self.source
                .as_ref()
                .map_or_else(|| self.path.display().to_string(), |(_, name)| name.clone()),
            info.table
        );
        let runtime = open.sessions.runtime().clone();
        let spawn = |job: Box<dyn FnOnce() + Send>| runtime.spawn(job);
        self.panes = Some(TablePanes {
            host: PaneHost::new(folder, &shown, &spawn),
            reading: None,
            whole: Rect::default(),
        });
    }

    /// The panes' work: their states, what they ask (the table read again, up to the limit
    /// asked), the rows lent as pages come; `true` to draw again.
    pub(super) fn follow_panes(&mut self) -> bool {
        let (Some(panes), Some(open), Some(grid)) = (&mut self.panes, &self.open, &self.grid)
        else {
            return false;
        };
        let runtime = open.sessions.runtime().clone();
        let spawn = |job: Box<dyn FnOnce() + Send>| runtime.spawn(job);
        let (mut changed, request) = panes.host.poll(&spawn);
        if panes.host.pane_ref().is_none() {
            // Closed: what was read let go.
            panes.reading = None;
            return changed;
        }
        let limit = match request {
            Some(PaneRequest::AllRows { limit }) => Some(limit),
            Some(PaneRequest::SaveAs { name }) => {
                self.pane_saving = true;
                let start = self.path.parent().map(Path::to_path_buf);
                self.action = Some(ViewerAction::Request(AppRequest::PickSavePath { start, name }));
                None
            }
            Some(PaneRequest::Tell { text, ok }) => {
                self.told.push(if ok {
                    Notification::success(text)
                } else {
                    Notification::error(text)
                });
                None
            }
            // The host's own (never passed on).
            Some(PaneRequest::WholeRows) => None,
            None if panes.reading.is_none() => Some(FIRST_ROWS),
            None => None,
        };
        let (order, filter) = grid.source().query();
        // A new order or filter: read again as shown.
        let stale = panes
            .reading
            .as_ref()
            .is_some_and(|r| r.ask().order != order || r.ask().filter != filter);
        if let Some(limit) = limit
            .or_else(|| stale.then(|| panes.reading.as_ref().map_or(FIRST_ROWS, |r| r.ask().limit)))
        {
            let ask = ReadAsk { table: grid.source().table().clone(), order, filter, limit };
            let wake = self.waker.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let wake: Arc<dyn Fn() + Send + Sync> = wake.unwrap_or_else(|| Arc::new(|| {}));
            panes.reading = Some(PaneRows::start(&open.worker, runtime.clone(), ask, wake));
            panes.host.lend_again();
        }
        let Some(reading) = &panes.reading else { return changed };
        if let Some(why) = reading.take_failure() {
            self.told.push(Notification::error(why));
        }
        let read = reading.read();
        let held = Held {
            rows: read.rows,
            more: read.more || !read.done,
            fetching: !read.done,
            ..Held::default()
        };
        let (pages, version) = (read.pages, read.version);
        let columns: Vec<charts_model::Column> =
            grid.source().columns().iter().map(GridColumn::chart_column).collect();
        let lent = move || std::sync::Arc::new(LentPages::chained(columns, pages)) as Lent;
        changed |= panes.host.lend(version, lent, held);
        changed
    }
}
