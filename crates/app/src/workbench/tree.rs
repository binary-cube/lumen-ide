//! The project's entries on disk: the watcher's changes announced (whoever shows them
//! follows: the project tree's module), the index marked out of date, tabs of files written
//! by others reloaded; a new entry asked of the project panel.

use super::*;

/// The worker reading files changed on disk for their tabs.
const RELOAD_THREAD: &str = "file-reload";

/// A file read again for its tabs: its path, its stamp when asked, what it holds.
pub(super) type Reloaded = (PathBuf, Option<DiskStamp>, document::Decoded);

impl Workbench {
    /// Applies the changes on disk to the tree (files created, deleted, renamed).
    pub fn poll_fs(&mut self) -> bool {
        let (Some(watcher), Some(tree)) = (&self.tree_watcher, &mut self.project) else {
            return false;
        };
        let mut changed = false;
        // Just started watching, or events were dropped: what changed may have been missed —
        // everyone following the entries looks again (the indexes too), and so do the open
        // tabs (a file another program changed meanwhile: its write wasn't heard).
        let rescan = watcher.take_rescan();
        if rescan {
            let root = tree.root_path().to_path_buf();
            self.data.events.publish(&workspace::events::EntriesRescan { root });
        }
        let paths = watcher.drain_changed();
        // Announced: whoever shows the project's entries follows (the tree, wherever it is).
        if !paths.is_empty() {
            let event = workspace::events::EntriesChanged { paths: paths.clone() };
            self.data.events.publish(&event);
        }
        // Each folder read once, however many of its files changed.
        let listed = listing::ListedFs::new(&*self.ports.disk, &self.support.project_services);
        match workspace::refresh_paths(tree, &paths, false, &listed) {
            Ok(c) => changed |= c,
            Err(e) => tracing::debug!(error = %e, "can't reread a changed folder"),
        }
        // Files written (by others: a download, another program; our saves too): announced
        // (the index reads them again), and their tabs follow.
        let written = watcher.drain_written();
        if !written.is_empty() {
            let event = workspace::events::FilesWritten { paths: written.clone() };
            self.data.events.publish(&event);
        }
        // The project's own files (its marks, its settings): whoever holds them, and their tabs.
        let own = watcher.drain_own_written();
        if !own.is_empty() {
            self.data.events.publish(&workspace::events::OwnFilesWritten { paths: own.clone() });
        }
        let mut looked = written;
        if rescan {
            // Only those whose stamp moved are read again (on a worker).
            let open = all_tabs_mut(&mut self.tabs, &mut self.editors.groups)
                .filter_map(|t| t.doc.path().map(Path::to_path_buf))
                .collect::<Vec<_>>();
            looked.extend(open.into_iter().filter(|p| !own.contains(p)));
        }
        changed | self.reload_written(&looked) | self.reload_written(&own) | self.poll_reloads()
    }

    /// The open tabs of files changed on disk: reloaded (their cursors kept) when they have
    /// no unsaved changes; else those are kept, the user told, and the automatic save won't
    /// write over the file. Our own saves (the same text) change nothing. The files are read
    /// and decoded on a worker (a log another program appends to is read whole each time:
    /// never on the UI thread), what they hold applied when it's back ([`Self::poll_reloads`]);
    /// without a waker (tests, snapshots) here and now. `true`: a redraw.
    pub(super) fn reload_written(&mut self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }
        // A view in its file's place (an image, a diagram) reads it again itself.
        let mut views = false;
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if !tab.holds_text()
                && let Some(viewer) = tab.viewer.as_mut()
                && tab.doc.path().is_some_and(|p| paths.iter().any(|w| w == p))
            {
                viewer.viewer.file_changed();
                views = true;
            }
        }
        // Only the open tabs whose file's stamp moved (our own save, reported now: not read).
        let mut asked: Vec<(PathBuf, Option<DiskStamp>)> = Vec::new();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            let Some(path) = tab.doc.path().filter(|p| paths.iter().any(|w| w == p)) else {
                continue;
            };
            if !tab.holds_text() || tab.untitled.is_some() {
                continue;
            }
            // Our own save, being written: its result says the stamp.
            if self.saving.is_running(path) {
                continue;
            }
            let now = self.ports.disk.stamp(path);
            if now.is_some() && now == tab.disk_stamp {
                continue;
            }
            if !asked.iter().any(|(p, _)| p == path) {
                asked.push((path.to_path_buf(), now));
            }
        }
        if asked.is_empty() {
            return views;
        }
        let disk = Arc::clone(&self.ports.disk);
        let read = move |asked: Vec<(PathBuf, Option<DiskStamp>)>| -> Vec<Reloaded> {
            asked
                .into_iter()
                .filter_map(|(path, stamp)| {
                    let bytes = document::DocumentStorage::read(&*disk, &path).ok()?;
                    Some((path, stamp, document::decode(&bytes)))
                })
                .collect()
        };
        let Some(waker) = self.waker.as_ref().map(|w| Arc::clone(&w.0)) else {
            return self.apply_reloaded(read(asked));
        };
        let tx = self.reloads.0.clone();
        kernel::work::run(RELOAD_THREAD, move || {
            if tx.send(read(asked)).is_ok() {
                waker();
            }
        });
        false
    }

    /// The window taken again: the open tabs' files whose stamp moved meanwhile read again
    /// (the watcher may have lost their writes: macOS loses events while the disk is very
    /// busy). The stamps are compared on the worker, only the files changed read there
    /// (applied as [`Self::reload_written`]'s); one check at a time. Without a waker (tests,
    /// snapshots) here and now. `true`: a redraw.
    pub fn check_open_files(&mut self) -> bool {
        let mut known: Vec<(PathBuf, Option<DiskStamp>)> = Vec::new();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            let Some(path) = tab.doc.path() else { continue };
            if !tab.holds_text() || tab.untitled.is_some() || known.iter().any(|(p, _)| p == path) {
                continue;
            }
            known.push((path.to_path_buf(), tab.disk_stamp));
        }
        if known.is_empty() || self.files_checking.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return false;
        }
        let disk = Arc::clone(&self.ports.disk);
        let checking = Arc::clone(&self.files_checking);
        let check = move || -> Vec<Reloaded> {
            let read = known
                .into_iter()
                .filter_map(|(path, before)| {
                    let now = disk.stamp(&path);
                    // Gone (its tab says so when it's written to), or as it was.
                    if now.is_none() || now == before {
                        return None;
                    }
                    let bytes = document::DocumentStorage::read(&*disk, &path).ok()?;
                    Some((path, now, document::decode(&bytes)))
                })
                .collect();
            checking.store(false, std::sync::atomic::Ordering::Release);
            read
        };
        let Some(waker) = self.waker.as_ref().map(|w| Arc::clone(&w.0)) else {
            return self.apply_reloaded(check());
        };
        let tx = self.reloads.0.clone();
        kernel::work::run(RELOAD_THREAD, move || {
            let read = check();
            if !read.is_empty() && tx.send(read).is_ok() {
                waker();
            }
        });
        false
    }

    /// The files read again on the worker, applied to their tabs. `true`: a redraw.
    pub(super) fn poll_reloads(&mut self) -> bool {
        let done: Vec<Reloaded> = self.reloads.1.try_iter().flatten().collect();
        !done.is_empty() && self.apply_reloaded(done)
    }

    /// What the files hold now, applied to their tabs (as [`Self::reload_written`] says).
    fn apply_reloaded(&mut self, read: Vec<Reloaded>) -> bool {
        let mut reloaded = false;
        let mut kept = Vec::new();
        for (path, now, decoded) in read {
            for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
                if tab.doc.path() != Some(path.as_path()) || !tab.holds_text() {
                    continue;
                }
                // As it is now: seen (whatever comes of it below).
                tab.disk_stamp = now;
                // Our own last save, reported late (the text may have changed since).
                if tab.saved_text == Some(text_fingerprint([decoded.text.as_str()].into_iter())) {
                    continue;
                }
                if tab.doc.buffer().len() == decoded.text.len()
                    && same_text(tab.doc.buffer().chunks(), &decoded.text)
                {
                    // Ours (just saved), or nothing new.
                    continue;
                }
                if tab.doc.is_dirty() {
                    if !tab.disk_changed {
                        tab.disk_changed = true;
                        kept.push(path.clone());
                    }
                    continue;
                }
                reloaded |= tab.doc.reload(decoded.clone());
            }
        }
        for path in kept {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            workbench_model::WorkbenchHost::notify(
                self,
                workbench_model::Notification::warning(i18n::tr!(
                    "files.changed_on_disk",
                    name = name
                )),
            );
        }
        reloaded
    }

    #[cfg(any(test, feature = "snapshot"))]
    /// Starts a new file / folder, typed in the project panel (the tree's module).
    pub fn new_entry(&mut self, is_dir: bool) -> bool {
        self.tell_project_panel(workbench_model::EntriesRequest::NewEntry { is_dir }, true)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/tree.rs"]
mod tests;

/// Whether `chunks` make up `text` (compared piece by piece: no copy of the document).
fn same_text<'a>(chunks: impl Iterator<Item = &'a str>, text: &str) -> bool {
    let mut rest = text;
    for chunk in chunks {
        match rest.strip_prefix(chunk) {
            Some(after) => rest = after,
            None => return false,
        }
    }
    rest.is_empty()
}
