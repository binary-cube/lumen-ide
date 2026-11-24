//! An editor tab's context menu, and the rows modules list in menus (grouped, with their
//! icons).

use super::*;

/// The rows modules list in menus: their ids and icons.
pub(super) static LISTED: std::sync::RwLock<Vec<(&'static str, Option<&'static str>)>> =
    std::sync::RwLock::new(Vec::new());

/// The modules' listed rows (their ids, their icons), as they contributed them.
pub fn set_listed(listed: Vec<(&'static str, Option<&'static str>)>) {
    if let Ok(mut list) = LISTED.write() {
        merge_by(&mut list, listed, |(label, _)| *label);
    }
}

/// `items` as submenu `label` (inline when there's one; nothing when there's none).
pub fn group(
    label: String,
    icon: Option<&'static str>,
    items: Vec<MenuItem>,
) -> Vec<ui_workbench::MenuEntry> {
    let entries = menu_entries(items);
    let commands = entries.iter().filter(|e| !matches!(e, ui_workbench::MenuEntry::Separator));
    match commands.count() {
        0 => Vec::new(),
        1 => entries
            .into_iter()
            .filter(|e| !matches!(e, ui_workbench::MenuEntry::Separator))
            .collect(),
        _ => vec![ui_workbench::MenuEntry::Submenu { label, icon, entries }],
    }
}

/// What a tab's context menu depends on: the tab count and the tab's state.
#[derive(Debug, Clone, Copy)]
pub struct TabMenuState {
    pub count: usize,
    pub has_path: bool,
    pub in_project: bool,
    pub pinned: bool,
    pub can_reopen: bool,
    /// The tab can be locked (read-only): whether it is (`None`: it can't be).
    pub locked: Option<bool>,
    /// Its view can be renamed (a session's terminal): no file's rows, its own rename.
    pub renamable: bool,
}

/// A tab's context menu: Close › (this tab, the others…), the path, Split ›, Compare
/// with › (`compare`: the other tabs, the clipboard), the tab's own actions, revealing it.
/// Each command carries the tab's document (`@doc`).
pub fn tab_context_menu(
    (tab, doc): (usize, u64),
    state: TabMenuState,
    file_manager: &str,
    compare: Vec<MenuItem>,
) -> Vec<ui_workbench::MenuEntry> {
    let TabMenuState { count, has_path, in_project, pinned, can_reopen, locked, renamable } = state;
    // A renamable view's tab has a name, not a file.
    let has_path = has_path && !renamable;
    use ui_workbench::MenuEntry::Separator;
    let item = |id: CommandId, label: &str| {
        let shortcuts = crate::keymaps::shortcuts(id);
        MenuItem::command(&format!("{}@{doc}", id.0), label, shortcuts.first().map(String::as_str))
    };
    // Every close under Close › (this one alone when it's the only tab).
    let mut closes = vec![item(TAB_CLOSE, &label(TAB_CLOSE))];
    if count > 1 {
        closes.push(item(TAB_CLOSE_OTHERS, &label(TAB_CLOSE_OTHERS)));
        closes.push(MenuItem::Separator);
        if tab > 0 {
            closes.push(item(TAB_CLOSE_LEFT, &label(TAB_CLOSE_LEFT)));
        }
        if tab + 1 < count {
            closes.push(item(TAB_CLOSE_RIGHT, &label(TAB_CLOSE_RIGHT)));
        }
        closes.push(MenuItem::Separator);
        closes.push(item(TAB_CLOSE_SAVED, &label(TAB_CLOSE_SAVED)));
        closes.push(item(TAB_CLOSE_ALL, &label(TAB_CLOSE_ALL)));
    }
    let mut entries = group(i18n::t("menu.close"), Some("x"), closes);
    if has_path {
        entries.push(Separator);
        entries.extend(menu_entries(vec![
            item(COPY_PATH, &label(COPY_PATH)),
            item(COPY_PATH_MENU, &label(COPY_PATH_MENU)),
        ]));
        // Split: the file also opens in the new group (or moves there, if there are other tabs).
        entries.push(Separator);
        let mut splits = vec![item(SPLIT_RIGHT, &label(SPLIT_RIGHT))];
        if count > 1 {
            splits.push(item(SPLIT_MOVE_RIGHT, &label(SPLIT_MOVE_RIGHT)));
        }
        splits.push(item(SPLIT_DOWN, &label(SPLIT_DOWN)));
        if count > 1 {
            splits.push(item(SPLIT_MOVE_DOWN, &label(SPLIT_MOVE_DOWN)));
        }
        entries.extend(group(i18n::t("menu.split"), Some("view-split"), splits));
        entries.extend(group(i18n::t("menu.compare_with"), Some("compare"), compare));
    }
    entries.push(Separator);
    let pin = if pinned { localized("command.unpin_tab", "Unpin Tab") } else { label(TAB_PIN) };
    let mut own = vec![item(TAB_PIN, &pin)];
    if renamable {
        own.push(item(TAB_RENAME, &localized("command.rename_tab", "Rename Tab…")));
    }
    // Lock (Read-only) / Unlock: only where it can be.
    if let Some(locked) = locked {
        let lock = if locked {
            localized("command.unlock_tab", "Unlock")
        } else {
            localized("command.lock_tab", "Lock (Read-only)")
        };
        own.push(item(TOGGLE_READ_ONLY, &lock));
    }
    if has_path {
        own.push(item(TAB_NEW_WINDOW, &label(TAB_NEW_WINDOW)));
    }
    if can_reopen {
        let shortcuts = crate::keymaps::shortcuts(REOPEN_CLOSED_TAB);
        let shortcut = shortcuts.first().map(String::as_str);
        own.push(MenuItem::command(REOPEN_CLOSED_TAB.0, &label(REOPEN_CLOSED_TAB), shortcut));
    }
    entries.extend(menu_entries(own));
    if has_path {
        entries.push(Separator);
        let mut reveal = vec![item(REVEAL_IN_FILE_MANAGER, file_manager)];
        if in_project {
            reveal.push(item(REVEAL_IN_PROJECT, &label(REVEAL_IN_PROJECT)));
        }
        reveal.push(MenuItem::Separator);
        reveal.push(item(TAB_RENAME, &label(TAB_RENAME)));
        entries.extend(menu_entries(reveal));
    }
    entries
}
