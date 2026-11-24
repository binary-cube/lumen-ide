//! Uploads after a save: checked against the server's copy, kept pending when they can't
//! go, what is still to upload, the local changes since the last transfer, the files being
//! sent.

use super::*;

/// How `entry` of the local copy stands if the server hasn't it as it is here (changed
/// here, or never sent); `None`: it has. Small files are read to tell a touched one from a
/// changed one.
fn changed_here(
    local: &fs_local::remote_copy::LocalCopy,
    manifest: &Manifest,
    entry: &remote::LocalEntry,
) -> Option<Standing> {
    let hash = |p: &str| {
        (entry.size <= HASHED_FOR_MARKS)
            .then(|| local.read(p).ok().map(|b| remote::content_hash(&b)))
            .flatten()
    };
    match standing(entry, manifest, None, &hash) {
        s @ (Standing::OnlyHere | Standing::Ahead) => Some(s),
        _ => None,
    }
}

impl State {
    /// Uploads `relative` (a save), unless the server's copy changed since the last transfer
    /// (the user is asked then).
    pub(crate) fn upload(
        &self,
        relative: &str,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) -> Uploaded {
        // Compared and saved: that's the user's resolution — it goes up as it is.
        if self.resolving.lock().is_ok_and(|mut r| r.remove(relative)) {
            return match self.transfer(relative, Action::Upload, session) {
                Ok(()) => Uploaded::Done,
                Err(why) => Uploaded::Failed(why),
            };
        }
        let local = local_copy(&self.saved.local);
        let Some(here) = local.stat(relative) else { return Uploaded::Gone };
        // The server's copy now (a fresh session if the old one dropped).
        let mut server = None;
        for _ in 0..self.retry().attempts.max(1) {
            if session.is_none() {
                *session = self.connector.connect().ok();
            }
            let Some(fs) = session.as_mut() else { continue };
            match fs.stat(relative) {
                Ok(found) => {
                    server = Some(found);
                    break;
                }
                Err(e) if e.is_transient() => *session = None,
                Err(e) => return Uploaded::Failed(crate::connect_view::explain(&e)),
            }
        }
        let Some(server) = server else { return Uploaded::Offline };
        let changed_there = self.changed_there(relative, server.as_ref());
        if changed_there && let (Some(there), Some(fs)) = (&server, session.as_mut()) {
            // The tree marks it as changed on both sides.
            if let Ok(mut listings) = self.listings.lock() {
                learn(&mut listings, there.clone());
            }
            self.relist_around(std::iter::once(relative));
            if let Ok(mut conflicts) = self.conflicts.lock() {
                conflicts.insert(relative.to_owned());
            }
            let server_text = fs.read(relative).map(|b| String::from_utf8_lossy(&b).into_owned());
            let local_text = local.read(relative).map(|b| String::from_utf8_lossy(&b).into_owned());
            if let (Ok(server_text), Ok(local_text)) = (server_text, local_text) {
                self.tell(Notice::Conflict {
                    path: self.saved.local.join(relative),
                    relative: relative.to_owned(),
                    local: local_text,
                    server: server_text,
                });
            }
            return Uploaded::Conflict;
        }
        // Back to the pool before the transfer takes one (never two held at once).
        *session = None;
        let change = Change {
            path: relative.to_owned(),
            action: Action::Upload,
            conflict: false,
            chosen: true,
            remote: server.map(|s: RemoteEntry| s),
            local: Some(here),
        };
        match self.run(&change, &local) {
            Ok(()) => Uploaded::Done,
            Err(why) => Uploaded::Failed(why),
        }
    }

    /// Uploaded: not pending any more; the server is reachable again.
    pub(crate) fn uploaded(&self, relative: &str) {
        self.settled(relative);
        self.forget_pending(relative);
        self.offline.store(false, std::sync::atomic::Ordering::Relaxed);
        self.failures.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// `relative` still to upload, and why.
    pub(crate) fn keep_pending(&self, relative: &str, why: &str) {
        let added = self
            .pending
            .lock()
            .is_ok_and(|mut p| p.insert(relative.to_owned(), why.to_owned()).is_none());
        if added {
            self.announce();
        }
    }

    pub(crate) fn forget_pending(&self, relative: &str) {
        if self.pending.lock().is_ok_and(|mut p| p.remove(relative).is_some()) {
            self.announce();
        }
    }

    /// What's still to upload, and why it couldn't go.
    pub fn to_upload(&self) -> BTreeMap<String, String> {
        self.pending.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// The files here the server hasn't as they are (changed here, or never sent), but
    /// those excluded: what a Push would send. Reads the disk (a background thread's work).
    pub fn local_changes(&self) -> Vec<(String, Standing)> {
        let local = local_copy(&self.saved.local);
        let Ok(manifest) = self.manifest.lock().map(|m| m.clone()) else { return Vec::new() };
        let connection = &self.saved.connection;
        let mut changes: Vec<(String, Standing)> = local
            .list()
            .into_iter()
            .filter(|e| {
                !connection.is_excluded(&e.path) && !e.path.starts_with(kernel::brand::PROJECT_DIR)
            })
            .filter_map(|entry| {
                Some((entry.path.clone(), changed_here(&local, &manifest, &entry)?))
            })
            .collect();
        changes.sort_by(|a, b| a.0.cmp(&b.0));
        changes
    }

    /// The files here (each once) changed on disk at `from` or after, and before `to`
    /// (`None`: up to now) — but those never sent (excluded, the project's own data) —
    /// newest first, each with whether the server has it as it is. One walk of the local
    /// copy; reads the disk (a background thread's work).
    pub fn changed_between(&self, from: i64, to: Option<i64>) -> Vec<RecentFile> {
        let local = local_copy(&self.saved.local);
        let manifest = self.manifest.lock().map(|m| m.clone()).unwrap_or_default();
        let mut files: Vec<RecentFile> = local
            .list()
            .into_iter()
            .filter(|e| e.modified >= from && to.is_none_or(|to| e.modified < to))
            .filter(|e| self.travels(&e.path))
            .map(|e| RecentFile {
                sent: changed_here(&local, &manifest, &e).is_none(),
                path: e.path,
                modified: e.modified,
                size: e.size,
            })
            .collect();
        files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
        files
    }

    /// How file `relative` stands if the server hasn't it as it is here (changed here, or
    /// never sent); `None`: it has, or it isn't here.
    pub(crate) fn local_changes_of(&self, relative: &str) -> Option<Standing> {
        let local = local_copy(&self.saved.local);
        let entry = local.stat(relative)?;
        let manifest = self.manifest.lock().ok()?.clone();
        changed_here(&local, &manifest, &entry)
    }

    /// `paths` up now (the user asked: no waiting for quiet, nor for a batch of changes
    /// going on — a paste; the server checked first, as for any save).
    pub fn upload_now(&self, paths: Vec<String>) {
        self.uploads.push_forced(paths);
    }

    /// A conflict decided: `relative` transferred `action`'s way (ours up, or the
    /// server's down), no longer in conflict.
    pub(super) fn decide(
        self: &Arc<Self>,
        relative: &str,
        action: Action,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        match self.transfer(relative, action, session) {
            Ok(()) => {
                self.settled(relative);
                crate::outgoing::refresh(self);
            }
            Err(why) => self.failed(&why),
        }
    }

    /// `relative` is no longer in conflict (uploaded, or the server's taken).
    pub(super) fn settled(&self, relative: &str) {
        if let Ok(mut conflicts) = self.conflicts.lock() {
            conflicts.remove(relative);
        }
    }

    /// The server's copy of `relative` (`server`, as it is now) changed since its last
    /// transfer: it appeared there, or its size or date moved.
    pub(super) fn changed_there(&self, relative: &str, server: Option<&RemoteEntry>) -> bool {
        let known = self.manifest.lock().ok().and_then(|m| m.get(relative).copied());
        match (server, known) {
            (Some(s), Some(k)) => s.size != k.remote_size || s.modified != k.remote_modified,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    /// `paths` sent by the user: going now (their buttons' loaders turn).
    pub(super) fn begin_sending<'a>(&self, paths: impl Iterator<Item = &'a str>) {
        use std::sync::atomic::Ordering::Relaxed;
        let Ok(mut sending) = self.sending.lock() else { return };
        let now = self.sending_generation.fetch_add(1, Relaxed) + 1;
        for path in paths {
            sending.insert(path.to_owned(), now);
        }
        drop(sending);
        (self.wake)();
    }

    /// `paths` sent ended (up, failed, or waiting for the user): their loaders stop.
    pub(crate) fn end_sending<'a>(&self, paths: impl Iterator<Item = &'a str>) {
        self.end_sending_since(paths, u64::MAX);
    }

    /// `paths`' loaders stop if they were sent by `started` (the generation when the work
    /// ending began): one sent again since waits for its own.
    pub(crate) fn end_sending_since<'a>(&self, paths: impl Iterator<Item = &'a str>, started: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        let Ok(mut sending) = self.sending.lock() else { return };
        let before = sending.len();
        for path in paths {
            if sending.get(path).is_some_and(|&sent| sent <= started) {
                sending.remove(path);
            }
        }
        if sending.len() != before {
            drop(sending);
            self.sending_generation.fetch_add(1, Relaxed);
            (self.wake)();
        }
    }

    /// `relative`, sent by the user, hasn't ended yet.
    pub fn is_sending(&self, relative: &str) -> bool {
        self.sending.lock().is_ok_and(|s| s.contains_key(relative))
    }

    /// One of `paths`, sent by the user, hasn't ended yet (one look at what's going).
    pub fn sending_among<'a>(&self, mut paths: impl Iterator<Item = &'a str>) -> bool {
        self.sending.lock().is_ok_and(|s| !s.is_empty() && paths.any(|p| s.contains_key(p)))
    }

    /// Something sent by the user hasn't ended yet.
    pub fn sending_any(&self) -> bool {
        self.sending.lock().is_ok_and(|s| !s.is_empty())
    }

    /// How many times what's being sent changed (a view draws again when it moves).
    pub fn sending_generation(&self) -> u64 {
        self.sending_generation.load(std::sync::atomic::Ordering::Relaxed)
    }
}
