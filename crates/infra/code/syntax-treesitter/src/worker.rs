//! Parsing on a separate thread. The UI sends new document versions and
//! receives snapshots back, without ever waiting.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::parse::{SyntaxConfig, SyntaxSnapshot, parse_document};
use document::{Document, TextChange, TextSnapshot};
use kernel::DocumentId;
use languages::LanguageId;

#[derive(Debug)]
pub struct ParseJob {
    pub doc: DocumentId,
    pub version: u64,
    pub language: LanguageId,
    pub text: TextSnapshot,
    /// Changes since the last sent version; `None` = parse from scratch.
    pub changes: Option<Vec<TextChange>>,
}

#[derive(Debug)]
pub struct ParseResult {
    pub doc: DocumentId,
    pub snapshot: SyntaxSnapshot,
    pub elapsed: Duration,
}

enum Message {
    Parse(ParseJob),
    Forget(DocumentId),
}

/// Called after each result, so the UI event loop wakes up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug)]
pub struct SyntaxWorker {
    jobs: Sender<Message>,
    results: Receiver<ParseResult>,
}

impl SyntaxWorker {
    pub fn spawn(config: Arc<SyntaxConfig>, waker: Waker) -> std::io::Result<Self> {
        let (jobs, inbox) = mpsc::channel();
        let (outbox, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("syntax".into())
            .spawn(move || run(&config, &inbox, &outbox, &waker))?;
        Ok(Self { jobs, results })
    }

    pub fn submit(&self, job: ParseJob) {
        let _ = self.jobs.send(Message::Parse(job));
    }

    /// Releases the state of a closed document.
    pub fn forget(&self, doc: DocumentId) {
        let _ = self.jobs.send(Message::Forget(doc));
    }

    pub fn try_recv(&self) -> Option<ParseResult> {
        self.results.try_recv().ok()
    }
}

/// A document's latest snapshot (in its version's coordinates), for incremental
/// parsing of the next version.
struct DocState {
    snapshot: SyntaxSnapshot,
}

fn run(
    config: &SyntaxConfig,
    inbox: &Receiver<Message>,
    outbox: &Sender<ParseResult>,
    waker: &Waker,
) {
    let mut states: HashMap<DocumentId, DocState> = HashMap::new();
    while let Ok(first) = inbox.recv() {
        // Merge everything that has piled up: only the latest version matters for each document,
        // but with all the changes along the way (for incremental parsing).
        let mut pending: Vec<ParseJob> = Vec::new();
        for message in std::iter::once(first).chain(inbox.try_iter()) {
            match message {
                Message::Forget(doc) => {
                    states.remove(&doc);
                    pending.retain(|j| j.doc != doc);
                }
                Message::Parse(job) => match pending.iter_mut().find(|j| j.doc == job.doc) {
                    Some(existing) => {
                        let changes = match (existing.changes.take(), job.changes) {
                            (Some(mut a), Some(b)) => {
                                a.extend(b);
                                Some(a)
                            }
                            _ => None,
                        };
                        *existing = ParseJob { changes, ..job };
                    }
                    None => pending.push(job),
                },
            }
        }

        for job in pending {
            let started = Instant::now();
            // The previous snapshot, updated with the changes, enables incremental parsing.
            // Without known changes (`None`), it is parsed from scratch.
            let previous = match (states.remove(&job.doc), &job.changes) {
                (Some(mut state), Some(changes)) => {
                    state.snapshot.apply_changes(changes, job.version);
                    Some(state.snapshot)
                }
                _ => None,
            };
            let changes = job.changes.as_deref().unwrap_or_default();
            // A document that makes the parser crash doesn't stop the thread: it is skipped (no
            // colors), and the other documents keep being parsed.
            let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parse_document(
                    config,
                    &job.language,
                    &job.text,
                    previous.as_ref(),
                    changes,
                    job.version,
                )
            }));
            let snapshot = match parsed {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => {
                    tracing::warn!(doc = %job.doc, language = %job.language, "parsing failed");
                    continue;
                }
                Err(_) => {
                    tracing::error!(doc = %job.doc, language = %job.language, "the parser crashed on the document; skipping it");
                    continue;
                }
            };
            states.insert(job.doc, DocState { snapshot: snapshot.clone() });
            let elapsed = started.elapsed();
            if elapsed > std::time::Duration::from_millis(200) {
                tracing::info!(doc = %job.doc, bytes = job.text.len(), ?elapsed, "slow parse");
            } else {
                tracing::debug!(doc = %job.doc, layers = snapshot.layers.len(), ?elapsed, "parsare");
            }
            if outbox.send(ParseResult { doc: job.doc, snapshot, elapsed }).is_err() {
                return;
            }
            waker();
        }
    }
}

/// The syntax state of an open document, on the UI side.
#[derive(Debug)]
pub struct DocumentSyntax {
    language: LanguageId,
    snapshot: Option<SyntaxSnapshot>,
    /// The last document version sent for parsing.
    sent: Option<u64>,
    /// How many real parses have arrived. A tree only adjusted after an edit (without
    /// reparsing) already has the document's version, but its errors are stale: whatever
    /// depends on the parse result (e.g. errors) is recomputed based on this number.
    parses: u64,
}

impl DocumentSyntax {
    pub fn new(language: LanguageId) -> Self {
        Self { language, snapshot: None, sent: None, parses: 0 }
    }

    pub fn language(&self) -> &LanguageId {
        &self.language
    }

    /// The number of the current real parse (increases with each result received).
    pub fn parses(&self) -> u64 {
        self.parses
    }

    pub fn snapshot(&self) -> Option<&SyntaxSnapshot> {
        self.snapshot.as_ref()
    }

    /// Frees the tree (e.g. a tab not seen for a while); the next `sync` parses from
    /// scratch. The parse counter keeps going, so whatever was cached by it (errors,
    /// colors) can't be mistaken for the new result.
    pub fn release(&mut self) {
        self.snapshot = None;
        self.sent = None;
    }

    /// Sends the document for parsing if it changed; meanwhile adjusts the current snapshot
    /// with the changes, so highlighting doesn't jump.
    pub fn sync(&mut self, doc: &Document, worker: &SyntaxWorker) {
        if self.sent == Some(doc.version()) {
            return;
        }
        let changes = self.sent.and_then(|v| doc.changes_since(v));
        // From the snapshot's own version: a result taken meanwhile (`accept`) may already be
        // at the document's (the changes applied twice would shift its colors).
        if let Some(snapshot) = &mut self.snapshot
            && snapshot.version() < doc.version()
            && let Some(since) = doc.changes_since(snapshot.version())
        {
            snapshot.apply_changes(&since, doc.version());
        }
        worker.submit(ParseJob {
            doc: doc.id(),
            version: doc.version(),
            language: self.language.clone(),
            text: doc.buffer().snapshot(),
            changes,
        });
        self.sent = Some(doc.version());
    }

    /// Takes a parse result. A result older than the document is brought up to
    /// date with the changes made after it.
    pub fn accept(&mut self, mut snapshot: SyntaxSnapshot, doc: &Document) {
        if snapshot.version() < doc.version() {
            match doc.changes_since(snapshot.version()) {
                Some(changes) => snapshot.apply_changes(&changes, doc.version()),
                None => return,
            }
        }
        self.snapshot = Some(snapshot);
        self.parses += 1;
    }
}

#[cfg(test)]
#[path = "../tests/unit/worker.rs"]
mod tests;
