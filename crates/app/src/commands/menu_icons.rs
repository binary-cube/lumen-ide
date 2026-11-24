//! The icons of the commands in the menus: by id, or for the rows without a command of
//! their own (a bottom tab's, a recent file's, a module's listed row).

use super::*;

/// The icon for commands without a `CommandId` (bottom tabs, the rows modules list: "Compare
/// with ›" a tab).
pub fn dynamic_icon(id: &str) -> Option<&'static str> {
    if let Some((action, _)) = parse_tool_tab(id) {
        return Some(action.icon());
    }
    if id.starts_with(OPEN_RECENT_PROJECT) {
        return Some("folder");
    }
    if id.starts_with(OPEN_RECENT_FILE) {
        return Some("file-text");
    }
    let (listed, _) = id.split_once(':')?;
    LISTED.read().ok()?.iter().find(|(l, _)| *l == listed).and_then(|(_, icon)| *icon)
}

/// The icon of a command in the context menus (by id, without the `@…` argument).
pub fn menu_icon(id: &str) -> Option<&'static str> {
    if let Some(icon) = dynamic_icon(id) {
        return Some(icon);
    }
    if let Some(icon) = tool_window_icon(id) {
        return Some(icon);
    }
    let id = id.split_once('@').map_or(id, |(id, _)| id);
    if let Some((_, icon)) = contributed().into_iter().find(|(s, _)| s.id.0 == id) {
        return icon;
    }
    let icon = match SPECS.iter().find(|s| s.id.0 == id)?.id {
        TAB_CLOSE => "x",
        TAB_CLOSE_OTHERS => "x-square",
        TAB_CLOSE_LEFT => "arrow-left-to-line",
        TAB_CLOSE_RIGHT => "arrow-right-to-line",
        TAB_CLOSE_SAVED => "file-check",
        TAB_CLOSE_ALL => "layers-x",
        COPY_PATH => "copy",
        COPY_RELATIVE_PATH | COPY_PATH_MENU | TREE_COPY_MENU => "link",
        COPY_REFERENCE => "hash",
        SPLIT_RIGHT => "view-split",
        SPLIT_MOVE_RIGHT => "view-split-move",
        SPLIT_DOWN => "view-split-down",
        SPLIT_MOVE_DOWN => "view-split-move-down",
        TAB_PIN => "pin",
        TOGGLE_READ_ONLY => "lock",
        CHOOSE_LANGUAGE => "file-code",
        CHOOSE_ENCODING => "file-text",
        CHOOSE_LINE_ENDING => "text-wrap",
        CHOOSE_INDENT => "indent",
        TAB_NEW_WINDOW => "app-window",
        FULL_SCREEN | EDITOR_FULL => "maximize-2",
        EDITOR_FULL_WIDTH => "column",
        EDITOR_FULL_HEIGHT => "rows",
        WORD_WRAP => "text-wrap",
        REOPEN_CLOSED_TAB => "rotate-ccw",
        REVEAL_IN_FILE_MANAGER => "folder-open",
        REVEAL_IN_PROJECT => "crosshair",
        TAB_RENAME => "file-pen",
        COPY => "copy",
        CUT => "scissors",
        PASTE => "clipboard",
        COMMENT => "comment",
        GOTO_IMPLEMENTATION => "arrow-down",
        GOTO_TYPE => "tag",
        TYPE_HIERARCHY => "layers",
        SHOW_ACTIONS => "lightbulb",
        // The menu bar's rows (the menu drawer draws them).
        ABOUT => "info",
        OPEN_SETTINGS => "gear",
        NEW_FILE => "file",
        NEW_WINDOW => "app-window",
        OPEN_FILE => "file-text",
        OPEN_PROJECT => "folder-open",
        CLOSE_PROJECT => "folder-x",
        SAVE => "download",
        SAVE_AS => "file-pen",
        CLOSE_TAB => "x",
        CLEAR_CACHES => "refresh",
        UNDO => "undo",
        REDO => "redo",
        SELECT_ALL => "text-cursor",
        COMPLETE_CODE => "sparkles",
        FOLD_ALL => "chevrons-collapse",
        UNFOLD_ALL => "chevrons-expand",
        FIND | SEARCH_PROJECT => "search",
        SEARCH_FILES => "file",
        NAVIGATE_BACK => "arrow-left",
        NAVIGATE_FORWARD => "arrow-right",
        TOGGLE_PROJECT => "folder",
        ZOOM_IN => "plus",
        ZOOM_OUT => "minus",
        SWITCH_THEME => "palette",
        RECENT_NOTIFICATIONS => "bell",
        TOGGLE_DEBUG => "cpu",
        QUIT => "log-out",
        CLEAR_RECENT => "trash",
        KEYBOARD_SHORTCUTS => "keyboard",
        LOG_REVEAL_FOLDER | LOG_CHOOSE_FOLDER => "folder-open",
        DUPLICATE => "copy-plus",
        DELETE_LINE => "trash",
        MOVE_LINE_UP => "arrow-up",
        MOVE_LINE_DOWN => "arrow-down",
        MOVE_STATEMENT_UP => "chevron-up",
        MOVE_STATEMENT_DOWN => "chevron-down",
        EXTEND_SELECTION => "maximize-2",
        SHRINK_SELECTION => "minimize-2",
        SELECT_NEXT => "plus",
        UNSELECT_OCCURRENCE => "minus",
        SELECT_ALL_OCCURRENCES => "layers",
        ADD_CURSOR_UP | ADD_CURSOR_DOWN => "text-cursor",
        ZOOM_RESET => "rotate-ccw",
        CLOSE_NOTIFICATIONS => "x",
        REPLACE => "replace",
        FIND_NEXT => "arrow-down",
        FIND_PREV => "arrow-up",
        GOTO_LINE => "rows",
        GOTO_DEFINITION => "arrow-up-right",
        FOLD => "chevron-right",
        UNFOLD => "chevron-down",
        NEXT_TAB => "arrow-right",
        PREV_TAB => "arrow-left",
        NEXT_GROUP => "chevron-right",
        PREV_GROUP => "chevron-left",
        CLOSE_GROUP => "x-square",
        _ => return None,
    };
    Some(icon)
}

/// The app's own rows in the context menus (the modules bring theirs).
pub const CONTEXT_ITEMS: &[workbench_model::ContextMenuItem] = {
    use code_intelligence::LanguageFeature;
    use workbench_model::{ContextMenuItem as Item, MenuPlace, MenuSection, MenuWhen};
    const EDITOR: &[MenuPlace] = &[MenuPlace::Editor];
    const ENTRY: &[MenuPlace] = &[MenuPlace::TreeFile, MenuPlace::TreeFolder];
    &[
        // Where the file's language resolves names (none in plain text, nor in SQL).
        Item::new(GOTO_DEFINITION, EDITOR, MenuSection::Navigate, 10)
            .when(MenuWhen::Provides(LanguageFeature::Navigation)),
        Item::new(GOTO_IMPLEMENTATION, EDITOR, MenuSection::Navigate, 12)
            .when(MenuWhen::Provides(LanguageFeature::Navigation)),
        Item::new(GOTO_TYPE, EDITOR, MenuSection::Navigate, 14)
            .when(MenuWhen::Provides(LanguageFeature::Navigation)),
        Item::new(TYPE_HIERARCHY, EDITOR, MenuSection::Navigate, 16)
            .when(MenuWhen::Provides(LanguageFeature::TypeDiagram)),
        Item::new(CUT, EDITOR, MenuSection::Clipboard, 10),
        Item::new(COPY, EDITOR, MenuSection::Clipboard, 20),
        Item::new(PASTE, EDITOR, MenuSection::Clipboard, 30),
        Item::new(TREE_COPY_MENU, ENTRY, MenuSection::Path, 20),
    ]
};
