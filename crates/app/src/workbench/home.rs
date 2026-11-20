//! The editor's empty screen and the projects list (the same content), with the recent
//! projects they show.

use super::*;

/// The empty screen, the projects modal and the recent projects.
#[derive(Debug, Default)]
pub(super) struct HomeScreen {
    /// The editor's empty screen (the projects, or an open project's actions and files).
    pub picker: ProjectsPicker,
    /// The empty screen's files (recent or found) and what they were made for: rebuilt
    /// when the query, the project's files or the theme change, not on every frame.
    pub files: Option<(HomeKey, Option<HomeFiles>)>,
    /// Projects: the modal (from the top bar), with the empty screen's content.
    pub projects_menu: ProjectsPicker,
    pub recent: ui_workbench::RecentList,
    /// What the projects list's right-click menu acts on.
    pub recent_target: Option<ui_workbench::ProjectsTarget>,
    /// The projects list's question being asked (a group's name).
    pub recent_ask: Option<project_groups::RecentAsk>,
    /// The recent projects' generation (`recent::generation`) `recent` was read at.
    pub recent_generation: Option<u64>,
    /// What `recent.sessions` was built for: the theme's generation (the files' icons),
    /// the language, the minute (`2 hours ago`).
    pub sessions_key: Option<(u64, String, u64)>,
}

impl Workbench {
    /// The app's past sessions as the projects list shows them (plan 78): read at start,
    /// they don't change; their rows are made again only when the theme (the files' icons),
    /// the language or the minute (`2 hours ago`) changes.
    pub(super) fn sync_home_sessions(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let key = (self.theme_generation, i18n::locale(), now / 60);
        if self.home.sessions_key.as_ref() == Some(&key) {
            return;
        }
        self.home.sessions_key = Some(key);
        let past = crate::sessions::past();
        let theme = self.theme_generation;
        let (support, icons, palette) = (&self.support, &self.icons, &self.palette);
        let file_icons = &mut self.file_icons;
        let mut file = |path: &Path, root: Option<&Path>| {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let parent = path.parent().unwrap_or(path);
            let detail = match root.and_then(|r| parent.strip_prefix(r).ok()) {
                Some(inside) => inside.display().to_string(),
                None => disk::home_relative(parent),
            };
            let icon = Some(file_icons.get(support, icons, palette, theme, &name));
            ui_workbench::HomeFile { name, detail, path: path.to_path_buf(), icon }
        };
        let sessions = past
            .list
            .iter()
            .map(|session| ui_workbench::HomeSession {
                id: session.id,
                when: i18n::ago(i64::try_from(now.saturating_sub(session.updated)).unwrap_or(0)),
                crashed: !session.clean,
                windows: session
                    .windows
                    .iter()
                    .map(|window| {
                        let root = window.project.as_deref();
                        let (name, detail) = match root {
                            Some(root) => (
                                root.file_name().map_or_else(
                                    || root.display().to_string(),
                                    |n| n.to_string_lossy().into_owned(),
                                ),
                                disk::home_relative(root),
                            ),
                            None => (i18n::t("home.session_files_alone"), String::new()),
                        };
                        ui_workbench::HomeSessionWindow {
                            name,
                            detail,
                            project: root.is_some(),
                            path: root.map(Path::to_path_buf),
                            files: window.files.iter().map(|f| file(f, root)).collect(),
                        }
                    })
                    .collect(),
            })
            .collect();
        self.home.recent.sessions = sessions;
    }
}
