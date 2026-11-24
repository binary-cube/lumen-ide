//! What the user sends from the outgoing list: checked then sent, compared with the
//! server's copy, discarded (to the trash) or excluded; a file fetched from the server.

use super::*;

/// What of `rows` goes when sent (files and moves; a deletion is asked about first).
fn sent_paths(rows: &[(String, crate::outgoing::Why)]) -> impl Iterator<Item = &str> {
    rows.iter().filter(|(_, why)| *why != crate::outgoing::Why::Deleted).map(|(p, _)| p.as_str())
}

/// `path` to the system's trash (the tests' scratch files: removed, never in the user's
/// trash). `Err`: why it couldn't go.
fn to_trash(path: &Path) -> Result<(), String> {
    if cfg!(test) {
        return std::fs::remove_file(path).map_err(|e| e.to_string());
    }
    workspace::entries::trash(&fs_local::entries::LocalEntries, &[path.to_path_buf()])
}

impl State {
    /// The listed `rows` sent (the user asked). One alone goes now (in conflict: the user
    /// is asked what to do with it). Several: the server is checked for all of them first —
    /// nothing goes before; files changed there too are marked, and the user chooses to
    /// resolve them first or to upload the others now.
    ///
    /// The check runs on a worker of its own (never behind the session's other jobs: a
    /// listing, a comparison), the user told at once that it started.
    pub fn send(self: &Arc<Self>, rows: &[(String, crate::outgoing::Why)]) {
        if rows.len() > 1 {
            // Going from the check on (its loaders turn while it runs).
            self.begin_sending(sent_paths(rows));
            self.tell(Notice::Message(workbench_model::Notification::info(i18n::tr!(
                "remote.checking",
                count = rows.len()
            ))));
            let (state, rows) = (Arc::clone(self), rows.to_vec());
            kernel::work::run("remote-check", move || {
                let mut session = None;
                state.check_then_send(rows, &mut session);
                (state.wake)();
            });
        } else {
            self.send_now(rows);
        }
    }

    /// The rows several to send, checked first (see [`Self::send`]): none in conflict, they
    /// go; else the user decides. Out of reach, they go (and wait, as any upload).
    fn check_then_send(
        &self,
        rows: Vec<(String, crate::outgoing::Why)>,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        use crate::outgoing::Why;
        let mut conflicts = Vec::new();
        for (path, why) in &rows {
            if matches!(why, Why::Renamed { .. } | Why::Deleted) {
                continue;
            }
            if session.is_none() {
                *session = self.connector.connect().ok();
            }
            let Some(fs) = session.as_mut() else { break };
            match fs.stat(path) {
                Ok(server) if self.changed_there(path, server.as_ref()) => {
                    conflicts.push(path.clone());
                }
                Ok(_) => {}
                Err(e) if e.is_transient() => *session = None,
                Err(_) => {}
            }
        }
        // Back to the pool before the uploads take theirs.
        *session = None;
        if conflicts.is_empty() {
            self.send_now(&rows);
            return;
        }
        if let Ok(mut known) = self.conflicts.lock() {
            known.extend(conflicts.iter().cloned());
        }
        self.relist_around(conflicts.iter().map(String::as_str));
        // The user decides now: nothing goes until then.
        self.end_sending(sent_paths(&rows));
        let rest = rows.into_iter().filter(|(p, _)| !conflicts.contains(p)).collect();
        self.tell(Notice::BulkConflicts { conflicts, rest });
    }

    /// `rows` sent now: files uploaded (the server checked first, as for any save), moves
    /// made on the server (their content following if it changed), deletions asked about
    /// first (all in one question).
    pub(crate) fn send_now(&self, rows: &[(String, crate::outgoing::Why)]) {
        use crate::outgoing::Why;
        self.begin_sending(sent_paths(rows));
        let mut files = Vec::new();
        let mut deleted = Vec::new();
        for (path, why) in rows {
            match why {
                Why::Renamed { from } => self.send_job(Job::SendMove(path.clone(), from.clone())),
                Why::Deleted => deleted.push(path.clone()),
                Why::Waiting(_) | Why::Changed | Why::New | Why::Conflict => {
                    files.push(path.clone());
                }
            }
        }
        if !files.is_empty() {
            self.upload_now(files);
        }
        if !deleted.is_empty() {
            self.tell(Notice::ConfirmRemove(deleted));
        }
    }

    /// File `here` (its name on the server: `there`) compared with the server's copy, read
    /// in the background (the comparison opens when it's here).
    pub fn compare_with_server(&self, here: &str, there: &str) {
        self.send_job(Job::Compare(here.to_owned(), there.to_owned()));
    }

    /// The rows' changes here given up — the user is asked first.
    pub fn discard(&self, rows: Vec<(String, crate::outgoing::Why)>) {
        if !rows.is_empty() {
            self.tell(Notice::ConfirmDiscard(rows));
        }
    }

    /// `paths` (files, folders, `*.ext`) never uploaded any more: added to the connection's
    /// exclusions (the session starts again with them).
    pub fn exclude(&self, paths: Vec<String>) {
        if !paths.is_empty() {
            self.tell(Notice::Exclude(paths));
        }
    }

    /// Reads both sides of `here` (`there` on the server) and has them compared.
    pub(super) fn compare(&self, here: &str, there: &str, session: &mut Option<Box<dyn RemoteFs>>) {
        if session.is_none() {
            *session = self.connector.connect().ok();
        }
        let Some(fs) = session.as_mut() else {
            self.tell(Notice::Message(workbench_model::Notification::warning(i18n::t(
                "remote.offline",
            ))));
            return;
        };
        let server = match fs.read(there) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(remote::RemoteError::NotFound(_)) => String::new(),
            Err(e) => {
                if e.is_transient() {
                    *session = None;
                }
                self.failed(&crate::connect_view::explain(&e));
                return;
            }
        };
        let local = local_copy(&self.saved.local)
            .read(here)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        self.tell(Notice::Compare {
            path: self.saved.local.join(here),
            relative: here.to_owned(),
            local,
            server,
        });
    }

    /// The rows' changes here given up (confirmed): a file changed here takes the server's
    /// copy back, one made here goes to the trash, a deleted one comes back, a moved one
    /// comes back where it was (the moved copy to the trash).
    pub(super) fn discard_now(
        self: &Arc<Self>,
        rows: &[(String, crate::outgoing::Why)],
        session: &mut Option<Box<dyn RemoteFs>>,
    ) {
        use crate::outgoing::Why;
        for (path, why) in rows {
            let done = match why {
                Why::New => to_trash(&self.saved.local.join(path)),
                Why::Renamed { from } => self
                    .transfer(from, Action::Download, session)
                    .and_then(|()| to_trash(&self.saved.local.join(path)))
                    .map(|()| self.change_outbox(|o| o.rename_done(path))),
                Why::Deleted => self.transfer(path, Action::Download, session).map(|()| {
                    self.change_outbox(|o| {
                        o.revived(path);
                    });
                }),
                Why::Waiting(_) | Why::Changed | Why::Conflict => {
                    self.transfer(path, Action::Download, session)
                }
            };
            match done {
                Ok(()) => {
                    self.settled(path);
                    self.forget_pending(path);
                }
                Err(why) => self.failed(&why),
            }
        }
        self.relist_around(rows.iter().map(|(p, _)| p.as_str()));
        crate::outgoing::refresh(self);
    }

    /// Brings `relative` (on demand: a file opened); `done` told when it's here (asked again
    /// meanwhile: brought once, both told).
    pub(crate) fn fetch(&self, relative: String, done: Sender<Result<(), String>>) {
        let first = self.fetching.lock().is_ok_and(|mut waiting| {
            let waiters = waiting.entry(relative.clone()).or_default();
            waiters.push(done);
            waiters.len() == 1
        });
        if first {
            // Someone waits for it: no waiting for quiet.
            self.downloads.push_urgent([relative]);
        }
    }

    /// A file brought (or why not): whoever waited is told.
    pub(crate) fn fetched(&self, relative: &str, outcome: Result<(), String>) {
        let waiting = self.fetching.lock().ok().and_then(|mut w| w.remove(relative));
        for done in waiting.into_iter().flatten() {
            let _ = done.send(outcome.clone());
        }
        (self.wake)();
    }
}
