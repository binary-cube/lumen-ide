//! A large document's find: the whole search for a new query (typed in the bar, an option
//! changed) runs on a worker, its answer taken at the next wake; the bar's typing never
//! waits for it. Small documents and edits (only their lines searched again) stay here.

use document::WholeSearch;

use super::*;

/// Documents at least this large (bytes) have their whole searches on a worker.
const ON_WORKER: usize = 1 << 20;

/// The worker searching them.
const FIND_THREAD: &str = "find";

/// A whole search's answer for a document: the query's generation, the document's version,
/// the matches.
pub(super) type Found = (DocumentId, u64, u64, Vec<std::ops::Range<usize>>);

/// Where a tab's whole search is sent (the workbench's channel and waker).
pub(super) struct FindSink {
    tx: std::sync::mpsc::Sender<Found>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl FindSink {
    /// The search `tab`'s find needs now, if any: on the worker (without a waker — tests,
    /// snapshots — here, at once).
    pub(super) fn start(&self, tab: &mut Tab) {
        let Some(search) = tab.find.whole_search(&tab.doc, ON_WORKER) else { return };
        let id = tab.doc.id();
        let Some(waker) = self.waker.clone() else {
            let WholeSearch { generation, version, .. } = search;
            tab.find.found(generation, version, search.run());
            return;
        };
        let tx = self.tx.clone();
        kernel::work::run(FIND_THREAD, move || {
            let matches = search.run();
            if tx.send((id, search.generation, search.version, matches)).is_ok() {
                waker();
            }
        });
    }
}

impl Workbench {
    pub(super) fn find_sink(&self) -> FindSink {
        FindSink { tx: self.found.0.clone(), waker: self.waker.as_ref().map(|w| Arc::clone(&w.0)) }
    }

    /// The active tab's whole search, when it needs one (before its matches are drawn).
    pub(super) fn start_find_search(&mut self) {
        let sink = self.find_sink();
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.find_open
        {
            sink.start(tab);
        }
    }

    /// The whole searches answered: their tabs' matches, the match after the cursor selected
    /// (and shown) where the bar asked for it meanwhile. `true`: a redraw.
    pub fn poll_find(&mut self, text: &mut TextSystem) -> bool {
        let done: Vec<Found> = self.found.1.try_iter().collect();
        let mut changed = false;
        for (id, generation, version, matches) in done {
            let palette = &self.palette;
            for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
                if tab.doc.id() != id {
                    continue;
                }
                changed = true;
                if tab.find.found(generation, version, matches.clone()) {
                    tab.find.select_from_cursor(&mut tab.doc);
                    tab.view.reveal_cursor(&tab.doc, &tab.folds, text, palette);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/find_search.rs"]
mod tests;
