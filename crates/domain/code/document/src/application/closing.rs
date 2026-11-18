//! Closing documents ("Close Tab", "Close All", the window, quitting): one after another,
//! a modified one asking first — save it, discard its changes, or cancel everything. The
//! rules without the UI: what the documents are now is asked through [`OpenDocuments`];
//! asking, saving and closing are the caller's. `T` is what follows once all are closed
//! (the window closing, the app quitting…), handed back at the end.

use std::collections::VecDeque;

use kernel::DocumentId;

/// The open documents, as the closing asks about them.
pub trait OpenDocuments {
    /// Still open (one closed meanwhile is skipped).
    fn is_open(&self, id: DocumentId) -> bool;
    /// It has changes not saved.
    fn is_modified(&self, id: DocumentId) -> bool;
}

/// What to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStep {
    /// Close it (no changes): it's off the queue.
    Close(DocumentId),
    /// Ask whether to save it; the closing waits for [`CloseDocuments::resolved`] (saved
    /// or discarded, and closed) or is dropped (cancelled).
    Ask(DocumentId),
    /// All closed: [`CloseDocuments::into_then`] says what follows.
    Done,
}

/// A closing in progress: the documents left, in the order they close.
#[derive(Debug)]
pub struct CloseDocuments<T> {
    queue: VecDeque<DocumentId>,
    then: T,
}

impl<T> CloseDocuments<T> {
    pub fn new(documents: impl IntoIterator<Item = DocumentId>, then: T) -> Self {
        Self { queue: documents.into_iter().collect(), then }
    }

    /// The next step: closed ones skipped, an unmodified one closed, a modified one asked
    /// about (it stays first until resolved).
    pub fn next(&mut self, documents: &dyn OpenDocuments) -> CloseStep {
        while let Some(&id) = self.queue.front() {
            if !documents.is_open(id) {
                self.queue.pop_front();
                continue;
            }
            if documents.is_modified(id) {
                return CloseStep::Ask(id);
            }
            self.queue.pop_front();
            return CloseStep::Close(id);
        }
        CloseStep::Done
    }

    /// The one asked about was saved or its changes discarded (and it was closed): the
    /// closing goes on with the next.
    pub fn resolved(&mut self) {
        self.queue.pop_front();
    }

    /// What follows the closing (once [`CloseStep::Done`]).
    pub fn into_then(self) -> T {
        self.then
    }
}

#[cfg(test)]
#[path = "../../tests/unit/application/closing.rs"]
mod tests;
