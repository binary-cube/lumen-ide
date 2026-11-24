//! The folders suggested while a path is typed: the local ones (read on a worker at each
//! key), the server's (from the browsing session), the list's keys and a suggestion taken.

use super::*;

/// Folders completing `typed` (a path being typed; `~` is the home folder): those in its
/// folder whose names start with what follows the last separator (hidden ones only when
/// asked for), sorted, at most 8 — none once it names one exactly.
pub(super) fn folder_suggestions(typed: &str) -> Vec<String> {
    const MAX: usize = 8;
    let sep = std::path::MAIN_SEPARATOR;
    let expanded = match typed.strip_prefix('~') {
        Some(rest) => match dirs::home_dir() {
            Some(home) => format!("{}{rest}", home.display()),
            None => return Vec::new(),
        },
        None => typed.to_owned(),
    };
    if expanded.is_empty() {
        return Vec::new();
    }
    let (dir, prefix) = match expanded.rfind(['/', sep]) {
        Some(at) => (&expanded[..=at], &expanded[at + 1..]),
        None => return Vec::new(),
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let lower = prefix.to_lowercase();
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_lowercase().starts_with(&lower))
        .filter(|name| !name.starts_with('.') || prefix.starts_with('.'))
        .filter(|name| name != prefix)
        .map(|name| format!("{dir}{name}"))
        .collect();
    found.sort_by_key(|p| p.to_lowercase());
    found.truncate(MAX);
    found
}

/// A server path being typed (relative ones from `current`) split into the folder it's in
/// and the start of the name typed after it; nothing for an empty one.
pub(super) fn typed_server_path(typed: &str, current: &str) -> Option<(String, String)> {
    if typed.is_empty() {
        return None;
    }
    let full = if typed.starts_with('/') { typed.to_owned() } else { join(current, typed) };
    let (dir, prefix) = full.rsplit_once('/')?;
    Some((folder_key(dir), prefix.to_owned()))
}

/// The folders of `dir` (`names`) completing `prefix`, like the local copy's: those starting
/// with it (any case; hidden ones only when asked for), sorted, at most 8, as full paths —
/// none once it names one exactly.
pub(super) fn server_suggestions(dir: &str, prefix: &str, names: &[String]) -> Vec<String> {
    const MAX: usize = 8;
    let lower = prefix.to_lowercase();
    let mut found: Vec<String> = names
        .iter()
        .filter(|name| name.to_lowercase().starts_with(&lower))
        .filter(|name| !name.starts_with('.') || prefix.starts_with('.'))
        .filter(|name| name.as_str() != prefix)
        .map(|name| join(dir, name))
        .collect();
    found.sort_by_key(|p| p.to_lowercase());
    found.truncate(MAX);
    found
}

impl ConnectView {
    /// The folders that complete the local copy's typed path (read from the one it's in:
    /// a single folder, on a key).
    /// The local folders completing what's typed: read on a worker (a key never reads the
    /// disk), shown when they come if it's still typed (the list kept until then).
    pub(super) fn suggest(&mut self) {
        self.suggested = None;
        let typed = self.value(Field::Local);
        let (tx, rx) = std::sync::mpsc::channel();
        let (asked, wake) = (typed.clone(), self.waker.clone());
        kernel::work::run("remote-suggest", move || {
            let _ = tx.send(folder_suggestions(&asked));
            if let Some(wake) = wake {
                wake();
            }
        });
        self.local_folders = Some((typed, rx));
    }

    /// The local folders read meanwhile, shown if their text is still typed. `true`: shown.
    pub(super) fn local_folders_read(&mut self) -> bool {
        let Some((typed, rx)) = &self.local_folders else { return false };
        let found = match rx.try_recv() {
            Ok(found) => found,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        // Still typed, still in that field (not another step's, the list not closed).
        let current = *typed == self.value(Field::Local)
            && self.step == Step::Contents
            && self.focused() == Field::Local;
        self.local_folders = None;
        if !current {
            return false;
        }
        self.suggestions = found;
        self.suggested = None;
        true
    }

    /// The server's folders that complete step 2's typed path: from the folders already
    /// read (the one shown counts), else asked of the browsing session once (in the
    /// background: they show when it answers, if the path still wants them).
    pub(super) fn suggest_server(&mut self) {
        self.suggested = None;
        let Some((dir, prefix)) = typed_server_path(&self.value(Field::Path), &self.current) else {
            self.suggestions.clear();
            return;
        };
        match self.server_folders.get(&dir) {
            Some(Some(names)) => self.suggestions = server_suggestions(&dir, &prefix, names),
            Some(None) => self.suggestions.clear(),
            None => {
                self.suggestions.clear();
                if let Some((tx, _)) = &self.browser
                    && tx.send(Request::Folders(dir.clone())).is_ok()
                {
                    self.server_folders.insert(dir, None);
                }
            }
        }
    }

    /// The server's folders of `dir`, kept; suggested at once if the path is still in it.
    pub(super) fn server_folders_read(&mut self, dir: String, names: Vec<String>) {
        self.server_folders.insert(folder_key(&dir), Some(names));
        if self.step == Step::Folder && self.focused() == Field::Path && !self.on_list {
            self.suggest_server();
        }
    }

    /// The focused path completed with suggestion `i` (its folders listed next).
    pub(super) fn complete(&mut self, i: usize) {
        let Some(path) = self.suggestions.get(i).cloned() else { return };
        if self.focused() == Field::Path {
            self.input_mut(Field::Path).set_text(&format!("{path}/"));
            self.input_mut(Field::Path).end(false);
            self.suggest_server();
            return;
        }
        let path = format!("{path}{}", std::path::MAIN_SEPARATOR);
        self.input_mut(Field::Local).set_text(&path);
        self.input_mut(Field::Local).end(false);
        self.local_typed = true;
        self.suggest();
    }

    /// The keys of the listing (step 2) and of the tree (step 3).
    pub(super) fn list_key(&mut self, key: ViewerKey) -> bool {
        let len = if self.step == Step::Folder { self.listing.len() } else { self.tree.len() };
        match key {
            ViewerKey::Down if len > 0 => {
                self.highlight = Some(self.highlight.map_or(0, |h| (h + 1).min(len - 1)));
            }
            ViewerKey::Up => self.highlight = self.highlight.map(|h| h.saturating_sub(1)),
            ViewerKey::Tab => {
                self.on_list = false;
                self.focus(if self.step == Step::Folder { Field::Path } else { Field::Name });
            }
            ViewerKey::Right | ViewerKey::Enter if self.step == Step::Folder => {
                match self.highlight.and_then(|h| self.listing.get(h)).filter(|e| e.is_dir) {
                    Some(entry) => {
                        let name = entry.path.rsplit('/').next().unwrap_or("").to_owned();
                        let dir = join(&self.current, &name);
                        self.go(dir);
                    }
                    None if key == ViewerKey::Enter => self.choose(),
                    None => {}
                }
            }
            ViewerKey::Left | ViewerKey::Backspace if self.step == Step::Folder => {
                let up = parent(&self.current);
                self.go(up);
            }
            ViewerKey::Right | ViewerKey::Left => {
                let Some(h) = self.highlight.filter(|h| *h < len) else { return true };
                let node = &self.tree[h];
                if node.is_dir && node.expanded != (key == ViewerKey::Right) {
                    self.expand(h);
                } else if key == ViewerKey::Left {
                    // On its folder.
                    let depth = node.depth;
                    self.highlight =
                        self.tree[..h].iter().rposition(|n| n.depth < depth).or(Some(h));
                }
            }
            ViewerKey::Enter if self.editing.is_some() => self.save(),
            ViewerKey::Enter => self.connect(),
            ViewerKey::Escape if self.busy() => self.stop(),
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            _ => return false,
        }
        true
    }
}
