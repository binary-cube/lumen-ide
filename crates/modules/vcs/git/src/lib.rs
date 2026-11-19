//! Git module: opens the project's repository (with gitoxide: no `git` installed needed,
//! Windows too), decorates the editor's documents (the gutter's changed lines, the blame
//! annotations: `decorations`) and shows the branch beside the project's name, and brings the Git menu: compare the active file with the last commit, with a
//! version from its history or with a branch, the blame annotations, commit, fetch / pull /
//! push, revert a change (or the gutter bar's menu), and the log with its graph. Its
//! button counts the working tree's changed files (read in the background as they change),
//! and opens its window: the Log and the Commit, fixed tabs. Conflicts (a rebase, a merge
//! stopped on them, here or in another program) are told, and listed in their own tab with
//! the operation's steps (continue, skip, abort).
//!
//! The commands only use the workbench port ([`WorkbenchHost`]) and the version control
//! use cases (`version_control::application`). Each piece loads when used: the views are made
//! when opened (the log reads its commits page by page, in the background) and let
//! everything go when their tab closes; the remote operations run in the background.
//! Without `git` installed, reading works and the operations that need it say so.

mod agent_tools;
mod branch_picker;
mod branches;
mod changes;
mod changes_view;
mod commit_view;
mod conflicts_view;
mod decorations;
mod divider;
mod files_tree;
mod gutter_diff;
mod ignored;
mod local_time;
mod log_view;
mod operation;
mod push_dialog;
mod ref_dialog;
mod ref_menu;
mod refs_store;
mod stash;
mod stash_dialog;
mod stash_menu;
mod status;
mod switch;
mod switch_dialog;
mod sync;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec, MenuAt, MenuSectionSpec, Requires, SubmenuSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use ui_components::{
    Dock, ToolContent, ToolWindow, ToolWindowProvider, Viewer, ViewerKind, ViewerProvider,
};
use version_control::application::{SyncOp, compare_with, file_history};
use version_control::{OperationStep, Repository, Revision, VcsProvider};
use workbench_model::{
    ActiveFile, ChoiceItem, ChoiceList, CommandContribution, CommandHandler, Completion,
    EVERYWHERE, GutterAction, GutterMark, MenuContribution, MenuPlace, MenuRow, ProjectService,
    ViewAction, WorkbenchHost,
};

/// What its views read of the project's files (a change compared): the Task Manager's disk
/// column, on the module's row.
pub(crate) static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("vcs.git"),
    name: "Git",
    description: "Lines changed since the last commit, comparisons with revisions and branches, blame, commits",
    depends_on: &[],
};

pub const COMPARE_HEAD: CommandId = CommandId("git.compare_head");
pub const COMPARE_REVISION: CommandId = CommandId("git.compare_revision");
pub const COMPARE_BRANCH: CommandId = CommandId("git.compare_branch");
pub const ANNOTATE: CommandId = CommandId("git.annotate");
pub const COMMIT: CommandId = CommandId("git.commit");
pub const FETCH: CommandId = CommandId("git.fetch");
pub const PULL: CommandId = CommandId("git.pull");
pub const PUSH: CommandId = CommandId("git.push");
pub const PUSH_TO: CommandId = CommandId("git.push_to");
pub const SHOW_LOG: CommandId = CommandId("git.log");
pub const SHOW_CHANGES: CommandId = CommandId("git.changes");
pub const REVERT_CHANGE: CommandId = CommandId("git.revert_change");
/// The actions of the change at the cursor (a click on a gutter bar).
pub const CHANGE_ACTIONS: CommandId = CommandId("git.change_actions");
/// The commit of the active line's annotation, in the log (a click on a blame annotation).
pub const ANNOTATED_COMMIT: CommandId = CommandId("git.annotated_commit");
pub const FILE_HISTORY: CommandId = CommandId("git.file_history");
pub const ROLLBACK: CommandId = CommandId("git.rollback");
pub const NEW_BRANCH: CommandId = CommandId("git.new_branch");
pub const RENAME_BRANCH: CommandId = CommandId("git.rename_branch");
pub const NEW_TAG: CommandId = CommandId("git.new_tag");
pub const SHOW_CONFLICTS: CommandId = CommandId("git.conflicts");
pub const CONTINUE_OPERATION: CommandId = CommandId("git.continue_operation");
pub const SKIP_OPERATION: CommandId = CommandId("git.skip_operation");
pub const ABORT_OPERATION: CommandId = CommandId("git.abort_operation");
/// The work put aside to switch branches brought back (the switch's notification).
pub const POP_SWITCH_STASH: CommandId = CommandId("git.pop_switch_stash");
pub const STASH: CommandId = CommandId("git.stash");
pub const SWITCH_BRANCH: CommandId = CommandId("git.switch_branch");

/// The menu bar's Git menu: its sections (the module's own, as its context menu groups
/// them) and Compare With ›.
const REPOSITORY: &str = "git.repository";
/// In Progress ›: the operation in progress' conflicts and steps.
const IN_PROGRESS: &str = "git.in_progress";
const OPERATION: &str = "git.operation";
const SYNC: &str = "git.sync";
const FILE: &str = "git.file";
const LINE: &str = "git.line";
const COMPARE_WITH: &str = "git.compare_with";
const COMPARE: &str = "git.compare";
/// Branches and Tags ›: New Branch…, Rename Branch…, New Tag….
const BRANCHES_AND_TAGS: &str = "git.branches_and_tags";
const BRANCHES: &str = "git.branches";
/// Where a command is in the Git menu: the repository's, its syncing, the file's, the
/// cursor's line.
fn menu_place(id: CommandId) -> Option<MenuAt> {
    let (section, order) = match id {
        COMMIT => (REPOSITORY, 10),
        SHOW_CHANGES => (REPOSITORY, 20),
        SHOW_LOG => (REPOSITORY, 30),
        STASH => (BRANCHES, 50),
        SHOW_CONFLICTS => (OPERATION, 10),
        CONTINUE_OPERATION => (OPERATION, 20),
        SKIP_OPERATION => (OPERATION, 30),
        ABORT_OPERATION => (OPERATION, 40),
        SWITCH_BRANCH => (BRANCHES, 5),
        NEW_BRANCH => (BRANCHES, 10),
        RENAME_BRANCH => (BRANCHES, 20),
        NEW_TAG => (BRANCHES, 30),
        PULL => (SYNC, 10),
        PUSH => (SYNC, 20),
        PUSH_TO => (BRANCHES, 40),
        FETCH => (SYNC, 30),
        ANNOTATE => (FILE, 10),
        FILE_HISTORY => (FILE, 20),
        ROLLBACK => (FILE, 40),
        COMPARE_HEAD => (COMPARE, 10),
        COMPARE_REVISION => (COMPARE, 20),
        COMPARE_BRANCH => (COMPARE, 30),
        REVERT_CHANGE => (LINE, 10),
        CHANGE_ACTIONS => (LINE, 20),
        ANNOTATED_COMMIT => (LINE, 30),
        _ => return None,
    };
    Some(MenuAt::new(section, order))
}

/// How many commits the history list shows at most.
const HISTORY: usize = 200;

#[derive(Debug, Default)]
pub struct GitModule;

impl Module for GitModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&[
            "git-status",
            "git-log",
            "git-commit",
            "git-input",
            "git-output",
            "git-conflicts",
        ]));
        ctx.contribute(workbench_model::ModuleCounters { disk: Some(&DISK), traffic: None });
        ctx.contribute::<Arc<dyn VcsProvider>>(Arc::new(GitProvider));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(GitPrograms));
        // The editor's changed lines and blame annotations, and the branch.
        ctx.contribute::<Arc<dyn workbench_model::LineDecorator>>(Arc::new(
            decorations::GitDecorator,
        ));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(
            decorations::DecorationsCost,
        ));
        // A click on a change bar: its actions (revert, compare).
        ctx.contribute(GutterAction { mark: GutterMark::Change, command: CHANGE_ACTIONS });
        // A click on an annotation: its commit in the log.
        ctx.contribute(GutterAction { mark: GutterMark::Annotation, command: ANNOTATED_COMMIT });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(CommitProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(log_view::LogProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(changes_view::ChangesProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(conflicts_view::ConflictsProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(ref_dialog::RefDialogProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(push_dialog::PushDialogProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(switch_dialog::SwitchDialogProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(stash_dialog::StashDialogProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(branch_picker::BranchPickerProvider));
        // The top bar's branch: a list to switch it.
        ctx.contribute(workbench_model::TopBarLabelAction {
            command: SWITCH_BRANCH,
            tooltip: "git.switch_branch_tip",
        });
        // The changed files, followed per project (the button's count, the Changes tab).
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(status::ChangesService));
        let keyed = |id, label, icon, keys: &'static [&'static str], handler| CommandContribution {
            spec: CommandSpec { id, label, menu: None, shortcuts: keys },
            icon: Some(icon),
            handler,
        };
        let command = |id, label, icon, handler: Arc<dyn CommandHandler>| {
            keyed(id, label, icon, &[], handler)
        };
        for contribution in [
            command(COMMIT, "Commit…", "git-commit", Arc::new(OpenCommit)),
            keyed(PULL, "Pull", "arrow-down", &["CmdOrCtrl+T"], Arc::new(sync::Sync(SyncOp::Pull))),
            keyed(
                PUSH,
                "Push",
                "arrow-up",
                &["CmdOrCtrl+Shift+K"],
                Arc::new(sync::Sync(SyncOp::Push)),
            ),
            command(PUSH_TO, "Push…", "arrow-up", Arc::new(push_dialog::OpenPush)),
            command(FETCH, "Fetch", "download", Arc::new(sync::Sync(SyncOp::Fetch))),
            keyed(SHOW_LOG, "Show Git Log", "git-branch", &["Alt+9"], Arc::new(log_view::ShowLog)),
            command(SHOW_CHANGES, "Show Changes", "file-pen", Arc::new(ShowChanges)),
            keyed(
                REVERT_CHANGE,
                "Revert Change",
                "rotate-ccw",
                &["CmdOrCtrl+Alt+Z"],
                Arc::new(changes::RevertChange),
            ),
            command(
                CHANGE_ACTIONS,
                "Change Actions",
                "git-commit",
                Arc::new(changes::ChangeActions),
            ),
            command(
                ANNOTATED_COMMIT,
                "Show Annotated Commit",
                "git-commit",
                Arc::new(log_view::ShowAnnotatedCommit),
            ),
            command(FILE_HISTORY, "Show History", "clock", Arc::new(log_view::ShowFileHistory)),
            keyed(
                ROLLBACK,
                "Rollback…",
                "rotate-ccw",
                &["CmdOrCtrl+Alt+Shift+Z"],
                Arc::new(Rollback),
            ),
            command(COMPARE_HEAD, "Compare with Last Commit", "git-commit", Arc::new(CompareHead)),
            command(
                COMPARE_REVISION,
                "Compare with Revision…",
                "git-commit",
                Arc::new(ChooseRevision),
            ),
            command(COMPARE_BRANCH, "Compare with Branch…", "git-branch", Arc::new(ChooseBranch)),
            command(ANNOTATE, "Annotate with Git Blame", "file-pen", Arc::new(Annotate)),
            command(
                NEW_BRANCH,
                "New Branch…",
                "git-branch",
                Arc::new(ref_dialog::AskRef(ref_dialog::new_branch)),
            ),
            command(
                RENAME_BRANCH,
                "Rename Branch…",
                "file-pen",
                Arc::new(ref_dialog::AskRef(ref_dialog::rename_branch)),
            ),
            command(NEW_TAG, "New Tag…", "tag", Arc::new(ref_dialog::AskRef(ref_dialog::new_tag))),
            command(SHOW_CONFLICTS, "Resolve Conflicts…", "compare", Arc::new(ShowConflicts)),
            command(
                SWITCH_BRANCH,
                "Switch Branch…",
                "arrow-left-right",
                Arc::new(branch_picker::SwitchBranch),
            ),
            keyed(
                STASH,
                "Stash Changes…",
                "archive-down",
                &[],
                Arc::new(stash_dialog::StashChanges),
            ),
            command(
                POP_SWITCH_STASH,
                "Bring Back the Work Put Aside",
                "archive-up",
                Arc::new(switch::PopSwitchStash),
            ),
            command(
                CONTINUE_OPERATION,
                "Continue",
                "play",
                Arc::new(operation::Proceed(OperationStep::Continue)),
            ),
            command(
                SKIP_OPERATION,
                "Skip Commit",
                "arrow-right-to-line",
                Arc::new(operation::Proceed(OperationStep::Skip)),
            ),
            command(
                ABORT_OPERATION,
                "Abort",
                "rotate-ccw",
                Arc::new(operation::Proceed(OperationStep::Abort)),
            ),
        ] {
            let mut contribution = contribution;
            contribution.spec.menu = menu_place(contribution.spec.id);
            // A project in a repository; the file's (and its lines') a file on disk too.
            let file = [FILE, LINE, COMPARE]
                .iter()
                .any(|s| contribution.spec.menu.is_some_and(|m| m.section == *s));
            let requires: &'static [Requires] = if file {
                &[Requires::Project, Requires::Repository, Requires::FileOnDisk]
            } else {
                &[Requires::Project, Requires::Repository]
            };
            contribution.handler = workbench_model::Requiring::new(requires, contribution.handler);
            ctx.contribute(contribution);
        }
        // ⌘⇧K is the jetbrains preset's Push; vscode, zed and sublime have none (⌘⇧K deletes
        // the line).
        for preset in ["vscode", "zed", "sublime"] {
            ctx.contribute(command_model::bindings::PresetShortcuts {
                preset,
                command: PUSH,
                shortcuts: &[],
            });
        }
        for (id, order) in [(REPOSITORY, 10), (SYNC, 20), (FILE, 30), (LINE, 40)] {
            ctx.contribute(MenuSectionSpec::new(id, workbench_model::menus::bar::GIT, order));
        }
        ctx.contribute(SubmenuSpec {
            id: COMPARE_WITH,
            at: MenuAt::new(FILE, 30),
            label_key: "menu.compare_with",
            label: "Compare With",
            icon: Some("compare"),
        });
        ctx.contribute(MenuSectionSpec::new(COMPARE, COMPARE_WITH, 10));
        ctx.contribute(SubmenuSpec {
            id: BRANCHES_AND_TAGS,
            at: MenuAt::new(REPOSITORY, 40),
            label_key: "menu.git_branches_and_tags",
            label: "Branches and Tags",
            icon: Some("git-branch"),
        });
        ctx.contribute(MenuSectionSpec::new(BRANCHES, BRANCHES_AND_TAGS, 10));
        ctx.contribute(SubmenuSpec {
            id: IN_PROGRESS,
            at: MenuAt::new(REPOSITORY, 50),
            label_key: "menu.git_in_progress",
            label: "In Progress",
            icon: Some("compare"),
        });
        ctx.contribute(MenuSectionSpec::new(OPERATION, IN_PROGRESS, 10));
        ctx.contribute(git_menu());
        // What it offers the agents (through whichever MCP server runs).
        ctx.contribute(Arc::new(agent_tools::GitStatus) as Arc<dyn mcp::McpTool>);
        ctx.contribute(Arc::new(agent_tools::GitHeadText) as Arc<dyn mcp::McpTool>);
        // Its window at the bottom (shown in a repository): the Log and the Changes as its
        // first, fixed tabs, then the files' histories; its button opens it on the Log.
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: "git",
            icon: "git-branch",
            title_key: "tooltip.git",
            dock: Dock::Bottom,
            // The last of its group: the strip's bottom corner.
            order: 30,
            strip_end: false,
            group: "",
            toggle: None,
            content: ToolContent::Views { command: SHOW_LOG.0, new: None },
        }));
        // Committing from the message (↵ makes a new line there).
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: COMMIT_NOW,
                label: "Commit Now",
                menu: None,
                shortcuts: &["CmdOrCtrl+Enter"],
            },
            icon: Some("git-commit"),
            handler: Arc::new(ViewAction { view: "git.commit", action: "commit" }),
        });
        // Reloading the log (its key never reached it at the bottom: it does now).
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RELOAD_LOG,
                label: "Reload Log",
                menu: None,
                shortcuts: &["r"],
            },
            icon: None,
            handler: Arc::new(ViewAction { view: log_view::VIEW, action: "reload" }),
        });
        Ok(())
    }
}

/// Commits what the commit view has chosen (remappable; its key there, ⌘ / Ctrl+↵).
pub const COMMIT_NOW: CommandId = CommandId("git.commit_now");

/// Reads the log again (remappable; its key in the log, `r`).
pub const RELOAD_LOG: CommandId = CommandId("git.reload_log");

/// The Git menu in the editor's and the tree's right-click menus (`Git ›`) and behind the
/// tool strip's button: what concerns a file where there is one, the repository's
/// operations everywhere.
fn git_menu() -> MenuContribution {
    const FILE: &[MenuPlace] = &[MenuPlace::Editor, MenuPlace::TreeFile];
    const EDITOR: &[MenuPlace] = &[MenuPlace::Editor];
    MenuContribution {
        id: "git",
        label_key: "git.menu",
        icon: "git-branch",
        places: EVERYWHERE,
        rows: vec![
            MenuRow::Command(SHOW_LOG, &[MenuPlace::ToolStrip]),
            MenuRow::Command(SHOW_CONFLICTS, &[MenuPlace::ToolStrip]),
            MenuRow::Command(COMMIT, EVERYWHERE),
            MenuRow::Command(NEW_BRANCH, &[MenuPlace::ToolStrip]),
            MenuRow::Command(NEW_TAG, &[MenuPlace::ToolStrip]),
            MenuRow::Separator,
            MenuRow::Command(ANNOTATE, EDITOR),
            MenuRow::Command(COMPARE_HEAD, FILE),
            MenuRow::Command(COMPARE_REVISION, FILE),
            MenuRow::Command(COMPARE_BRANCH, FILE),
            MenuRow::Command(FILE_HISTORY, FILE),
            MenuRow::Command(REVERT_CHANGE, EDITOR),
            MenuRow::Command(ROLLBACK, EDITOR),
            MenuRow::Separator,
            MenuRow::Command(PULL, EVERYWHERE),
            MenuRow::Command(PUSH, EVERYWHERE),
            MenuRow::Command(PUSH_TO, EVERYWHERE),
            MenuRow::Command(FETCH, EVERYWHERE),
            MenuRow::Separator,
            MenuRow::Command(
                SHOW_LOG,
                &[MenuPlace::Editor, MenuPlace::TreeFile, MenuPlace::TreeFolder],
            ),
        ],
    }
}

/// The "Commit" view, in a tab.
#[derive(Debug)]
struct CommitProvider;

impl ViewerProvider for CommitProvider {
    fn id(&self) -> &'static str {
        "git.commit"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == commit_view::EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(commit_view::CommitView::default()))
    }
}

/// The Git tool window's "Changes" tab.
struct ShowChanges;

impl CommandHandler for ShowChanges {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        log_view::open_fixed_tabs(host);
        show_changes(host)
    }
}

/// The Changes tab shown (made if it isn't open). `false` without a repository.
pub(crate) fn show_changes(host: &mut dyn WorkbenchHost) -> bool {
    let (Some(repo), Some(project)) = (host.repository(), host.project_id()) else {
        return false;
    };
    let data = changes_view::ChangesData { project, repo, queries: host.queries() };
    let title = t("git.changes_tab");
    host.show_in_panel_keyed(
        log_view::TOOL,
        changes_view::KEY,
        &title,
        changes_view::EXTENSION,
        &data,
    )
}

/// The Commit tab shown (made if it isn't open): the commit screen with the repository's
/// places beside it. `false` without a repository.
pub(crate) fn show_commit(host: &mut dyn WorkbenchHost) -> bool {
    let Some(repo) = host.repository() else { return false };
    let data = commit_view::CommitData { repo, project: host.project_id(), docked: true };
    let title = t("git.commit_title");
    host.show_in_panel_keyed(
        log_view::TOOL,
        commit_view::KEY,
        &title,
        commit_view::EXTENSION,
        &data,
    )
}

/// The Git tool window's "Conflicts" tab (while there are conflicts or an operation is in
/// progress).
struct ShowConflicts;

impl CommandHandler for ShowConflicts {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        let Some(changes) = host.project_id().and_then(status::changes) else { return false };
        host.repository().is_some()
            && (changes.operation().is_some() || !changes.conflicted().is_empty())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(repo), Some(project)) = (host.repository(), host.project_id()) else {
            return false;
        };
        log_view::open_fixed_tabs(host);
        let data = conflicts_view::ConflictsData { project, repo, queries: host.queries() };
        let title = t("git.conflicts_tab");
        host.show_in_panel_keyed(
            log_view::TOOL,
            conflicts_view::KEY,
            &title,
            conflicts_view::EXTENSION,
            &data,
        )
    }
}

struct OpenCommit;

impl CommandHandler for OpenCommit {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    /// The Git window's Commit tab (no longer a tab over the editor).
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        log_view::open_fixed_tabs(host);
        show_commit(host)
    }
}

/// Opens git repositories.
#[derive(Debug)]
struct GitProvider;

impl VcsProvider for GitProvider {
    fn id(&self) -> &'static str {
        "git"
    }

    fn open(&self, dir: &Path) -> Option<Arc<dyn Repository>> {
        git::GitRepository::discover(dir).map(|r| Arc::new(r) as Arc<dyn Repository>)
    }

    fn metadata_dir(&self) -> Option<&'static str> {
        Some(".git")
    }
}

/// The active file (or the one the menu was opened on) compared with its version at
/// `revision` (called `label`).
fn compare(host: &mut dyn WorkbenchHost, revision: &Revision, label: &str) -> bool {
    let Some(file) = host.active_file() else { return false };
    compare_file(host, &file, revision, label)
}

/// `file` compared with its version at `revision`.
fn compare_file(
    host: &mut dyn WorkbenchHost,
    file: &ActiveFile,
    revision: &Revision,
    label: &str,
) -> bool {
    let Some(repo) = host.repository() else { return false };
    let Some(input) = compare_with(repo.as_ref(), &file.path, &file.text, revision, label) else {
        host.notify(workbench_model::Notification::info(t("git.not_in_repository")));
        return false;
    };
    let name = kernel::paths::file_name(&file.path);
    host.compare(input, &format!("{name} ↔ {label}"))
}

/// The active file's name (the subject of the lists).
fn file_name(host: &dyn WorkbenchHost) -> String {
    host.active_file().map(|f| kernel::paths::file_name(&f.path)).unwrap_or_default()
}

/// Brings the active file back to its last commit (one undo step).
struct Rollback;

impl CommandHandler for Rollback {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        match (host.active_file(), decorations::committed_text(host)) {
            (Some(file), Some(base)) => *file.text != *base.replace("\r\n", "\n"),
            _ => false,
        }
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(file), Some(base)) = (host.active_file(), decorations::committed_text(host))
        else {
            return false;
        };
        let base = base.replace("\r\n", "\n");
        let done = host.edit_active(0..file.text.len(), &base);
        if done {
            host.notify(workbench_model::Notification::success(t("git.rolled_back")));
        }
        done
    }
}

/// Enabled when the project is in a repository and a file is open.
fn applies(host: &dyn WorkbenchHost) -> bool {
    host.repository().is_some() && host.active_file().is_some()
}

/// [`applies`], and someone shows comparisons.
fn can_compare(host: &dyn WorkbenchHost) -> bool {
    applies(host) && workbench_model::presence::compare()
}

struct CompareHead;

impl CommandHandler for CompareHead {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        can_compare(host)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        compare(host, &Revision::Head, "HEAD")
    }
}

struct ChooseRevision;

impl CommandHandler for ChooseRevision {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        can_compare(host)
    }

    /// The history is read in the background (a long one takes a moment), then listed.
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(repo), Some(file)) = (host.repository(), host.active_file()) else {
            return false;
        };
        host.spawn(Box::new(move || {
            let commits = file_history(repo.as_ref(), &file.path, HISTORY);
            Box::new(move |host: &mut dyn WorkbenchHost| show_history(host, file, commits))
                as Completion
        }));
        true
    }
}

fn show_history(
    host: &mut dyn WorkbenchHost,
    file: ActiveFile,
    commits: Vec<version_control::Commit>,
) {
    if commits.is_empty() {
        host.notify(workbench_model::Notification::info(t("git.no_history")));
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let items = commits
        .iter()
        .map(|c| ChoiceItem {
            icon: "git-commit",
            label: c.summary.clone(),
            detail: format!("{} · {} · {}", c.short(), c.author, i18n::ago(now - c.time)),
        })
        .collect();
    let list = ChoiceList {
        title: t("git.compare_revision"),
        subject: kernel::paths::file_name(&file.path),
        icon: "git-commit",
        action: t("git.compare_action"),
        items,
    };
    host.choose(
        list,
        Box::new(move |i, host| {
            if let Some(c) = commits.get(i) {
                compare_file(host, &file, &Revision::Named(c.id.clone()), c.short());
            }
        }),
    );
}

struct ChooseBranch;

impl CommandHandler for ChooseBranch {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        can_compare(host)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(repo), Some(file)) = (host.repository(), host.active_file()) else {
            return false;
        };
        host.spawn(Box::new(move || {
            let current = repo.branch();
            let branches: Vec<String> =
                repo.branches().into_iter().filter(|b| Some(b) != current.as_ref()).collect();
            Box::new(move |host: &mut dyn WorkbenchHost| {
                offer_branches(host, file, branches);
            }) as Completion
        }));
        true
    }
}

/// The other branches offered to compare `file` with (none: says so).
fn offer_branches(host: &mut dyn WorkbenchHost, file: ActiveFile, branches: Vec<String>) {
    if branches.is_empty() {
        host.notify(workbench_model::Notification::info(t("git.no_other_branch")));
        return;
    }
    let items = branches
        .iter()
        .map(|b| ChoiceItem {
            icon: "git-branch",
            label: b.clone(),
            detail: t("git.branch_detail"),
        })
        .collect();
    let list = ChoiceList {
        title: t("git.compare_branch"),
        subject: kernel::paths::file_name(&file.path),
        icon: "git-branch",
        action: t("git.compare_action"),
        items,
    };
    host.choose(
        list,
        Box::new(move |i, host| {
            if let Some(branch) = branches.get(i) {
                compare_file(host, &file, &Revision::Named(branch.clone()), branch);
            }
        }),
    );
}

struct Annotate;

impl CommandHandler for Annotate {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        applies(host)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        decorations::toggle_annotations(host)
    }
}

/// A version control error, for the user.
pub fn error_message(error: &version_control::VcsError) -> String {
    use version_control::VcsError;
    match error {
        VcsError::ToolMissing(tool) => tr!("git.tool_missing", tool = tool.as_str()),
        VcsError::NoRemote => t("git.no_remote"),
        VcsError::Authentication(message) => tr!("git.authentication", message = message.as_str()),
        VcsError::TimedOut => t("git.timed_out"),
        VcsError::Failed(message) => message.clone(),
        VcsError::Untrusted(_) => t("git.untrusted"),
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;

/// A repository in memory, for the module's tests.
/// The `git`s running (fetch, pull, push, commit…): their CPU, memory and disk, with the
/// module's threads.
#[derive(Debug)]
struct GitPrograms;

impl workbench_model::ResourceReporter for GitPrograms {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        let running = git::running();
        // No git running and no project to run it for: nothing to show.
        if running.is_empty() && !decorations::any_project() {
            return Vec::new();
        }
        let mut part =
            workbench_model::ResourceUse::new(i18n::t("tasks.git_programs"), MANIFEST.id.as_str());
        part.threads = vec!["git-status", "git-log", "git-commit"];
        part.group = workbench_model::resources::PROGRAMS;
        part.processes = running;
        part.detail = i18n::tr!("tasks.programs_detail", count = part.processes.len());
        // No actions: a git stopped midway leaves its `index.lock` (and a half-written
        // repository) behind.
        vec![part]
    }

    /// Its fetches, pulls and pushes running (asked about before closing).
    fn running(&self) -> Vec<workbench_model::RunningWork> {
        sync::running()
    }
}

#[cfg(test)]
#[path = "../tests/unit/fake.rs"]
mod fake;
