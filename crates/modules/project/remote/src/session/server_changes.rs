//! What changes on the server besides uploads: a move or a rename followed there, files
//! removed, automatic upload turned on or off, the manifest kept after a transfer.

use super::*;

/// The folders (relative) of the files `deleted` on the server that aren't in the local copy
/// at `root` any more, the deepest first: up from each file while its folder is gone here
/// (one still here stops the walk — and everything above it is still here too).
pub(super) fn gone_folders<'a>(
    root: &Path,
    deleted: impl Iterator<Item = &'a Transfer>,
) -> Vec<String> {
    let here = |dir: &str| dir.split('/').fold(root.to_path_buf(), |p, part| p.join(part)).exists();
    let mut folders = std::collections::BTreeSet::new();
    for transfer in deleted {
        let mut dir = parent(&transfer.path);
        while !dir.is_empty() && !folders.contains(dir) && !here(dir) {
            folders.insert(dir.to_owned());
            dir = parent(dir);
        }
    }
    let mut folders: Vec<String> = folders.into_iter().collect();
    folders.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    folders
}

impl State {
    /// The files the server got from here at `relative` (one) or under it (a folder's).
    pub(crate) fn sent_under(&self, relative: &str) -> Vec<String> {
        let folder = format!("{relative}/");
        let Ok(manifest) = self.manifest.lock() else { return Vec::new() };
        manifest
            .paths()
            .filter(|p| *p == relative || p.starts_with(&folder))
            .map(str::to_owned)
            .collect()
    }

    /// `from` moved here to `to` (a file or a folder): moved on the server, what was sent
    /// under it following in the manifest. `true` if it was.
    pub(super) fn rename_on_server(
        &self,
        from: &str,
        to: &str,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) -> bool {
        if session.is_none() {
            *session = self.connector.connect().ok();
        }
        let Some(fs) = session.as_mut() else {
            self.tell(Notice::Message(workbench_model::Notification::warning(i18n::t(
                "remote.offline",
            ))));
            return false;
        };
        if let Err(e) = fs.rename(from, to) {
            if e.is_transient() {
                *session = None;
            }
            self.failed(&crate::connect_view::explain(&e));
            return false;
        }
        if let Ok(mut manifest) = self.manifest.lock() {
            let folder = format!("{from}/");
            let moved: Vec<String> = manifest
                .paths()
                .filter(|p| *p == from || p.starts_with(&folder))
                .map(str::to_owned)
                .collect();
            for old in moved {
                let Some(synced) = manifest.get(&old).copied() else { continue };
                let new = format!("{to}{}", &old[from.len()..]);
                manifest.remove(&old);
                manifest.set(&new, synced);
            }
            save_manifest(&self.saved.local, &manifest);
        }
        self.relist_around([from, to].into_iter());
        true
    }

    /// A move waiting in the outbox (`there` on the server is `here` here) made on the
    /// server — the folders it left removed if they're empty now (a folder's files moved
    /// one by one) — then its content sent if it changed here too.
    pub(super) fn send_move(
        self: &Arc<Self>,
        here: &str,
        there: &str,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        if !self.rename_on_server(there, here, session) {
            self.end_sending(std::iter::once(here));
            return;
        }
        self.change_outbox(|o| o.rename_done(here));
        if let Some(fs) = session.as_mut() {
            let mut dir = parent(there);
            while !dir.is_empty() && !self.manifest.lock().is_ok_and(|m| m.has_under(dir)) {
                // Not empty on the server (what it has that never came here): it stays.
                if fs.remove(dir).is_err() {
                    break;
                }
                dir = parent(dir);
            }
        }
        // Its content follows (the upload ends what was sent), or it's done.
        if self.local_changes_of(here).is_some() {
            self.uploads.push_forced([here.to_owned()]);
        } else {
            self.end_sending(std::iter::once(here));
        }
        crate::outgoing::refresh(self);
    }

    /// The outbox changed by `change`, kept on disk if it did.
    pub(crate) fn change_outbox(&self, change: impl FnOnce(&mut Outbox)) {
        let Ok(mut outbox) = self.outbox.lock() else { return };
        let before = outbox.clone();
        change(&mut outbox);
        if *outbox != before {
            save_outbox(&self.saved.local, &outbox);
        }
    }

    /// A save uploads at once (else it waits for the user).
    pub fn auto_upload(&self) -> bool {
        self.auto_upload.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Saves upload at once (`on`) or wait for the user: kept in the connection (wherever
    /// it's saved), the session going on as it is. `Err`: why it couldn't be kept.
    pub fn set_auto_upload(self: &Arc<Self>, on: bool) -> Result<(), String> {
        let mut saved = config::find(&self.saved.local).unwrap_or_else(|| self.saved.clone());
        saved.connection.auto_upload = on;
        config::save(&saved).map_err(|e| e.to_string())?;
        self.auto_upload.store(on, std::sync::atomic::Ordering::Relaxed);
        crate::outgoing::refresh(self);
        (self.wake)();
        Ok(())
    }

    /// Moved here while uploads wait: kept to do on the server (what it has of `from`).
    pub(super) fn note_moved(&self, from: &str, to: &str) {
        let sent = self.sent_under(from);
        self.change_outbox(|o| o.moved(from, to, &sent));
    }

    /// Deleted here while uploads wait: kept to delete on the server (asked first then).
    pub(super) fn note_removed(&self, paths: &[String]) {
        let sent: Vec<(String, Vec<String>)> =
            paths.iter().map(|p| (p.clone(), self.sent_under(p))).collect();
        self.change_outbox(|o| {
            for (path, sent) in &sent {
                o.removed(path, sent);
            }
        });
    }

    /// `paths` deleted here (confirmed): what the server got of them deleted there (logged),
    /// then their folders left empty, the deepest first.
    pub(super) fn remove_on_server(
        &self,
        paths: &[String],
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        let files: Vec<String> = paths.iter().flat_map(|p| self.sent_under(p)).collect();
        let changes: Vec<Change> = files
            .iter()
            .map(|path| Change {
                path: path.clone(),
                action: Action::DeleteRemote,
                conflict: false,
                chosen: true,
                remote: None,
                local: None,
            })
            .collect();
        // Never two sessions held at once: this one back to the pool before the removals.
        *session = None;
        let local = local_copy(&self.saved.local);
        let done = self.run_all(&changes, &local);
        if let Some(why) = done.iter().find_map(|t| t.error.clone()) {
            self.failed(&why);
        }
        // Deleted there: no longer waiting (one that failed stays).
        self.change_outbox(|o| {
            for t in done.iter().filter(|t| t.error.is_none()) {
                o.delete_done(&t.path);
            }
        });
        // The folders of the files deleted that aren't here any more (a folder deleted here,
        // its files sent one by one from the list): gone there too, the deepest first. A
        // folder still holding something on the server stays (it's never emptied).
        let folders = gone_folders(&self.saved.local, done.iter().filter(|t| t.error.is_none()));
        if folders.is_empty() {
            return;
        }
        *session = self.connector.connect().ok();
        if let Some(fs) = session.as_mut() {
            for dir in &folders {
                let _ = fs.remove(dir);
            }
        }
        self.relist_around(folders.iter().map(String::as_str));
    }

    /// What transfers `done` changed in `manifest` (a copy they worked on) put into the
    /// session's manifest — only their entries, so transfers running at once never undo each
    /// other's — and kept on disk; what's known of the server follows them, and their folders
    /// are listed again (the tree's marks).
    pub fn keep(&self, manifest: Manifest, done: &[Transfer]) {
        if let Ok(mut kept) = self.manifest.lock() {
            for transfer in done.iter().filter(|t| t.error.is_none()) {
                match manifest.get(&transfer.path) {
                    Some(synced) => kept.set(&transfer.path, *synced),
                    None => kept.remove(&transfer.path),
                }
            }
            // Written while held: two sessions' writes never interleave on disk.
            save_manifest(&self.saved.local, &kept);
        }
        if let Ok(mut listings) = self.listings.lock() {
            for t in done.iter().filter(|t| t.error.is_none()) {
                match manifest.get(&t.path) {
                    Some(s) => learn(
                        &mut listings,
                        RemoteEntry {
                            path: t.path.clone(),
                            is_dir: false,
                            size: s.remote_size,
                            modified: s.remote_modified,
                        },
                    ),
                    None => {
                        if let Some(entries) = listings.get_mut(parent(&t.path)) {
                            entries.retain(|e| e.path != t.path);
                        }
                    }
                }
            }
        }
        self.relist_around(done.iter().map(|t| t.path.as_str()));
    }
}
