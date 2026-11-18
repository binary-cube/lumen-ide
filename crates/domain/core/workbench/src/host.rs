//! The port modules use to act on the workbench: what a contributed command's handler can
//! see (the project, the active file, its repository) and do (ask to choose, work in the
//! background, tell the user through notifications, publish events), and the capabilities
//! only some modules use (`capabilities`: views, the editor, refactorings, shared services,
//! the desktop). The app implements them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::EventBus;

mod capabilities;

pub use capabilities::{EditorHost, RefactoringHost, ServicesHost, SystemHost, ViewHost};

/// The file in the active tab.
#[derive(Debug, Clone)]
pub struct ActiveFile {
    pub path: PathBuf,
    /// Its text as in the editor (unsaved changes included).
    pub text: Arc<str>,
    /// The cursor's line (0-based).
    pub line: usize,
    /// The primary cursor (bytes in `text`).
    pub cursor: usize,
    /// The primary selection (bytes in `text`), if it isn't empty.
    pub selection: Option<std::ops::Range<usize>>,
}

/// A tab of the window's current editor group, as a menu lists it: its document's identity,
/// its title and its file (`None`: never saved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    pub id: kernel::DocumentId,
    pub title: String,
    pub path: Option<PathBuf>,
}

/// A row of a choice list: icon name, label and a detail under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceItem {
    pub icon: &'static str,
    pub label: String,
    pub detail: String,
}

/// A list to choose from (a popover): its title, what it's about, its icon and what Enter
/// does ("compare"), and the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceList {
    pub title: String,
    pub subject: String,
    pub icon: &'static str,
    pub action: String,
    pub items: Vec<ChoiceItem>,
}

/// What happens with the chosen row (its index), back on the UI thread.
pub type OnChoice = Box<dyn FnOnce(usize, &mut dyn WorkbenchHost) + Send>;

/// A question asked before what can't be undone easily (a deletion): its title, what it
/// says, its icon, the "yes" button's label, whether that's dangerous (shown in red), and
/// what it is about when that's many things (the paths deleted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    pub message: String,
    pub icon: &'static str,
    pub yes: String,
    pub danger: bool,
    /// The entries the question is about (paths, `/`-separated), listed under the message:
    /// each on its row, in a box that scrolls when they're many. Empty: none.
    pub list: Vec<String>,
}

/// What runs once the question is answered "yes".
pub type OnYes = Box<dyn FnOnce(&mut dyn WorkbenchHost) + Send>;

/// Work going on, shown in the status bar: its text and icon, and the command a click on it
/// runs (its stop), if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub label: String,
    pub icon: &'static str,
    pub stop: Option<command_model::CommandId>,
}

/// A count on a tool window's button (the files to upload, the files changed): how many,
/// what it means (its color), and what the button's tooltip adds (`3 files changed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBadge {
    pub count: usize,
    pub kind: BadgeKind,
    pub label: String,
}

/// What a button's count means: its color by meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeKind {
    /// Something to do (changes, uploads): the accent.
    #[default]
    Count,
    /// Something to look at (a conflict): the warning color.
    Warning,
    /// Something failed: the error color.
    Error,
}

/// A tab a tool window at the bottom always has (Git's Log and Changes, Remote's "To upload"
/// and transfers): found by `key` (the same in every language), made by `show` — the
/// module's own command's work — when it isn't open.
#[derive(Debug, Clone, Copy)]
pub struct FixedTab {
    pub key: &'static str,
    pub show: fn(&mut dyn WorkbenchHost) -> bool,
}

/// Tool window `tool`'s fixed tabs made if they aren't open, in `tabs`' order (an open one
/// is left as it is): called by each of the window's commands before it shows its own
/// tab, so the window opens with all of them whichever is asked for.
pub fn open_fixed_tabs(host: &mut dyn WorkbenchHost, tool: &str, tabs: &[FixedTab]) {
    for tab in tabs {
        if !host.in_panel(tool, tab.key) {
            (tab.show)(host);
        }
    }
}

/// Work done in the background; its result continues on the UI thread.
pub type Job = Box<dyn FnOnce() -> Completion + Send>;
pub type Completion = Box<dyn FnOnce(&mut dyn WorkbenchHost) + Send>;

/// The workbench, as modules see it: the core every command's handler has (the project, the
/// active file, its repository, asking, working in the background, telling the user), and
/// the capabilities only some modules use, each its own trait ([`ViewHost`],
/// [`EditorHost`], [`RefactoringHost`], [`ServicesHost`], [`SystemHost`]). Modules call
/// them through the host they're given; a function needing only one capability can take it
/// (`&mut dyn RefactoringHost`, the host upcast). The capabilities' methods default to "not
/// here", so a host (a test's fake) implements only what it has.
pub trait WorkbenchHost:
    ViewHost + EditorHost + RefactoringHost + ServicesHost + SystemHost
{
    /// The open project's root.
    fn project_root(&self) -> Option<PathBuf>;
    /// What's selected where the keyboard is (the editor, the terminal, a view, a field).
    fn selected_text(&self) -> Option<String> {
        None
    }
    /// The open project's identity (new at each opening; never its name or path).
    fn project_id(&self) -> Option<kernel::ProjectId> {
        None
    }
    /// The file in the active tab (not for virtual tabs).
    fn active_file(&self) -> Option<ActiveFile>;
    /// The tabs of the current editor group, in their order.
    fn tabs(&self) -> Vec<TabInfo> {
        Vec::new()
    }
    /// What a command acts on, with what's known of it: the menu's (what was right-clicked),
    /// else the keyboard's (the focused panel's selection, else the active file).
    fn context(&self) -> crate::ActionContext {
        // A tab shows a file, never a folder (nothing to ask the disk).
        let targets = self.active_file().map(|f| crate::Target::new(f.path, false));
        crate::ActionContext::new(crate::MenuPlace::Editor, targets.into_iter().collect())
    }
    /// The file or folder a command acts on: the context's first target.
    fn target_path(&self) -> Option<PathBuf> {
        self.context().path().map(Path::to_path_buf)
    }
    /// The project's repository (version control), if it is in one.
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>>;
    /// The open project's data folder outside the project (see [`crate::ProjectService`]).
    fn project_storage(&self) -> Option<PathBuf>;
    /// What the user marked in the open project (`None`: no project, or a host without).
    fn path_marks(&self) -> Option<workspace::marks::ProjectMarks> {
        None
    }
    /// The open project's own settings (`<project dir>/project.toml`; `None`: no project open).
    fn project_settings(&self) -> Option<&settings_model::SettingsService> {
        None
    }
    /// The user's settings (`settings.toml`, one for the process): where a module saves a
    /// choice made outside its settings block (turned on from its view). `None`: a host
    /// without them.
    fn user_settings(&self) -> Option<&'static settings_model::SettingsService> {
        None
    }
    /// A file's language (its id), as the languages' modules say.
    fn language_of(&self, path: &std::path::Path) -> Option<String> {
        let _ = path;
        None
    }
    /// Lists folder `dir` of the project again (what a module knows about it changed).
    fn relist(&mut self, dir: &std::path::Path) {
        let _ = dir;
    }
    /// A project's file or folder renamed (or moved) from `from` to `to` as the app renames
    /// one: its tabs follow it, the change is announced. `Err`: why not (`to` taken, the
    /// disk's refusal, a host that can't).
    fn rename_entry(&mut self, from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
        let _ = (from, to);
        Err("renaming isn't available here".to_owned())
    }
    /// The domain events.
    fn events(&self) -> &EventBus;
    /// What the modules answer (`ask` / `can_ask` go through it). `None`: a host without.
    fn answers(&self) -> Option<Arc<crate::AnswerBook>> {
        None
    }
    /// Asks `question`; `then` runs if it's answered "yes" (nothing otherwise). `false`: a
    /// host that can't ask.
    fn confirm(&mut self, question: Question, then: OnYes) -> bool {
        let _ = (question, then);
        false
    }
    /// Before restarting module `owner`'s work for `project` (a reconnect): what of it still
    /// runs asked about in the running-work question, as before closing ([`crate::running`]);
    /// `then` runs once it's stopped ("Stop"), ended ("Wait"), or at once when nothing runs —
    /// never on "Cancel". `false`: a host that can't ask (`then` dropped, nothing ran).
    fn ask_running(&mut self, project: kernel::ProjectId, owner: &str, then: OnYes) -> bool {
        let _ = (project, owner, then);
        false
    }
    /// Shows a list; `then` gets the chosen row (nothing happens if it's dismissed).
    fn choose(&mut self, list: ChoiceList, then: OnChoice) -> bool;
    /// Runs `job` on a background thread; its completion runs here when it's done.
    fn spawn(&mut self, job: Job);
    /// Tells the user something: a toast over the workbench (this project's; merged with
    /// an identical one shown). Its id (`notification.id`) updates or closes it later.
    fn notify(&mut self, notification: crate::Notification);
    /// Replaces notification `notification.id` (work in progress that ended); shown if
    /// it's gone.
    fn update_notification(&mut self, notification: crate::Notification) {
        self.notify(notification);
    }
    /// Notification `id` is shown in this window (not leaving): what a module answers in
    /// the window that asked (its work in progress there) rather than another one.
    fn shows_notification(&self, id: crate::NotificationId) -> bool {
        let _ = id;
        false
    }
    /// Closes notification `id`.
    fn dismiss_notification(&mut self, id: crate::NotificationId) {
        let _ = id;
    }
    /// Shows (`Some`) or ends (`None`) work `id` in the status bar.
    fn set_progress(&mut self, id: &'static str, progress: Option<Progress>) {
        let _ = (id, progress);
    }
    /// What the next menu's commands act on (what was right-clicked), its facts added by
    /// the modules; `None`: the keyboard's context again.
    fn set_menu_context(&mut self, context: Option<crate::ActionContext>) {
        let _ = context;
    }
    /// A command for the view that has the keys (a modal's, the bottom window's when it's
    /// focused, else the active tab's: ⌘K on the canvas, its search). `true` if it used it.
    fn view_command(&mut self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// Runs app command `id` once the current input is handled (a choice made in a list).
    fn request_command(&mut self, id: command_model::CommandId) {
        let _ = id;
    }
    /// [`Self::request_command`] for a command named by its id's text (another module's,
    /// as a port gives it: a table's view opened). `false`: no such command.
    fn request_command_named(&mut self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// The kind of view that has the keys (its viewer's id: `canvas`, `git.log`…), unless
    /// a field of it takes them; `None`: the editor, or no view.
    fn keyed_view(&self) -> Option<&'static str> {
        None
    }
    /// The editor (a file's text) has the keyboard, not a panel, a field or a view.
    fn editor_focused(&self) -> bool {
        false
    }
    /// This window has the keyboard (the one the user works in now, of all that are open).
    fn window_focused(&self) -> bool {
        false
    }
}

/// A command contributed by a module: how it appears (its spec: label, menu, shortcuts;
/// the label is translated from `command.<id>` with dots as `_`, the spec's being the
/// fallback), its icon in context menus, and what it does.
#[derive(Clone)]
pub struct CommandContribution {
    pub spec: command_model::CommandSpec,
    pub icon: Option<&'static str>,
    pub handler: Arc<dyn CommandHandler>,
}

/// A module implementing a command the workbench declares (its label, its place in the
/// menus and its keys are the app's: [`SEARCH_PROJECT`]); without the module, the command
/// isn't there.
#[derive(Clone)]
pub struct CommandImplementation {
    pub id: command_model::CommandId,
    pub handler: Arc<dyn CommandHandler>,
}

impl std::fmt::Debug for CommandImplementation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandImplementation").field("id", &self.id).finish_non_exhaustive()
    }
}

/// A mark in the editor's gutter a click acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMark {
    /// A bar beside lines changed since the last commit.
    Change,
    /// A line's annotation (blame): its commit.
    Annotation,
}

/// What a click on a gutter mark runs: a module's command (the git module's change
/// actions); none, or it declines: nothing (the click only moves the cursor there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterAction {
    pub mark: GutterMark,
    pub command: command_model::CommandId,
}

/// "Search in Project…": declared by the workbench, implemented by whoever searches the
/// project (the indexing module).
pub const SEARCH_PROJECT: command_model::CommandId = command_model::CommandId("project.search");

/// Shows what the app's parts cost (the status bar's CPU / RAM / GPU open it): declared by
/// the workbench, implemented by whoever shows it (the Task Manager module).
pub const SHOW_RESOURCES: command_model::CommandId = command_model::CommandId("tasks.open");

/// Opens the theme configurator (the settings' "Theme Studio…"): declared by the workbench,
/// implemented by the theme studio module.
pub const THEME_STUDIO: command_model::CommandId = command_model::CommandId("app.theme_studio");

/// Opens a project on a server (the projects list's action): declared by the workbench,
/// implemented by whoever connects to servers (the remote projects module).
pub const OPEN_REMOTE_PROJECT: command_model::CommandId = command_model::CommandId("remote.open");

/// The tool window showing the project's entries (the project tree's): the workbench tells
/// it what to reveal ([`crate::EntriesRequest`]) and shows it with a new project.
pub const PROJECT_PANEL: &str = "project";

/// Shows / hides the [`PROJECT_PANEL`] (⌘1): the workbench's command (without a project, it
/// asks for one), the panel's toggle.
pub const TOGGLE_PROJECT_PANEL: command_model::CommandId =
    command_model::CommandId("view.toggle_project");

/// Closes the active tab (as ⌘W: its unsaved changes asked about first): the workbench's
/// command, run by a module once it asked what it had to (a console's transaction).
pub const CLOSE_TAB: command_model::CommandId = command_model::CommandId("tab.close");

/// Comments the selected lines out with the language's line comment (and back): the
/// workbench's command, put in a language's editor menu by its module.
pub const COMMENT_LINE: command_model::CommandId = command_model::CommandId("code.comment");

/// "Show Diagram": the type at the cursor drawn with its family (the diagrams module's);
/// the workbench's type hierarchy offers it a row away.
pub const SHOW_TYPE_DIAGRAM: command_model::CommandId = command_model::CommandId("diagram.show");

/// Shows / hides the Services window: declared by the workbench, implemented by whoever shows
/// it; whoever has something to show there runs it.
pub const TOGGLE_SERVICES: command_model::CommandId =
    command_model::CommandId("view.toggle_services");
/// The Services window's tool id.
pub const SERVICES_TOOL: &str = "services";

/// "Choose the File's Language Engine…": declared by the workbench (its status bar's
/// engine segment runs it), implemented by whoever runs the language servers.
pub const CHOOSE_ENGINE: command_model::CommandId = command_model::CommandId("lsp.choose_engine");

/// "Find Usages" of the declaration at the cursor (the refactoring module's); the code
/// lenses' usages run it.
pub const FIND_USAGES: command_model::CommandId = command_model::CommandId("refactor.find_usages");

/// Goes to the declaration of what's at the cursor (⌘B, ⌘-click): the workbench's command,
/// put in an editor's menu by a module whose files it resolves (a query console's).
pub const GOTO_DECLARATION: command_model::CommandId =
    command_model::CommandId("navigate.definition");

/// Lays the active file out (⌥⌘L): the Code quality module's command — the project's
/// formatter, else the language's own (a SQL file's) — put in a language's editor menu by
/// its module.
pub const REFORMAT_FILE: command_model::CommandId = command_model::CommandId("quality.format_file");

/// Renaming in the editor: the symbol at the cursor (the refactoring module's command; a
/// tab's Rename runs it when the editor has the keys).
pub const RENAME_SYMBOL: command_model::CommandId = command_model::CommandId("refactor.rename");

/// The workbench's commands a module implements.
pub const IMPLEMENTED: &[command_model::CommandId] = &[SEARCH_PROJECT];

impl std::fmt::Debug for CommandContribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandContribution").field("id", &self.spec.id).finish_non_exhaustive()
    }
}

/// What a contributed command does.
/// A command showing / hiding a module's tool window (its toggle, with its shortcut).
#[derive(Debug, Clone, Copy)]
pub struct ToggleTool(pub &'static str);

impl CommandHandler for ToggleTool {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.toggle_tool(self.0)
    }
}

/// A module taking over renaming or moving a project's file when that's a refactoring (the
/// type named after the file renamed with it, the namespaces following a move): it opens
/// its preview. `false` leaves it to the plain operation.
pub trait FileRefactoring: Send + Sync + std::fmt::Debug {
    /// Renaming file `path` to `name` (a file name).
    fn rename(&self, host: &mut dyn WorkbenchHost, path: &Path, name: &str) -> bool;
    /// Moving `from` (a file or a folder) into folder `into`.
    fn moved(&self, host: &mut dyn WorkbenchHost, from: &Path, into: &Path) -> bool;
}

pub trait CommandHandler: Send + Sync {
    /// Whether it applies now (e.g. a git command in a project without git).
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        true
    }
    /// Runs it. `true` if something changed (a redraw).
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool;
    /// A setting it turns on or off: how it stands now, shown as a switch at the end of its
    /// menu row. `None` (the default): a plain action. Asked as a menu opens.
    fn switched(&self, _host: &dyn WorkbenchHost) -> Option<bool> {
        None
    }
    /// Where its shortcuts work (a view's actions: in that view).
    fn when(&self) -> command_model::When {
        command_model::When::Everywhere
    }
    /// What it needs of the window (a project, a repository, a text file…): without it,
    /// its menu row is greyed out (with why) and its keys do nothing, before [`enabled`]
    /// is even asked.
    ///
    /// [`enabled`]: CommandHandler::enabled
    fn requires(&self) -> &'static [command_model::Requires] {
        &[]
    }
}

/// A handler with what it needs of the window declared ([`CommandHandler::requires`]).
#[derive(Clone)]
pub struct Requiring {
    pub requires: &'static [command_model::Requires],
    pub handler: Arc<dyn CommandHandler>,
}

impl Requiring {
    /// `handler`, needing `requires`.
    pub fn new(
        requires: &'static [command_model::Requires],
        handler: Arc<dyn CommandHandler>,
    ) -> Arc<Self> {
        Arc::new(Self { requires, handler })
    }
}

impl std::fmt::Debug for Requiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Requiring").field("requires", &self.requires).finish_non_exhaustive()
    }
}

impl CommandHandler for Requiring {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        self.handler.enabled(host)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        self.handler.run(host)
    }

    fn switched(&self, host: &dyn WorkbenchHost) -> Option<bool> {
        self.handler.switched(host)
    }

    fn when(&self) -> command_model::When {
        self.handler.when()
    }

    fn requires(&self) -> &'static [command_model::Requires] {
        self.requires
    }
}

/// A view's action as a command (remappable, its shortcuts working only in that view):
/// sent to the view as [`WorkbenchHost::view_command`] with `action` when it has the keys.
#[derive(Debug, Clone, Copy)]
pub struct ViewAction {
    /// The view's kind (its viewer's id).
    pub view: &'static str,
    pub action: &'static str,
}

impl CommandHandler for ViewAction {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.keyed_view() == Some(self.view)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.keyed_view() == Some(self.view) && host.view_command(self.action)
    }

    fn when(&self) -> command_model::When {
        command_model::When::View(self.view)
    }
}
