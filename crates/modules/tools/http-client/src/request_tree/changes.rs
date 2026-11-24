//! What the tree makes and changes: a collection named on its own row, a request made,
//! renamed, moved, copied or removed, written on a worker and the list read again at once.

use super::*;

/// A message for the user, as the tree's action.
fn notice(text: String) -> TreeAction {
    TreeAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
        host.notify(workbench_model::Notification::error(text));
    })))
}

/// What a change found its request changed (or gone) meanwhile answers: nothing written,
/// the user told.
fn changed_outside() -> workbench_model::Completion {
    Box::new(|host: &mut dyn WorkbenchHost| {
        host.notify(workbench_model::Notification::error(t("http.changed_outside")))
    })
}

impl RequestTree {
    /// Work for the host's worker: what it gives back runs on the UI thread (the list read
    /// again first).
    fn job(&mut self, work: impl FnOnce() -> workbench_model::Completion + Send + 'static) {
        let stale = Arc::clone(&self.stale);
        self.action = Some(TreeAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
            host.spawn(Box::new(move || {
                let then = work();
                stale.store(true, Ordering::Release);
                then
            }));
        }))));
    }

    /// A name to be typed for `naming` on its row, `text` first in it (selected).
    pub(super) fn start_naming(&mut self, naming: Naming, text: &str) {
        if let Naming::NewCollection { parent } = &naming {
            self.collapsed.remove(&collections::collapse_key(parent));
        }
        let mut input = TextInput::new(text);
        input.select_all();
        self.naming = Some((naming, input));
        self.naming_error = false;
        self.typing = false;
        self.filtered();
        if let Some(slot) = self.slots.iter().position(|s| matches!(s, Slot::Naming(_))) {
            let h = self.row_h();
            let top = slot as f32 * h;
            if top < self.scroll || top + h > self.scroll + self.list.size.height {
                self.scroll = (top - h).max(0.0);
            }
        }
    }

    /// A new collection to be named, in `parent` (`None`: where the tree is: its own
    /// folder, or the project's collections).
    pub fn new_collection(&mut self, parent: Option<String>) {
        let parent = parent.unwrap_or_else(|| {
            if self.within.is_empty() {
                collections::COLLECTIONS.to_owned()
            } else {
                self.within.clone()
            }
        });
        self.start_naming(Naming::NewCollection { parent }, "");
    }

    pub(super) fn cancel_naming(&mut self) {
        self.naming = None;
        self.lay_slots();
    }

    /// The name typed, done (Enter, or `settled`: the keyboard gone elsewhere). Not a name:
    /// asked again on Enter, given up when settled.
    pub(super) fn finish_naming(&mut self, settled: bool) {
        let Some((naming, input)) = self.naming.take() else { return };
        self.lay_slots();
        let typed = input.text().to_owned();
        let unchanged = matches!(&naming, Naming::Rename { path } if collections::base_name(path) == typed.trim());
        if typed.trim().is_empty() || unchanged {
            return;
        }
        let Some(name) = collections::valid_name(&typed) else {
            self.action = Some(notice(t("http.bad_name")));
            if !settled {
                self.naming = Some((naming, input));
                self.naming_error = true;
                self.lay_slots();
            }
            return;
        };
        match naming {
            Naming::NewCollection { parent } => self.make_folder(&parent, &name),
            Naming::Rename { path } => self.rename(&path, &name),
        }
    }

    /// Folder `name` made in `parent` (nothing else: no file, nothing opened).
    fn make_folder(&mut self, parent: &str, name: &str) {
        let Some(root) = self.root() else { return };
        let path = if parent.is_empty() { name.to_owned() } else { format!("{parent}/{name}") };
        let folder = root.join(&path);
        *self.made.lock().unwrap_or_else(PoisonError::into_inner) = Some(Made::Folder(path));
        self.job(move || {
            let made = if folder.exists() {
                Err(i18n::tr!("http.name_taken", name = folder.display().to_string().as_str()))
            } else {
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())
            };
            Box::new(move |host: &mut dyn WorkbenchHost| match made {
                Ok(()) => {
                    if let Some(dir) = folder.parent() {
                        host.relist(dir);
                    }
                }
                Err(why) => host.notify(workbench_model::Notification::error(why)),
            })
        });
    }

    /// A new request in folder `folder` (relative): at the end of its `requests.http`
    /// (made if it has none), then selected.
    pub fn new_request(&mut self, folder: Option<String>) {
        let Some(root) = self.root() else { return };
        let folder = folder.unwrap_or_else(|| self.within.clone());
        let relative = if folder.is_empty() {
            "requests.http".to_owned()
        } else {
            format!("{folder}/requests.http")
        };
        let file = root.join(&relative);
        let made = Arc::clone(&self.made);
        self.job(move || {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let mut out = text.trim_end().to_owned();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(TEMPLATE);
            let index = http_requests::parse(&out).requests.len().saturating_sub(1);
            let written = file
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| files::write_atomic(&file, out.as_bytes()))
                .map_err(|e| e.to_string());
            if written.is_ok() {
                *made.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(Made::Request(relative, index));
            }
            Box::new(move |host: &mut dyn WorkbenchHost| match written {
                Ok(()) => {
                    if let Some(dir) = file.parent() {
                        host.relist(dir);
                    }
                }
                Err(why) => host.notify(workbench_model::Notification::error(why)),
            })
        });
    }

    /// `path` (relative) renamed `name`, by the app (its tabs follow).
    pub(super) fn rename(&mut self, path: &str, name: &str) {
        let Some(root) = self.root() else { return };
        let from = root.join(path);
        let Some(to) = from.parent().map(|dir| dir.join(name)) else { return };
        let stale = Arc::clone(&self.stale);
        self.action = Some(TreeAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
            if let Err(why) = host.rename_entry(&from, &to) {
                host.notify(workbench_model::Notification::error(why));
            }
            stale.store(true, Ordering::Release);
        }))));
    }

    /// Request `request` of file `file` moved to the end of `into` (relative; on a worker).
    pub(super) fn move_request(&mut self, file: usize, request: usize, into: &str) {
        let (Some(root), Some(from)) = (self.root(), self.listing.files.get(file)) else {
            return;
        };
        let Some(hash) = from.requests.get(request).map(|r| r.hash) else { return };
        let (from, into) = (root.join(&from.path), root.join(into));
        if from == into {
            return;
        }
        self.job(move || {
            let source = std::fs::read_to_string(&from).unwrap_or_default();
            let target = std::fs::read_to_string(&into).unwrap_or_default();
            // Found by its text (the list may be older than the file); moved as it is.
            let moved = http_requests::draft::find_by_hash(&source, hash)
                .and_then(|at| http_requests::draft::move_raw(&source, at, &target));
            let Some((left, into_text)) = moved else { return changed_outside() };
            // Written where it goes first: never lost between the two.
            let made = into.parent().map_or(Ok(()), std::fs::create_dir_all);
            if made.is_ok() && files::write_atomic(&into, into_text.as_bytes()).is_ok() {
                let _ = files::write_atomic(&from, left.as_bytes());
            }
            Box::new(|_: &mut dyn WorkbenchHost| {})
        });
    }

    /// Request `request` of file `file` changed in it (on a worker): removed, or copied at
    /// its end.
    pub(super) fn change_request(&mut self, file: usize, request: usize, duplicate: bool) {
        let (Some(root), Some(listed)) = (self.root(), self.listing.files.get(file)) else {
            return;
        };
        let Some(hash) = listed.requests.get(request).map(|r| r.hash) else { return };
        let path = root.join(&listed.path);
        self.job(move || {
            let Ok(text) = std::fs::read_to_string(&path) else {
                return Box::new(|_: &mut dyn WorkbenchHost| {});
            };
            let Some(at) = http_requests::draft::find_by_hash(&text, hash) else {
                return changed_outside();
            };
            let changed = if duplicate {
                http_requests::draft::duplicate_raw(&text, at, &t("http.copy"))
            } else {
                Some(http_requests::draft::remove_from(&text, at))
            };
            if let Some(changed) = changed {
                let _ = files::write_atomic(&path, changed.as_bytes());
            }
            Box::new(|_: &mut dyn WorkbenchHost| {})
        });
    }

    /// The environments of folder `folder` (relative) in their tab.
    pub(super) fn edit_environments(&mut self, folder: &str) {
        let Some(root) = self.root() else { return };
        // A request file of the folder stands for it (its environments are the nearest).
        let file = root.join(folder).join("requests.http");
        let data = crate::env_view::EnvData { file, root: Some(root) };
        self.action = Some(TreeAction::View(ViewerAction::OpenView(ui_components::OpenView::new(
            t("http.edit_environments"),
            crate::env_view::EXTENSION,
            data,
        ))));
    }
}
