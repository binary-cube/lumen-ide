//! Automatic saving off the UI thread. A tab's text is made ready to write here (cleaned up,
//! encoded: what the document is now), then written on the `file-save` worker — the version
//! before kept first (`FileWillSave`, the local history), the file replaced through a
//! temporary one and synced to the disk, which can take tens of milliseconds, hundreds on
//! Windows while an antivirus holds the file — and told back by a poll: the version written
//! marked saved (typed on meanwhile: still unsaved against it), the others told
//! (`FileSaved`). A file has one save at a time: a save asked while one runs (the user's own)
//! waits for it first; the watcher's report of our own write isn't a change while it runs.
//! A window closing waits for its saves.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use document::{Encoding, LineEnding, PreparedSave};
use kernel::DocumentId;
use workspace::NodeKind;

use super::Workbench;
use super::tab::text_fingerprint;

/// The longest a save asked by the user (or a closing window) waits for one under way.
const WAIT: Duration = Duration::from_secs(10);

/// A save written on the worker.
#[derive(Debug)]
pub(super) struct Written {
    doc: DocumentId,
    path: PathBuf,
    version: u64,
    format: (Encoding, LineEnding),
    /// The encoding it had, when the save made it UTF-8 (the user is told).
    reencoded_from: Option<Encoding>,
    /// What its text was (the other copies of the file compare theirs with it).
    text: u64,
    /// The file as written (its stamp, its length), or why it wasn't.
    result: io::Result<Option<workspace::Stamp>>,
}

/// The saves under way.
#[derive(Debug)]
pub(super) struct Saving {
    running: HashSet<PathBuf>,
    done: (Sender<Written>, Receiver<Written>),
}

impl Default for Saving {
    fn default() -> Self {
        Self { running: HashSet::new(), done: channel() }
    }
}

impl Saving {
    /// A save of `path` under way (its file being written now).
    pub(super) fn is_running(&self, path: &Path) -> bool {
        self.running.contains(path)
    }
}

/// A window closing: its saves finished first (never a file left with an older version).
impl Drop for Saving {
    fn drop(&mut self) {
        while !self.running.is_empty() {
            match self.done.1.recv_timeout(WAIT) {
                Ok(written) => {
                    self.running.remove(&written.path);
                }
                Err(_) => break,
            }
        }
    }
}

impl Workbench {
    /// Tab `i`'s text saved on the worker (an automatic save); without a waker (tests:
    /// nothing would take the result), here and now. `false`: not started (a save of its
    /// file under way: the next pause saves what's left).
    pub(super) fn save_in_background(&mut self, i: usize) -> bool {
        let Some(waker) = self.waker.as_ref().map(|w| Arc::clone(&w.0)) else {
            if let Err(e) = self.save_tab(i, None) {
                tracing::warn!(path = ?self.tabs[i].doc.path(), error = %e, "autosave failed");
            }
            return true;
        };
        let tab = &mut self.tabs[i];
        if tab.doc.path().is_some_and(|p| self.saving.running.contains(p)) {
            return false;
        }
        // As the user's own save: the final newline the file's or the setting's.
        let cleanup = tab.doc.save_cleanup();
        tab.doc.set_save_cleanup(cleanup.or_final_newline(crate::final_newline::enabled()));
        let encoding = tab.doc.encoding();
        let prepared = document::prepare_save(&mut tab.doc, None);
        tab.doc.set_save_cleanup(cleanup);
        let PreparedSave { path, bytes, version, format } = match prepared {
            Ok(prepared) => prepared,
            Err(e) => {
                tracing::warn!(path = ?tab.doc.path(), error = %e, "autosave failed");
                return false;
            }
        };
        let text = text_fingerprint(tab.doc.buffer().chunks());
        let doc = tab.doc.id();
        let reencoded_from = (format.0 != encoding).then_some(encoding);
        self.saving.running.insert(path.clone());
        let (disk, events, tx) =
            (Arc::clone(&self.ports.disk), self.data.events.clone(), self.saving.done.0.clone());
        kernel::work::run("file-save", move || {
            // What's on disk is still the previous version (the local history keeps it).
            if disk.kind(&path) == Some(NodeKind::File) {
                events.publish(&document::events::FileWillSave { path: path.clone() });
            }
            let result =
                document::DocumentStorage::write(&*disk, &path, &bytes).map(|()| disk.stamp(&path));
            let written = Written { doc, path, version, format, reencoded_from, text, result };
            if tx.send(written).is_ok() {
                waker();
            }
        });
        true
    }

    /// The saves written since, told: their versions marked saved, the others told.
    /// `true` if any came.
    pub fn poll_saves(&mut self) -> bool {
        let done: Vec<Written> = self.saving.done.1.try_iter().collect();
        let any = !done.is_empty();
        for written in done {
            self.saved(written);
        }
        any
    }

    /// The saves of `path` under way finished and told (a save of it asked meanwhile: one
    /// at a time, in order).
    pub(super) fn finish_saves_of(&mut self, path: &Path) {
        while self.saving.running.contains(path) {
            match self.saving.done.1.recv_timeout(WAIT) {
                Ok(written) => self.saved(written),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    tracing::warn!(?path, "a save under way didn't finish in time");
                    self.saving.running.remove(path);
                }
            }
        }
    }

    fn saved(&mut self, written: Written) {
        let Written { doc, path, version, format, reencoded_from, text, result } = written;
        self.saving.running.remove(&path);
        let stamp = match result {
            Ok(stamp) => stamp,
            Err(e) => {
                tracing::warn!(?path, error = %e, "autosave failed");
                return;
            }
        };
        tracing::info!(?path, "file saved");
        if let Some((len, _)) = stamp {
            self.data.disk.written(len);
        }
        // Its tab (wherever it is now), still on that file: the version written saved.
        let mut title = None;
        for tab in super::tab::all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if tab.doc.id() == doc && tab.doc.path() == Some(path.as_path()) {
                tab.doc.mark_saved_version(version, format);
                tab.disk_changed = false;
                tab.disk_stamp = stamp;
                tab.saved_text = Some(text);
                tab.untitled = None;
                title = Some(tab.title());
            }
        }
        self.saved_everywhere(&path, text, format);
        self.data.events.publish(&document::events::FileSaved { path: path.clone() });
        if let (Some(from), Some(name)) = (reencoded_from, title) {
            let message = i18n::tr!("files.saved_as_utf8", name = name, from = from.label());
            workbench_model::WorkbenchHost::notify(
                self,
                workbench_model::Notification::info(message),
            );
        }
    }

    /// `path` written with text `text` in `format`: the other copies of the file (in other
    /// groups: they follow the one saved) don't take its report as a change on disk; those
    /// holding the text written are saved too (a save changes no version: their sync wouldn't
    /// run for it).
    pub(super) fn saved_everywhere(
        &mut self,
        path: &Path,
        text: u64,
        format: (Encoding, LineEnding),
    ) {
        for tab in super::tab::all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if tab.doc.path() != Some(path) {
                continue;
            }
            tab.saved_text = Some(text);
            if tab.doc.is_dirty() && text_fingerprint(tab.doc.buffer().chunks()) == text {
                tab.doc.set_encoding(format.0);
                tab.doc.set_line_ending(format.1);
                tab.doc.mark_saved(path.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/saving.rs"]
mod tests;
