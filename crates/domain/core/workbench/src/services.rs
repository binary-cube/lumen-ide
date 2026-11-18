//! Project services: a module's part that follows the open project — its events (saves,
//! renames, deletions) — with a folder of its own for the project's data, outside the
//! project (nothing to commit or ignore). Attached when a project opens; its subscriptions
//! (and whatever they hold: threads, caches) end when the project closes.

use std::path::Path;
use std::sync::Arc;

use kernel::{EventBus, Subscription};

/// What a project service is attached with.
pub struct ProjectContext<'a> {
    /// The project's identity (new at each opening): what a module keeps per project, it
    /// keeps by this.
    pub id: kernel::ProjectId,
    /// The project's root (where it is on disk).
    pub root: &'a Path,
    /// The project's data folder (created when written to).
    pub storage: &'a Path,
    pub events: &'a EventBus,
    /// What the user marked in the project.
    pub marks: workspace::marks::ProjectMarks,
    /// Wakes the app from a background thread: it then calls [`ProjectService::poll`].
    pub wake: Arc<dyn Fn() + Send + Sync>,
    /// The project's own settings (`<project dir>/project.toml`).
    pub settings: Option<Arc<settings_model::SettingsService>>,
    /// A file's language (its id: `php`, `javascript`…), as the languages' modules say.
    pub language_of: LanguageOf,
    /// The formatters and checkers the languages' modules contributed.
    pub quality_tools: Vec<Arc<dyn code_quality::QualityTool>>,
    /// The formatters the languages' modules built in (laying the editor's text out).
    pub text_formatters: Vec<Arc<dyn code_quality::TextFormatter>>,
    /// Tells the user something (a toast of this project's), from any thread.
    pub notifier: Arc<dyn crate::Notifier>,
}

impl std::fmt::Debug for ProjectContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectContext").field("root", &self.root).finish_non_exhaustive()
    }
}

/// A file's language id (`php`, `javascript`…), as the languages' modules say; `None`: none.
pub type LanguageOf = Arc<dyn Fn(&Path) -> Option<String> + Send + Sync>;

/// What a module says a project folder is (a remote project's local copy), shown in the
/// projects list. A folder no module describes is a local project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKind {
    /// The icon's name (e.g. `refresh` for a copy kept in sync).
    pub icon: &'static str,
    /// What it is, in the user's language (e.g. "SFTP · synced").
    pub label: String,
    /// Where it comes from (e.g. `demo@example.com:/var/www/shop`).
    pub detail: Option<String>,
    /// What of it the user can change (a remote project's connection): offered for the
    /// current project in the top bar's projects list and in a project row's menu, run by
    /// [`ProjectService::edit`].
    pub edit: Option<ProjectEdit>,
}

/// A project kind's settings, as the projects list offers them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEdit {
    /// The row for the current project (in the user's language: "Remote Connection…").
    pub label: String,
    /// The row in a project's right-click menu ("Edit Connection…").
    pub menu_label: String,
    pub icon: &'static str,
}

pub trait ProjectService: Send + Sync + std::fmt::Debug {
    /// Starts following the project. The subscriptions returned live as long as the project.
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription>;

    /// A window opened, a project in it or not (the start screen): `wake` wakes it, its
    /// [`Self::poll`] following — for what a module does without any project (the app's
    /// updates). The subscriptions returned live as long as the window.
    fn window_opened(&self, _wake: &std::sync::Arc<dyn Fn() + Send + Sync>) -> Vec<Subscription> {
        Vec::new()
    }

    /// Back on the UI thread after a wake: what the background work found reaches the user
    /// (a message, a comparison). `true` if something changed (a redraw).
    fn poll(&self, _host: &mut dyn crate::WorkbenchHost) -> bool {
        false
    }

    /// Its background work for project `id` still going on (what the user would see next:
    /// a count being read). Snapshots wait for it, so what they show doesn't depend on time.
    fn busy(&self, _id: kernel::ProjectId) -> bool {
        false
    }

    /// What the folder `root` is, if this module knows (read when the projects list is
    /// built, never per frame): `None` for the others.
    fn describe(&self, _root: &Path) -> Option<ProjectKind> {
        None
    }

    /// Opens what the user changes of the project at `root` (its kind's
    /// [`ProjectKind::edit`]), open here or not. `false`: nothing of this module's.
    fn edit(&self, _host: &mut dyn crate::WorkbenchHost, _root: &Path) -> bool {
        false
    }

    /// What folder `dir` holds beyond what's on disk (a remote project's files not fetched
    /// yet), listed in the tree as absent: `None` if the module knows nothing of it, `Some`
    /// (maybe empty, its contents still coming) if it does. Answered from what it knows —
    /// never waiting on the network; when it learns more, it wakes the app and asks for the
    /// folder to be listed again ([`crate::WorkbenchHost::relist`]).
    fn listed(&self, _dir: &Path) -> Option<Vec<workspace::DirEntry>> {
        None
    }

    /// The folders of the project's own folder (`kernel::brand::PROJECT_DIR`) holding the
    /// user's files (the query consoles' `.sql`): the disk's watcher hears them as the
    /// project's (another program renaming one is followed); the rest of that folder is the
    /// app's own, never heard.
    fn own_dirs(&self) -> &'static [&'static str] {
        &[]
    }

    /// `path` isn't on disk: the module brings it there, in the background — `Some` (the
    /// outcome comes back on the receiver: `Err` with why, for the user) if it does.
    fn fetch(&self, _path: &Path) -> Option<std::sync::mpsc::Receiver<Result<(), String>>> {
        None
    }
}
