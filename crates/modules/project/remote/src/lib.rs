//! Remote projects (SFTP, FTP, FTPS) as a module: "Open Remote Project…" (a wizard: the
//! connection, the folder on the server, what comes down — downloaded in parallel into a
//! local copy, which opens), a session following a remote project while it's open (a save
//! goes up after checking the server's copy — changed there: compared, not overwritten),
//! Remote › Pull / Push / Sync on the project, a folder or a file (reviewed first: what
//! changed where, conflicts decided and compared), and the Remote tool window with the
//! transfers' log (its button and menus only for a remote project).
//!
//! A local project costs nothing: the service attaches no session (no thread, no
//! connection) unless the project has a saved connection.

mod changed_view;
mod config;
mod connect_view;
mod local_time;
mod log_view;
mod outgoing;
mod paint;
mod pending_view;
pub mod pool;
mod properties_view;
mod review_view;
mod session;
pub mod settings;
mod transfers;
mod upload_bar;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec, MenuAt, MenuSectionSpec, Requires, SubmenuSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use remote::application::Direction;
use ui_components::{Dock, ToolContent, ToolWindow, ToolWindowProvider, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, MenuContribution, MenuPlace, MenuRow, ProjectService,
    WorkbenchHost,
};

#[cfg(feature = "snapshot")]
pub use changed_view::snapshot as snapshot_changed;
pub use connect_view::ConnectView;
pub use log_view::{LogData, TransferLog};
#[cfg(feature = "snapshot")]
pub use properties_view::snapshot as snapshot_properties;
pub use review_view::{ReviewData, TransferReview};
pub use session::RemoteService;

/// The module's id (the settings show its global options while it's on).
pub const MODULE_ID: &str = "remote.projects";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "Remote projects",
    description: "Projects on a server (SFTP, FTP, FTPS): a local copy kept in step, saves uploaded after checking the server, the transfers' log",
    depends_on: &[],
};

pub const OPEN: CommandId = workbench_model::OPEN_REMOTE_PROJECT;
pub const SHOW_LOG: CommandId = CommandId("remote.show");
/// The "To upload" tab: what the server hasn't as it is here.
pub const SHOW_PENDING: CommandId = CommandId("remote.pending");
/// On the right-clicked file / folder (or the active file).
pub const PULL: CommandId = CommandId("remote.pull");
pub const PUSH: CommandId = CommandId("remote.push");
pub const SYNC: CommandId = CommandId("remote.sync");
/// On the whole project (the tool strip's menu).
pub const PULL_PROJECT: CommandId = CommandId("remote.pull_project");
pub const PUSH_PROJECT: CommandId = CommandId("remote.push_project");
pub const SYNC_PROJECT: CommandId = CommandId("remote.sync_project");
/// On demand: everything under the folder brought here (reviewed first), and the folder
/// read from the server again.
pub const BRING: CommandId = CommandId("remote.bring");
pub const RELOAD: CommandId = CommandId("remote.reload");
/// On what's right-clicked (files, a selection, a folder): what of it is to upload, sent,
/// compared with the server (one file), or given up.
pub const UPLOAD_HERE: CommandId = CommandId("remote.upload_here");
pub const COMPARE_HERE: CommandId = CommandId("remote.compare_here");
pub const DISCARD_HERE: CommandId = CommandId("remote.discard_here");
/// The properties of what's right-clicked (or the active file) on the server: permissions,
/// owner, group, size.
pub const PROPERTIES: CommandId = CommandId("remote.properties");
/// The open project's connection, edited (its server, its sign-in, its folder, its options).
pub const CONNECTION: CommandId = CommandId("remote.connection");
/// The icon of the connection's settings.
pub const EDIT_ICON: &str = "gear";

/// Tools › Remote ›: the open remote project's whole-project transfers and its connection.
const TOOLS_SECTION: &str = "remote.tools";
const REMOTE_MENU: &str = "tools.remote";
const MENU_TRANSFERS: &str = "remote.menu.transfers";
const MENU_CONNECTION: &str = "remote.menu.connection";

/// The context menus' places (the editor, the tree's files and folders; tabs use the
/// editor's).
const HERE: &[MenuPlace] = &[MenuPlace::Editor, MenuPlace::TreeFile, MenuPlace::TreeFolder];
const STRIP: &[MenuPlace] = &[MenuPlace::ToolStrip];
/// Every place the Remote submenu is in (the tool window's menu and the context menus).
const EVERYWHERE: &[MenuPlace] =
    &[MenuPlace::ToolStrip, MenuPlace::Editor, MenuPlace::TreeFile, MenuPlace::TreeFolder];
/// The tool window (at the bottom).
pub const TOOL: &str = "remote";

#[derive(Debug, Default)]
pub struct RemoteModule;

impl Module for RemoteModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["remote-"]));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(connect_view::ConnectProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(log_view::LogProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(pending_view::PendingProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(changed_view::ChangedProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(review_view::ReviewProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(properties_view::PropertiesProvider));
        ctx.contribute::<Arc<dyn ui_components::editor_toolbar::EditorToolbarProvider>>(Arc::new(
            upload_bar::UploadBarProvider,
        ));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(RemoteService));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::RemoteSettings,
        ));
        ctx.contribute::<Arc<dyn workbench_model::ContextProvider>>(Arc::new(Excluded));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(
            session::RemoteResources,
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: OPEN,
                label: "Open Remote Project…",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::FILE_OPEN,
                    30,
                )),
                shortcuts: &[],
            },
            icon: Some("server"),
            handler: Arc::new(OpenRemote),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: SHOW_LOG, label: "Remote", menu: None, shortcuts: &[] },
            icon: Some("server"),
            handler: Arc::new(ShowLog),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: SHOW_PENDING, label: "To Upload", menu: None, shortcuts: &[] },
            icon: Some("upload"),
            handler: Arc::new(ShowPending),
        });
        let transfers = [
            (PULL, "Pull from Server", "download", Direction::Pull, None),
            (PUSH, "Push to Server", "upload", Direction::Push, None),
            (SYNC, "Sync with Server", "refresh", Direction::Sync, None),
            (PULL_PROJECT, "Pull the Project from Server", "download", Direction::Pull, Some(10)),
            (PUSH_PROJECT, "Push the Project to Server", "upload", Direction::Push, Some(20)),
            (SYNC_PROJECT, "Sync the Project with Server", "refresh", Direction::Sync, Some(30)),
        ];
        for (id, label, icon, direction, order) in transfers {
            // The whole project's: in Tools › Remote › too (greyed without a project).
            let menu = order.map(|o| MenuAt::new(MENU_TRANSFERS, o));
            let handler = Arc::new(Transfer { direction, project: order.is_some(), bring: false });
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu, shortcuts: &[] },
                icon: Some(icon),
                handler: workbench_model::Requiring::new(&[Requires::Project], handler),
            });
        }
        // The open project's connection: Tools › Remote ›, the tool window's menu, the top
        // bar's projects list (the project's kind) — greyed for a local project.
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: CONNECTION,
                label: "Connection Settings…",
                menu: Some(MenuAt::new(MENU_CONNECTION, 10)),
                shortcuts: &[],
            },
            icon: Some(EDIT_ICON),
            handler: workbench_model::Requiring::new(&[Requires::Project], Arc::new(Connection)),
        });
        ctx.contribute(MenuSectionSpec::new(TOOLS_SECTION, workbench_model::menus::bar::TOOLS, 40));
        ctx.contribute(SubmenuSpec {
            id: REMOTE_MENU,
            at: MenuAt::new(TOOLS_SECTION, 10),
            label_key: "menu.remote",
            label: "Remote",
            icon: Some("server"),
        });
        ctx.contribute(MenuSectionSpec::new(MENU_TRANSFERS, REMOTE_MENU, 10));
        ctx.contribute(MenuSectionSpec::new(MENU_CONNECTION, REMOTE_MENU, 20));
        let pending = [
            (UPLOAD_HERE, "Upload", "upload", Pending::Upload),
            (COMPARE_HERE, "Compare with the Server", "compare", Pending::Compare),
            (DISCARD_HERE, "Discard the Changes…", "undo", Pending::Discard),
        ];
        for (id, label, icon, what) in pending {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon: Some(icon),
                handler: Arc::new(what),
            });
        }
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: PROPERTIES, label: "Properties…", menu: None, shortcuts: &[]
            },
            icon: Some("info"),
            handler: Arc::new(ShowProperties),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: BRING, label: "Bring Here", menu: None, shortcuts: &[] },
            icon: Some("folder-input"),
            handler: Arc::new(Transfer { direction: Direction::Pull, project: false, bring: true }),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RELOAD,
                label: "Read Again from Server",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("refresh"),
            handler: Arc::new(Reload),
        });
        // Remote ›, only while a remote project is open (its rows enabled): in the context
        // menus on what was right-clicked, behind the tool strip's button on the project
        // (its first row, what's to upload, is what a click on the button does: what its
        // count counts).
        // Its window at the bottom: what's to upload, the transfers, the reviews (shown on a
        // remote project).
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: TOOL,
            icon: "server",
            title_key: "tooltip.remote",
            dock: Dock::Bottom,
            order: 20,
            strip_end: false,
            group: "",
            toggle: None,
            content: ToolContent::Views { command: SHOW_PENDING.0, new: None },
        }));
        ctx.contribute(MenuContribution {
            id: TOOL,
            label_key: "menu.remote",
            icon: "server",
            places: EVERYWHERE,
            rows: vec![
                MenuRow::Command(SHOW_PENDING, STRIP),
                MenuRow::Command(SHOW_LOG, STRIP),
                MenuRow::Separator,
                MenuRow::Command(UPLOAD_HERE, HERE),
                MenuRow::Command(COMPARE_HERE, HERE),
                MenuRow::Command(DISCARD_HERE, HERE),
                MenuRow::Separator,
                MenuRow::Command(PULL, HERE),
                MenuRow::Command(PUSH, HERE),
                MenuRow::Command(SYNC, HERE),
                MenuRow::Command(PULL_PROJECT, STRIP),
                MenuRow::Command(PUSH_PROJECT, STRIP),
                MenuRow::Command(SYNC_PROJECT, STRIP),
                MenuRow::Separator,
                MenuRow::Command(BRING, HERE),
                MenuRow::Command(RELOAD, HERE),
                MenuRow::Separator,
                MenuRow::Command(SHOW_LOG, HERE),
                MenuRow::Command(SHOW_PENDING, HERE),
                MenuRow::Separator,
                // The connection a right-click away too (from a file, a folder, the editor);
                // then what's here (settings first, properties after: the same everywhere).
                MenuRow::Command(CONNECTION, EVERYWHERE),
                MenuRow::Command(PROPERTIES, HERE),
            ],
        });
        Ok(())
    }
}

struct OpenRemote;

impl CommandHandler for OpenRemote {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.open_modal(&i18n::t("remote.connect_title"), connect_view::EXTENSION, &())
    }
}

/// The connection of the remote project at `root` (open here or not) edited in the wizard's
/// form, prefilled ([`connect_view`]'s edit mode). `false`: not a remote project.
pub fn edit_connection(host: &mut dyn WorkbenchHost, root: &std::path::Path) -> bool {
    let Some(saved) = config::find(root) else { return false };
    let data = connect_view::EditConnection { saved };
    host.open_modal(&i18n::t("remote.edit_title"), connect_view::EXTENSION, &data)
}

/// "Connection Settings…": the open project's, when it's a remote one.
struct Connection;

impl CommandHandler for Connection {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id()
            .is_some_and(|id| session::session(id).is_some() || session::is_waiting(id))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(root) = host.project_root() else { return false };
        edit_connection(host, &root)
    }
}

/// The Remote tool window's "To upload" tab.
struct ShowPending;

/// The Remote window's tabs, found by these (the same in every language).
pub const PENDING_KEY: &str = "remote.pending";
pub const LOG_KEY: &str = "remote.log";
pub const CHANGED_KEY: &str = "remote.changed";

/// The Remote window's fixed tabs: "To upload", the transfers — what moved since the
/// project opened is there even if the window wasn't (the session keeps it) — and the files
/// changed in a span of time.
const FIXED: &[workbench_model::FixedTab] = &[
    workbench_model::FixedTab { key: PENDING_KEY, show: show_pending },
    workbench_model::FixedTab { key: LOG_KEY, show: show_log },
    workbench_model::FixedTab { key: CHANGED_KEY, show: show_changed },
];

/// The "To upload" tab shown (made if it isn't open).
fn show_pending(host: &mut dyn WorkbenchHost) -> bool {
    let Some(project) = host.project_id() else { return false };
    let data = pending_view::PendingData { project, queries: host.queries() };
    let title = i18n::t("remote.pending_tab");
    host.show_in_panel_keyed(TOOL, PENDING_KEY, &title, pending_view::EXTENSION, &data)
}

/// The transfers' tab shown (made if it isn't open).
fn show_log(host: &mut dyn WorkbenchHost) -> bool {
    let Some(project) = host.project_id() else { return false };
    let data = LogData { project, events: host.events().clone() };
    let title = i18n::t("remote.log_tab");
    host.show_in_panel_keyed(TOOL, LOG_KEY, &title, log_view::EXTENSION, &data)
}

/// The tab of the files changed in a span of time shown (made if it isn't open).
fn show_changed(host: &mut dyn WorkbenchHost) -> bool {
    let Some(project) = host.project_id() else { return false };
    let data = changed_view::ChangedData {
        project,
        events: host.events().clone(),
        queries: host.queries(),
    };
    let title = i18n::t("remote.changed_tab");
    host.show_in_panel_keyed(TOOL, CHANGED_KEY, &title, changed_view::EXTENSION, &data)
}

impl CommandHandler for ShowPending {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id().is_some_and(|id| session::session(id).is_some())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        workbench_model::open_fixed_tabs(host, TOOL, FIXED);
        show_pending(host)
    }
}

struct ShowLog;

impl CommandHandler for ShowLog {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id().is_some_and(|id| session::session(id).is_some())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        workbench_model::open_fixed_tabs(host, TOOL, FIXED);
        show_log(host)
    }
}

/// A fact of the targets this module adds: never transferred (excluded by the connection,
/// or the project's own data).
pub const EXCLUDED: &str = "remote.excluded";

/// The targets a remote project never transfers, said to every module (its own commands
/// leave them out).
#[derive(Debug)]
struct Excluded;

impl workbench_model::ContextProvider for Excluded {
    fn add_facts(
        &self,
        project: Option<(kernel::ProjectId, &std::path::Path)>,
        context: &mut workbench_model::ActionContext,
    ) {
        let Some((id, root)) = project else { return };
        let Some(state) = session::session(id) else { return };
        for path in context.paths() {
            let Ok(relative) = path.strip_prefix(root) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            let own = relative.split('/').next() == Some(kernel::brand::PROJECT_DIR);
            if own || (!relative.is_empty() && state.saved.connection.is_excluded(&relative)) {
                context.add(&path, EXCLUDED);
            }
        }
    }
}

/// Pull / Push / Sync, reviewed in a tab: on the right-clicked file / folder (or the active
/// file), or on the whole project.
struct Transfer {
    direction: Direction,
    project: bool,
    /// On demand: everything under the scope comes (not only what was brought before).
    bring: bool,
}

impl Transfer {
    /// What it acts on, relative to the project's root (`""`: the whole project).
    fn scope(&self, host: &dyn WorkbenchHost, root: &std::path::Path) -> Option<String> {
        if self.project {
            return Some(String::new());
        }
        let context = host.context();
        // What never travels (excluded, the project's own data); what there's nothing of on
        // one side (only here: nothing to pull; only on the server: nothing to push).
        let nothing = match self.direction {
            Direction::Pull => workbench_model::context::facts::ONLY_HERE,
            Direction::Push => workbench_model::context::facts::ABSENT,
            Direction::Sync => EXCLUDED,
        };
        if context.any(EXCLUDED) || context.all(nothing) {
            return None;
        }
        let relative = context.path()?.strip_prefix(root).ok()?;
        let parts: Vec<String> =
            relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        Some(parts.join("/"))
    }
}

impl Transfer {
    /// The review keeps to what was brought: on demand, Pull / Sync of a folder or the
    /// project; "Bring here" takes everything, and so does one asked for a file (brought if
    /// it wasn't).
    fn fetched_only(&self, on_demand: bool, context: &workbench_model::ActionContext) -> bool {
        let a_file = !self.project && context.targets.first().is_some_and(|t| !t.is_dir);
        on_demand && !self.bring && !a_file
    }
}

impl CommandHandler for Transfer {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        let (Some(root), Some(id)) = (host.project_root(), host.project_id()) else {
            return false;
        };
        session::session(id).is_some_and(|s| !self.bring || s.on_demand())
            && self.scope(host, &root).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(root), Some(project)) = (host.project_root(), host.project_id()) else {
            return false;
        };
        let Some(scope) = self.scope(host, &root) else { return false };
        let Some(state) = session::session(project) else { return false };
        let what = if scope.is_empty() { i18n::t("remote.review_project") } else { scope.clone() };
        let key = match self.direction {
            _ if self.bring => "remote.review_bring",
            Direction::Pull => "remote.review_pull",
            Direction::Push => "remote.review_push",
            Direction::Sync => "remote.review_sync",
        };
        let title = format!("{} · {what}", i18n::t(key)).replace('/', "∕");
        let fetched_only = self.fetched_only(state.on_demand(), &host.context());
        let data = ReviewData { project, root, direction: self.direction, scope, fetched_only };
        host.open_view(&title, review_view::EXTENSION, &data)
    }
}

/// What's to upload of the right-clicked files / folders (or the active file): sent,
/// compared with the server (one file), or given up (asked first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Upload,
    Compare,
    Discard,
}

impl Pending {
    /// The rows to upload at or under what the action is on (each once).
    fn rows(host: &dyn WorkbenchHost) -> Vec<(String, outgoing::Why)> {
        let (Some(root), Some(state)) =
            (host.project_root(), host.project_id().and_then(session::session))
        else {
            return Vec::new();
        };
        let mut rows: Vec<(String, outgoing::Why)> = Vec::new();
        for path in host.context().paths() {
            let Ok(relative) = path.strip_prefix(&root) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            for row in state.outgoing.under(&relative) {
                if !rows.contains(&row) {
                    rows.push(row);
                }
            }
        }
        rows
    }
}

impl CommandHandler for Pending {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        let rows = Self::rows(host);
        match self {
            Self::Compare => rows.len() == 1 && workbench_model::presence::compare(),
            _ => !rows.is_empty(),
        }
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let rows = Self::rows(host);
        let Some(state) = host.project_id().and_then(session::session) else { return false };
        match self {
            Self::Upload => state.send(&rows),
            Self::Compare => {
                let [(here, why)] = rows.as_slice() else { return false };
                let there = match why {
                    outgoing::Why::Renamed { from } => from.as_str(),
                    _ => here.as_str(),
                };
                state.compare_with_server(here, there);
            }
            Self::Discard => state.discard(rows),
        }
        true
    }
}

/// "Properties…": of what a view of the module chose (its rows), else of what's
/// right-clicked (or the active file) — relative to the project's root.
struct ShowProperties;

impl ShowProperties {
    fn paths(host: &dyn WorkbenchHost) -> Vec<String> {
        let Some(root) = host.project_root() else { return Vec::new() };
        let mut paths: Vec<String> = Vec::new();
        for path in host.context().paths() {
            let Ok(relative) = path.strip_prefix(&root) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if !paths.contains(&relative) {
                paths.push(relative);
            }
        }
        paths
    }
}

impl CommandHandler for ShowProperties {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id().and_then(session::session).is_some() && !Self::paths(host).is_empty()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(project) = host.project_id() else { return false };
        if session::session(project).is_none() {
            return false;
        }
        let paths = Self::paths(host);
        !paths.is_empty() && properties_view::open(host, project, paths)
    }
}

/// On demand: the right-clicked folder (a file's folder) read from the server again.
struct Reload;

impl Reload {
    fn folder(host: &dyn WorkbenchHost) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let root = host.project_root()?;
        let target = host.target_path()?;
        let dir = if target.is_file() { target.parent()?.to_path_buf() } else { target };
        dir.starts_with(&root).then_some((root, dir))
    }
}

impl CommandHandler for Reload {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        Self::folder(host).is_some()
            && host.project_id().and_then(session::session).is_some_and(|s| s.on_demand())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some((root, dir)) = Self::folder(host) else { return false };
        let Some(state) = host.project_id().and_then(session::session) else { return false };
        let relative = dir.strip_prefix(&root).map(|r| r.to_string_lossy().replace('\\', "/"));
        state.forget(relative.as_deref().unwrap_or(""));
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
