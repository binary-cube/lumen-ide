//! The tree's view of the disk: each folder as it is on disk, plus what modules know is
//! there without being on disk yet (a remote project's files not fetched), shown absent;
//! and opening one of those: the module brings it (in the background), then it opens.

use std::sync::mpsc::Receiver;

use super::*;
use workspace::FileSystem;

/// The disk, with what the project services list.
pub(super) struct ListedFs<'a> {
    local: &'a dyn FileSystem,
    services: &'a [Arc<dyn workbench_model::ProjectService>],
}

impl<'a> ListedFs<'a> {
    pub(super) fn new(
        local: &'a dyn FileSystem,
        services: &'a [Arc<dyn workbench_model::ProjectService>],
    ) -> Self {
        Self { local, services }
    }
}

impl FileSystem for ListedFs<'_> {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<workspace::DirEntry>> {
        let lists: Vec<_> = self.services.iter().filter_map(|s| s.listed(path)).collect();
        let known = (!lists.is_empty()).then(|| lists.into_iter().flatten().collect::<Vec<_>>());
        let on_disk = match self.local.read_dir(path) {
            Ok(entries) => Some(entries),
            // A folder known only (not brought yet): what's known.
            Err(e) if e.kind() == io::ErrorKind::NotFound && known.is_some() => None,
            Err(e) => return Err(e),
        };
        workspace::merge_listing(on_disk, known).ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.local.read_file(path)
    }

    /// On disk only (a file known, not brought yet, isn't there).
    fn kind(&self, path: &Path) -> Option<workspace::NodeKind> {
        self.local.kind(path)
    }

    fn stamp(&self, path: &Path) -> Option<workspace::Stamp> {
        self.local.stamp(path)
    }

    fn canonical(&self, path: &Path) -> io::Result<PathBuf> {
        self.local.canonical(path)
    }
}

/// A file being brought to disk to be opened.
pub(super) struct Fetching {
    path: PathBuf,
    done: Receiver<Result<(), String>>,
    /// Its notification (work in progress until it's here).
    notice: workbench_model::NotificationId,
}

impl std::fmt::Debug for Fetching {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fetching").field("path", &self.path).finish_non_exhaustive()
    }
}

impl Workbench {
    /// `path` isn't on disk: a module that knows it brings it, and it opens then. `false`
    /// if none does.
    pub(super) fn fetch_to_open(&mut self, path: &Path) -> bool {
        if self.fetching.iter().any(|f| f.path == path) {
            return true;
        }
        let support = Arc::clone(&self.support);
        let Some(done) = support.project_services.iter().find_map(|s| s.fetch(path)) else {
            return false;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let notice =
            workbench_model::Notification::progress(i18n::tr!("files.fetching", name = name));
        let id = notice.id;
        workbench_model::WorkbenchHost::notify(self, notice);
        self.fetching.push(Fetching { path: path.to_path_buf(), done, notice: id });
        true
    }

    /// The files brought meanwhile open (their folder listed again). `true`: a redraw.
    pub(super) fn poll_fetching(&mut self) -> bool {
        let mut finished = Vec::new();
        self.fetching.retain(|f| match f.done.try_recv() {
            Ok(outcome) => {
                finished.push((f.path.clone(), outcome, f.notice));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        let changed = !finished.is_empty();
        for (path, outcome, notice) in finished {
            if let Some(dir) = path.parent() {
                self.relist_dir(dir);
            }
            let result = match outcome {
                Ok(()) => self.open_file(&path).map_err(|e| e.to_string()),
                Err(why) => Err(why),
            };
            match result {
                // Opened: what it was waiting for shows.
                Ok(()) => {
                    self.dismiss_notification(notice);
                }
                Err(why) => {
                    let name = path.display().to_string();
                    let message = i18n::tr!("files.fetch_failed", name = name, reason = why);
                    self.update_notification(
                        workbench_model::Notification::error(message).with_id(notice),
                    );
                }
            }
        }
        changed
    }

    /// Lists folder `dir` again in the tree (if it was read).
    pub(super) fn relist_dir(&mut self, dir: &Path) {
        // Announced: whoever shows the entries lists the folder again too.
        let event = workspace::events::EntriesChanged { paths: vec![dir.to_path_buf()] };
        self.data.events.publish(&event);
        self.relist_own(dir);
    }

    /// Lists folder `dir` again in the app's own tree only (if it was read), unannounced.
    pub(super) fn relist_own(&mut self, dir: &Path) {
        let fs = ListedFs::new(&*self.ports.disk, &self.support.project_services);
        let Some(tree) = self.project.as_mut() else { return };
        let Some(node) = tree.find(dir) else { return };
        if !tree.is_loaded(node) {
            return;
        }
        match fs.read_dir(dir) {
            Ok(entries) => tree.set_children(node, entries),
            Err(e) => {
                tracing::debug!(dir = %dir.display(), "the folder can't be listed again: {e}")
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/listing.rs"]
mod tests;
