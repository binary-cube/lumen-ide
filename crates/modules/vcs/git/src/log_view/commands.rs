//! The Git log's commands: the log, a file's history, the annotated commit — each in the
//! Git window, whose first two tabs are always the Log and the Commit (fixed: made the
//! first time the window opens, in this order, before any history).

use super::*;

/// The Git window's fixed tabs: the Log, then the Commit. The Changes is no longer one (the
/// Commit screen shows the same, with staging): it opens on demand ("Show Changes").
const FIXED: &[workbench_model::FixedTab] = &[
    workbench_model::FixedTab { key: LOG_KEY, show: show_log },
    workbench_model::FixedTab { key: crate::commit_view::KEY, show: crate::show_commit },
];

/// The Git window's fixed tabs made if they aren't open yet (an open one is left as it is).
pub(crate) fn open_fixed_tabs(host: &mut dyn WorkbenchHost) {
    workbench_model::open_fixed_tabs(host, TOOL, FIXED);
}

/// The Git window's fixed tabs a session lacked (saved before one was fixed, or one not
/// made again), made behind the one shown: Log and Commit always there, nothing shown
/// changes.
pub(crate) fn add_missing_fixed_tabs(host: &mut dyn WorkbenchHost) {
    let Some(repo) = host.repository() else { return };
    if !host.in_panel(TOOL, LOG_KEY) {
        let data = LogData {
            repo: Arc::clone(&repo),
            path: None,
            commit: None,
            queries: host.queries(),
            reference: None,
        };
        host.add_to_panel_keyed(TOOL, LOG_KEY, &t("git.log_tab"), EXTENSION, &data);
    }
    if !host.in_panel(TOOL, crate::commit_view::KEY) {
        let data =
            crate::commit_view::CommitData { repo, project: host.project_id(), docked: true };
        let title = t("git.commit_title");
        host.add_to_panel_keyed(
            TOOL,
            crate::commit_view::KEY,
            &title,
            crate::commit_view::EXTENSION,
            &data,
        );
    }
}

/// What a fixed tab made again at a start asks, once: the other fixed tabs there too.
pub(crate) fn missing_fixed_tabs() -> ui_components::ViewerAction {
    ui_components::ViewerAction::WithHost(ui_components::WithHost::once(add_missing_fixed_tabs))
}

/// The whole log shown (made if it isn't open). `false` without a repository.
fn show_log(host: &mut dyn WorkbenchHost) -> bool {
    show_log_of(host, None)
}

/// The log shown, `reference`'s history only when there's one (as chosen in its branches
/// pane), the Git window's other fixed tabs made too.
pub(crate) fn show_log_of(host: &mut dyn WorkbenchHost, reference: Option<String>) -> bool {
    let Some(repo) = host.repository() else { return false };
    let data = LogData { repo, path: None, commit: None, queries: host.queries(), reference };
    host.show_in_panel_keyed(TOOL, LOG_KEY, &t("git.log_tab"), EXTENSION, &data)
}

impl CommandHandler for ShowFileHistory {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some() && host.active_file().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(repo), Some(file)) = (host.repository(), host.active_file()) else {
            return false;
        };
        let Some(path) = version_control::relative_path(repo.root(), &file.path) else {
            host.notify(workbench_model::Notification::info(t("git.not_in_repository")));
            return false;
        };
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        open_fixed_tabs(host);
        let data = LogData {
            repo,
            path: Some(path),
            commit: None,
            queries: host.queries(),
            reference: None,
        };
        let title = tr!("git.file_history_tab", name = name.as_str());
        host.show_in_panel(TOOL, &title, EXTENSION, &data)
    }
}

impl CommandHandler for ShowLog {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        open_fixed_tabs(host);
        show_log(host)
    }
}

impl CommandHandler for ShowAnnotatedCommit {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some() && crate::decorations::annotated_commit(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(repo), Some(commit)) =
            (host.repository(), crate::decorations::annotated_commit(host))
        else {
            return false;
        };
        open_fixed_tabs(host);
        let data = LogData {
            repo,
            path: None,
            commit: Some(commit),
            queries: host.queries(),
            reference: None,
        };
        host.show_in_panel_keyed(TOOL, LOG_KEY, &t("git.log_tab"), EXTENSION, &data)
    }
}
