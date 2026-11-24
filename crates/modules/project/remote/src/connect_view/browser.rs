//! The server's folders browsed (step 2): the session opened, a folder listed and placed in
//! the tree, opened, chosen, its parts excluded; the download started; a failure told.

use super::*;

impl ConnectView {
    /// A session on its own thread, answering listings (reconnecting if it dropped): it
    /// holds the connection while the wizard browses.
    #[allow(clippy::disallowed_methods)]
    pub(super) fn open_browser(&mut self, connector: Arc<dyn Connector>) {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        let (replies, rx) = std::sync::mpsc::channel::<Reply>();
        let waker = self.waker.clone();
        let spawned = std::thread::Builder::new().name("remote-browse".into()).spawn(move || {
            let mut fs = None;
            for request in requests {
                let mut attempt = || {
                    if fs.is_none() {
                        fs = Some(connector.connect()?);
                    }
                    let Some(session) = fs.as_mut() else { return Err(RemoteError::TimedOut) };
                    match &request {
                        Request::Home => session.home().map(|h| Reply::Home(Ok(h))),
                        Request::List(dir) => {
                            let relative = dir.trim_start_matches('/').to_owned();
                            session.list(&relative).map(|l| Reply::Listed(dir.clone(), Ok(l)))
                        }
                        Request::Folders(dir) => {
                            let relative = dir.trim_start_matches('/').to_owned();
                            session.list(&relative).map(|l| Reply::Folders(dir.clone(), names(&l)))
                        }
                        Request::Accounts(dir) => {
                            Ok(Reply::Accounts(owner::read_accounts(session.as_mut(), dir)))
                        }
                    }
                };
                let reply = match attempt() {
                    Ok(reply) => reply,
                    Err(e) => {
                        if e.is_transient() {
                            fs = None;
                        }
                        match &request {
                            Request::Home => Reply::Home(Err(e)),
                            Request::List(dir) => Reply::Listed(dir.clone(), Err(e)),
                            Request::Folders(dir) => Reply::Folders(dir.clone(), Vec::new()),
                            Request::Accounts(_) => Reply::Accounts(ServerAccounts::default()),
                        }
                    }
                };
                if replies.send(reply).is_err() {
                    break;
                }
                if let Some(wake) = &waker {
                    wake();
                }
            }
        });
        if spawned.is_ok() {
            self.browser = Some((tx, rx));
        }
    }

    pub(super) fn ask(&mut self, request: Request) {
        if let Some((tx, _)) = &self.browser
            && tx.send(request).is_ok()
        {
            self.waiting += 1;
        }
    }

    /// Step 2: goes into `dir` (absolute).
    pub(super) fn go(&mut self, dir: String) {
        self.suggestions.clear();
        self.input_mut(Field::Path).set_text(&dir);
        self.current = dir.clone();
        self.listing.clear();
        self.on_list = true;
        self.highlight = None;
        self.scroll = 0.0;
        self.ask(Request::List(dir));
    }

    /// Step 2 → 3: the folder chosen; its tree listed.
    pub(super) fn choose(&mut self) {
        // Editing, the same folder again: its exclusions stay.
        let same = self.editing.is_some() && self.root == self.current;
        self.root = self.current.clone();
        self.step = Step::Contents;
        self.tree.clear();
        if !same {
            self.excluded.clear();
        }
        self.scroll = 0.0;
        self.focus(Field::Name);
        self.on_list = true;
        self.highlight = None;
        if !self.name_typed {
            let name = self.root.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned();
            self.input_mut(Field::Name).set_text(&name);
            if !self.label_typed {
                self.input_mut(Field::Label).set_text(&name);
            }
            if !self.local_typed {
                let local = default_local(&name);
                self.input_mut(Field::Local).set_text(&local);
            }
        }
        let root = self.root.clone();
        self.ask(Request::List(root));
        self.ask_accounts();
    }

    /// A tree path's absolute place on the server.
    pub(super) fn absolute(&self, relative: &str) -> String {
        if relative.is_empty() { self.root.clone() } else { join(&self.root, relative) }
    }

    /// `path` (relative) is left out: it or a folder above it is.
    pub(super) fn is_excluded(&self, path: &str) -> bool {
        self.excluded
            .iter()
            .any(|e| path == e || path.strip_prefix(e.as_str()).is_some_and(|r| r.starts_with('/')))
    }

    pub(super) fn toggle(&mut self, i: usize) {
        let Some(node) = self.tree.get(i) else { return };
        let path = node.path.clone();
        if !self.excluded.remove(&path) {
            // Left out by a folder above: that one decides.
            if self.is_excluded(&path) {
                return;
            }
            // What was excluded inside goes with it.
            self.excluded.retain(|e| !e.starts_with(&format!("{path}/")));
            self.excluded.insert(path);
        }
    }

    pub(super) fn expand(&mut self, i: usize) {
        let Some(node) = self.tree.get_mut(i) else { return };
        if !node.is_dir {
            return;
        }
        node.expanded = !node.expanded;
        let (path, expanded, loaded, depth) =
            (node.path.clone(), node.expanded, node.loaded, node.depth);
        if !expanded {
            // Its rows go (they come back when it opens again).
            let end = self.tree[i + 1..]
                .iter()
                .position(|n| n.depth <= depth)
                .map_or(self.tree.len(), |p| i + 1 + p);
            self.tree.drain(i + 1..end);
            self.tree[i].loaded = false;
        } else if !loaded {
            let dir = self.absolute(&path);
            self.ask(Request::List(dir));
        }
    }

    /// A listing for the tree: its rows under their folder.
    pub(super) fn place_listing(&mut self, dir: &str, entries: Vec<RemoteEntry>) {
        let relative =
            dir.strip_prefix(self.root.as_str()).unwrap_or("").trim_start_matches('/').to_owned();
        let (at, depth) = if relative.is_empty() {
            self.tree.clear();
            (0, 0)
        } else {
            let Some(i) = self.tree.iter().position(|n| n.path == relative) else { return };
            self.tree[i].loaded = true;
            (i + 1, self.tree[i].depth + 1)
        };
        let mut rows: Vec<Node> = entries
            .into_iter()
            .filter_map(|e| {
                let name = e.path.rsplit('/').next()?.to_owned();
                // The project's data and git's folder never travel: not shown.
                (name != ".git" && name != kernel::brand::PROJECT_DIR).then(|| Node {
                    path: if relative.is_empty() { name } else { format!("{relative}/{name}") },
                    is_dir: e.is_dir,
                    depth,
                    expanded: false,
                    loaded: false,
                })
            })
            .collect();
        // Folders first, then by name.
        rows.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
        });
        self.tree.splice(at..at, rows);
    }

    /// Downloads everything (but the exclusions) in parallel, saves the connection.
    pub(super) fn connect(&mut self) {
        let (saved, secret) = match self.saved() {
            Ok(s) => s,
            Err(invalid) => {
                self.reject(invalid);
                return;
            }
        };
        if self.downloading.is_some() {
            return;
        }
        let connector = (self.connectors)(&saved, secret.clone(), self.accepted.clone());
        // The browsing session closed first: one connection less for the server to count.
        self.browser = None;
        self.waiting = 0;
        // Its downloads share a pool: cautious for FTP (the server may take only a few).
        let limit = crate::settings::defaults().limits.downloads;
        let connector: Arc<dyn Connector> =
            Arc::new(crate::pool::Pooled(session::pool_for(&saved, connector, limit)));
        let progress = Arc::clone(&self.progress);
        progress.0.store(0, Ordering::Relaxed);
        progress.1.store(0, Ordering::Relaxed);
        self.cancel = Arc::default();
        let cancel = Arc::clone(&self.cancel);
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-download", move || {
            let wake = || {
                if let Some(w) = &waker {
                    w();
                }
            };
            // The Task Manager shows it while it runs.
            session::OPENING.0.fetch_add(1, Ordering::Relaxed);
            let outcome = download(&saved, secret, connector.as_ref(), &progress, &cancel, &wake);
            session::OPENING.0.fetch_sub(1, Ordering::Relaxed);
            let _ = tx.send(outcome);
            wake();
        });
        self.downloading = Some(rx);
        self.status = Some((t("remote.downloading"), Tone::Neutral));
    }

    pub(super) fn failed(&mut self, e: RemoteError) {
        self.testing = false;
        match e {
            // An unknown server: its fingerprint, to trust.
            RemoteError::HostKey(f) if !f.starts_with("changed") => {
                self.status = Some((t("remote.unknown_server"), Tone::Neutral));
                self.unknown = Some(f);
                self.browser = None;
            }
            other => self.status = Some((explain(&other), Tone::Danger)),
        }
    }

    /// Exclusions with their full paths on the server.
    pub(super) fn exclusions(&self) -> Vec<String> {
        self.excluded.iter().map(|e| self.absolute(e)).collect()
    }
}
