//! File › Open Recent: the recent projects and files the focused window sets, as rows.

use super::*;

/// What File › Open Recent lists: the recent projects and this window's project's recent
/// files (their labels and paths), set by the focused window when they change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentMenu {
    pub projects: Vec<(String, std::path::PathBuf)>,
    pub files: Vec<(String, std::path::PathBuf)>,
}

impl RecentMenu {
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty() && self.files.is_empty()
    }
}

#[cfg(not(test))]
static RECENT_MENU: std::sync::RwLock<RecentMenu> =
    std::sync::RwLock::new(RecentMenu { projects: Vec::new(), files: Vec::new() });

#[cfg(test)]
thread_local! {
    // Each test its own (the menus' golden sees an empty one).
    static RECENT_MENU: std::sync::RwLock<RecentMenu> = const {
        std::sync::RwLock::new(RecentMenu { projects: Vec::new(), files: Vec::new() })
    };
}

fn with_recent_menu<R>(f: impl FnOnce(&std::sync::RwLock<RecentMenu>) -> R) -> R {
    #[cfg(not(test))]
    return f(&RECENT_MENU);
    #[cfg(test)]
    RECENT_MENU.with(f)
}

/// What File › Open Recent lists from now on; `true` if it changed (the menus to make
/// again).
pub fn set_recent_menu(menu: RecentMenu) -> bool {
    with_recent_menu(|lock| {
        let mut current = lock.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current == menu {
            return false;
        }
        *current = menu;
        true
    })
}

/// File › Open Recent lists nothing (Clear Recent greyed out).
pub fn recent_menu_is_empty() -> bool {
    with_recent_menu(|lock| lock.read().map_or(true, |m| m.is_empty()))
}

/// The rows of a section of File › Open Recent made from the lists (none for the others).
pub(super) fn recent_rows(section: &str) -> Vec<MenuItem> {
    let projects = match section {
        s::RECENT_PROJECTS => true,
        s::RECENT_FILES => false,
        _ => return Vec::new(),
    };
    with_recent_menu(|lock| {
        let menu = lock.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (prefix, rows) = if projects {
            (OPEN_RECENT_PROJECT, &menu.projects)
        } else {
            (OPEN_RECENT_FILE, &menu.files)
        };
        rows.iter()
            .map(|(label, path)| {
                MenuItem::command(&format!("{prefix}{}", path.display()), label, None)
            })
            .collect()
    })
}

/// The prefix of a bottom window tab's menu commands: `tool-tab:<action>:<index>`.
pub const TOOL_TAB_PREFIX: &str = "tool-tab:";
