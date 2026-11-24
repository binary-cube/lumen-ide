//! The server's folders as the tree sees them: listed on demand, learned from a listing,
//! merged with the local copy, each entry's mark; failures told.

use super::*;

impl State {
    /// The folders holding `paths` (and those above them) listed again in the tree.
    pub(crate) fn relist_around<'a>(&self, paths: impl Iterator<Item = &'a str>) {
        let Ok(mut relisted) = self.relisted.lock() else { return };
        for path in paths {
            let mut dir = path;
            loop {
                dir = parent(dir);
                if !relisted.iter().any(|d| d == dir) {
                    relisted.push(dir.to_owned());
                }
                if dir.is_empty() {
                    break;
                }
            }
        }
        drop(relisted);
        (self.wake)();
    }

    /// A failure told with message `key` (its `{reason}`: `why`).
    pub(crate) fn failed_with(&self, key: &str, why: &str) {
        self.tell(Notice::Message(workbench_model::Notification::error(i18n::tr!(
            key,
            reason = why
        ))));
    }

    pub(crate) fn failed(&self, why: &str) {
        self.tell(Notice::Message(workbench_model::Notification::error(i18n::tr!(
            "remote.failed",
            reason = why
        ))));
    }

    /// `relative` can go to the server: not excluded, not the project's own data.
    pub(crate) fn travels(&self, relative: &str) -> bool {
        !self.saved.connection.is_excluded(relative)
            && relative.split('/').next() != Some(kernel::brand::PROJECT_DIR)
    }

    /// The project is worked on on the server (files brought when opened).
    pub fn on_demand(&self) -> bool {
        self.saved.connection.mode == remote::Mode::OnDemand
    }

    /// On demand: folder `dir` (relative; `""`: the root) and those under it are read from
    /// the server again (the tree shows them when they come).
    pub fn forget(&self, dir: &str) {
        if let Ok(mut listings) = self.listings.lock() {
            listings.retain(|known, _| !remote::application::in_scope(known, dir));
        }
        self.ask_list(dir);
    }

    /// On demand: asks for folder `dir` (relative) to be listed, once.
    fn ask_list(&self, dir: &str) {
        let first = self.asked.lock().is_ok_and(|mut asked| asked.insert(dir.to_owned()));
        if first {
            self.send_job(Job::List(dir.to_owned()));
        }
    }

    /// Lists folder `dir` of the server (tried again on a new session while the network
    /// fails); the tree then shows it.
    pub(super) fn list(&self, dir: &str, session: &mut Option<Box<dyn RemoteFs>>) {
        let mut listed = None;
        for attempt in 0..self.retry().attempts.max(1) {
            if attempt > 0 {
                std::thread::sleep(self.retry().wait_before(attempt));
            }
            if session.is_none() {
                *session = self.connector.connect().ok();
            }
            let Some(fs) = session.as_mut() else { continue };
            match fs.list(dir) {
                Ok(entries) => {
                    listed = Some(entries);
                    break;
                }
                Err(e) if e.is_transient() => *session = None,
                // A folder made here, not on the server yet: nothing there (no error).
                Err(remote::RemoteError::NotFound(_)) => {
                    listed = Some(Vec::new());
                    break;
                }
                Err(e) => {
                    let e = e.during(&format!("listing /{dir}"));
                    self.failed(&crate::connect_view::explain(&e));
                    break;
                }
            }
        }
        if let Ok(mut asked) = self.asked.lock() {
            asked.remove(dir);
        }
        let Some(entries) = listed else { return };
        self.learn_listing(dir, &entries);
    }

    /// On demand: folder `dir` of the server as just listed (by anyone: the tree, a review
    /// comparing) — the tree shows it (exclusions and the project's data left out).
    pub(crate) fn learn_listing(&self, dir: &str, entries: &[RemoteEntry]) {
        if !self.on_demand() {
            return;
        }
        let connection = &self.saved.connection;
        let entries = entries
            .iter()
            .filter(|e| !connection.is_excluded(&e.path) && e.path != kernel::brand::PROJECT_DIR)
            .cloned()
            .collect();
        if let Ok(mut listings) = self.listings.lock() {
            listings.insert(dir.to_owned(), entries);
        }
        if let Ok(mut relisted) = self.relisted.lock() {
            relisted.push(dir.to_owned());
        }
        (self.wake)();
    }

    /// What a failure adds when the server limited the connections: how many it takes.
    pub fn hint(&self) -> Option<String> {
        let pool = self.pool.as_ref().filter(|p| p.was_lowered())?;
        Some(i18n::tr!("remote.connections_lowered", count = pool.limit()))
    }

    /// What the tree shows of folder `dir` (relative; `path` on disk): on demand, the
    /// server's entries (those not on disk are absent, brought when opened) — asked for, and
    /// none meanwhile, if it wasn't listed yet; and each one on disk marked by where it
    /// stands against its last transfer (only here, changed here, changed there, both) — a
    /// folder by what's to upload under it (a conflict first).
    pub(super) fn known(&self, dir: &str, path: &Path) -> Vec<workspace::DirEntry> {
        use workspace::{DirEntry, Mark, NodeKind};
        let listed = if self.on_demand() {
            let listed = self.listings.lock().ok().and_then(|l| l.get(dir).cloned());
            if listed.is_none() {
                self.ask_list(dir);
            }
            listed
        } else {
            None
        };
        let server: Option<HashMap<&str, &RemoteEntry>> =
            listed.as_ref().map(|l| l.iter().map(|e| (e.path.as_str(), e)).collect());
        let kind = |is_dir| if is_dir { NodeKind::Directory } else { NodeKind::File };
        // The server's permissions on the rows (the setting on; asked for when not known).
        let notes = self.permission_notes(dir).unwrap_or_default();
        let note = |name: &str| notes.get(name).cloned();
        let mut entries: Vec<DirEntry> = listed
            .iter()
            .flatten()
            .filter_map(|e| {
                let name = e.path.rsplit('/').next()?;
                Some(DirEntry::new(name, kind(e.is_dir)).noted(note(name)))
            })
            .collect();
        let Ok(read) = std::fs::read_dir(path) else { return entries };
        let Ok(manifest) = self.manifest.lock() else { return entries };
        let local = local_copy(&self.saved.local);
        let mut by_name: HashMap<String, usize> =
            entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
            if relative == kernel::brand::PROJECT_DIR
                || self.saved.connection.is_excluded(&relative)
            {
                continue;
            }
            let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
            let mark = if meta.is_dir() {
                let there = match &server {
                    Some(server) => server.contains_key(relative.as_str()),
                    // Mirrored: a folder nothing was transferred from is only here.
                    None => self.on_demand() || manifest.has_under(&relative),
                };
                if there { self.folder_mark(&relative) } else { Mark::Local }
            } else {
                let Some(here) = local.stat(&relative) else { continue };
                let there = server.as_ref().map(|s| s.get(relative.as_str()).copied());
                let hash = |p: &str| {
                    (here.size <= HASHED_FOR_MARKS)
                        .then(|| local.read(p).ok().map(|b| remote::content_hash(&b)))
                        .flatten()
                };
                match standing(&here, &manifest, there.map(|r| r.filter(|r| !r.is_dir)), &hash) {
                    // Found changed on the server too when it was to go.
                    _ if self.conflicts.lock().is_ok_and(|c| c.contains(&relative)) => {
                        Mark::Conflict
                    }
                    Standing::Same => Mark::None,
                    Standing::OnlyHere => Mark::Local,
                    Standing::Ahead => Mark::Changed,
                    Standing::Behind => Mark::Behind,
                    Standing::Diverged => Mark::Conflict,
                }
            };
            let noted = note(&name);
            if mark == Mark::None && noted.is_none() {
                continue;
            }
            match by_name.get(&name) {
                Some(&i) => entries[i].mark = mark,
                None => {
                    by_name.insert(name.clone(), entries.len());
                    let entry = DirEntry::new(name, kind(meta.is_dir())).marked(mark);
                    entries.push(entry.noted(noted));
                }
            }
        }
        entries
    }

    /// A folder's mark by what's to upload under it (the list as last read): a conflict, a
    /// change, or nothing.
    pub(super) fn folder_mark(&self, relative: &str) -> workspace::Mark {
        let under = self.outgoing.under(relative);
        if under.iter().any(|(_, why)| matches!(why, crate::outgoing::Why::Conflict)) {
            workspace::Mark::Conflict
        } else if under.is_empty() {
            workspace::Mark::None
        } else {
            workspace::Mark::Changed
        }
    }
}
