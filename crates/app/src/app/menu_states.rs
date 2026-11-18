//! The menu bar's rows greyed out as the window's state asks (see
//! `workbench::menu_states`): pushed to the native menu bar when that state changes — a
//! project opened or closed, a repository found, another tab, a tab closed, the history —
//! and only from the focused window (the bar is the app's, macOS shows the focused one's).

use super::*;

impl EditorApp {
    /// The window's state read again (cheap); changed, the rows that don't apply now go to
    /// the native menu bar.
    pub(super) fn sync_menu_states(&mut self) {
        if !self.window_state.focused {
            return;
        }
        let context = self.workbench.menu_context();
        if self.window_state.menu_context.as_ref() == Some(&context) {
            return;
        }
        let disabled = self.workbench.disabled_rows(&context);
        self.window_state.menu_context = Some(context);
        if let Some(window) = &self.window {
            window.set_menu_states(disabled.into_iter().map(|(id, _)| id).collect());
        }
    }

    /// File › Open Recent made again when what it lists changed (a project recorded, a
    /// file opened, the lists cleared) — the focused window's, as the bar is.
    pub(super) fn sync_recent_menu(&mut self) {
        if !self.window_state.focused {
            return;
        }
        let root = self.workbench.project_root();
        let files = self.workbench.recent_files();
        let key = (crate::recent::generation(), root.clone(), files.len(), files.last().cloned());
        if self.window_state.recent_menu.as_ref() == Some(&key) {
            return;
        }
        self.window_state.recent_menu = Some(key);
        if cmd::set_recent_menu(recent_menu(root.as_deref(), files)) {
            if let Some(window) = &self.window {
                window.set_menus(cmd::menus());
            }
            // Clear Recent greyed out or not: the rows' states pushed again.
            self.window_state.menu_context = None;
        }
    }
}

/// How many projects / files File › Open Recent lists at most.
const RECENT_ROWS: usize = 10;

/// What File › Open Recent lists for a window with project `root` and its recent `files`
/// (the latest last): the other recent projects (their paths, `~` for home), then the
/// project's files (relative to it), the latest first.
pub(super) fn recent_menu(
    root: Option<&std::path::Path>,
    files: &[std::path::PathBuf],
) -> cmd::RecentMenu {
    let projects = crate::recent::menu_projects(RECENT_ROWS, root)
        .into_iter()
        .map(|p| (p.display, p.path))
        .collect();
    let files = match root {
        Some(root) => files
            .iter()
            .rev()
            .filter_map(|f| Some((f.strip_prefix(root).ok()?.display().to_string(), f.clone())))
            .take(RECENT_ROWS)
            .collect(),
        None => Vec::new(),
    };
    cmd::RecentMenu { projects, files }
}

#[cfg(test)]
#[path = "../../tests/unit/app/menu_states.rs"]
mod tests;
