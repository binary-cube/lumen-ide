//! The app commands: a single list from which the menu and the keymap are built.

use command_model::{
    CommandId, CommandSpec, MenuAt, MenuSectionSpec, Platforms, Requires, SubmenuSpec,
};
use platform_api::{Menu, MenuItem, NativeItem};
use workbench_model::menus::{bar, sections as s, submenus};

mod entries;
mod menu_bar;
mod menu_icons;
mod recent;
mod tab_menu;
mod tool_tabs;

pub use self::entries::*;
pub use self::menu_bar::*;
pub use self::menu_icons::*;
pub use self::recent::*;
pub use self::tab_menu::*;
pub use self::tool_tabs::*;

pub const NEW_FILE: CommandId = CommandId("file.new");
pub const NEW_WINDOW: CommandId = CommandId("window.new");
pub const OPEN_FILE: CommandId = CommandId("file.open");
pub const OPEN_PROJECT: CommandId = CommandId("project.open");
/// The window's project closed (its session kept), back to the start screen.
pub const CLOSE_PROJECT: CommandId = CommandId("project.close");
/// A past session's windows stop reopening (those open stay).
pub const STOP_REOPEN: CommandId = CommandId("session.stop_reopen");
pub const SAVE: CommandId = CommandId("file.save");
pub const SAVE_AS: CommandId = CommandId("file.save_as");
pub const CLOSE_TAB: CommandId = workbench_model::CLOSE_TAB;
pub const NEXT_TAB: CommandId = CommandId("tab.next");
pub const PREV_TAB: CommandId = CommandId("tab.prev");
/// The split groups: the keyboard to the next / previous one, the current one closed —
/// the bottom window's when it has the keyboard, else the editor's.
pub const NEXT_GROUP: CommandId = CommandId("view.next_group");
pub const PREV_GROUP: CommandId = CommandId("view.prev_group");
pub const CLOSE_GROUP: CommandId = CommandId("view.close_group");
pub const QUIT: CommandId = CommandId("app.quit");
pub const OPEN_SETTINGS: CommandId = CommandId("app.settings");
pub const ABOUT: CommandId = CommandId("app.about");
pub const ZOOM_IN: CommandId = CommandId("view.zoom_in");
pub const ZOOM_OUT: CommandId = CommandId("view.zoom_out");
pub const ZOOM_RESET: CommandId = CommandId("view.zoom_reset");
pub const FULL_SCREEN: CommandId = CommandId("view.full_screen");
/// The editor area all the window (the side and bottom windows hidden), all its width, all
/// its height — or back as it was.
pub const EDITOR_FULL: CommandId = CommandId("view.editor_full");
pub const EDITOR_FULL_WIDTH: CommandId = CommandId("view.editor_full_width");
pub const EDITOR_FULL_HEIGHT: CommandId = CommandId("view.editor_full_height");
/// Lines longer than a view go on in rows under them, in every view showing text.
pub const WORD_WRAP: CommandId = CommandId("view.word_wrap");
/// This window kept above every other one (other programs' too), or back among them.
pub const KEEP_ON_TOP: CommandId = CommandId("window.keep_on_top");
/// The app's menus in the drawer from the window's bar (where it has the button: Windows,
/// Linux; macOS when asked for).
pub const MENU: CommandId = CommandId("view.menu");

pub const NAVIGATE_BACK: CommandId = CommandId("navigate.back");
pub const NAVIGATE_FORWARD: CommandId = CommandId("navigate.forward");

// A tab's context menu. They take the tab index as argument: `tab.close_others@3`.
pub const TAB_CLOSE: CommandId = CommandId("tab.ctx.close");
pub const TAB_PIN: CommandId = CommandId("tab.ctx.pin");
pub const TAB_RENAME: CommandId = CommandId("tab.ctx.rename");
pub const TAB_NEW_WINDOW: CommandId = CommandId("tab.ctx.new_window");
pub const SPLIT_RIGHT: CommandId = CommandId("tab.ctx.split_right");
pub const SPLIT_DOWN: CommandId = CommandId("tab.ctx.split_down");
pub const SPLIT_MOVE_RIGHT: CommandId = CommandId("tab.ctx.split_move_right");
pub const SPLIT_MOVE_DOWN: CommandId = CommandId("tab.ctx.split_move_down");
pub const COPY_REFERENCE: CommandId = CommandId("tab.ctx.copy_reference");
pub const REOPEN_CLOSED_TAB: CommandId = CommandId("tab.reopen_closed");
pub const TAB_CLOSE_OTHERS: CommandId = CommandId("tab.close_others");
pub const TAB_CLOSE_LEFT: CommandId = CommandId("tab.close_left");
pub const TAB_CLOSE_RIGHT: CommandId = CommandId("tab.close_right");
pub const TAB_CLOSE_SAVED: CommandId = CommandId("tab.close_saved");
pub const TAB_CLOSE_ALL: CommandId = CommandId("tab.close_all");
pub const COPY_PATH: CommandId = CommandId("file.copy_path");
pub const COPY_RELATIVE_PATH: CommandId = CommandId("file.copy_relative_path");
/// The "Copy" popover: every way to copy the file's path / reference, with previews.
pub const COPY_PATH_MENU: CommandId = CommandId("file.copy_path_menu");
pub const REVEAL_IN_FILE_MANAGER: CommandId = CommandId("file.reveal_in_file_manager");
pub const REVEAL_IN_PROJECT: CommandId = CommandId("file.reveal_in_project");
/// The active tab locked (read-only) or unlocked; from a tab's menu, that tab (`@doc`).
pub const TOGGLE_READ_ONLY: CommandId = CommandId("file.toggle_read_only");
/// The active file's format, chosen from a list (the status bar's file window, the palette).
pub const CHOOSE_LANGUAGE: CommandId = CommandId("file.choose_language");
pub const CHOOSE_ENCODING: CommandId = CommandId("file.choose_encoding");
pub const CHOOSE_LINE_ENDING: CommandId = CommandId("file.choose_line_ending");
pub const CHOOSE_INDENT: CommandId = CommandId("file.choose_indent");

pub const UNDO: CommandId = CommandId("edit.undo");
pub const REDO: CommandId = CommandId("edit.redo");
pub const CUT: CommandId = CommandId("edit.cut");
pub const COPY: CommandId = CommandId("edit.copy");
pub const PASTE: CommandId = CommandId("edit.paste");
pub const SELECT_ALL: CommandId = CommandId("edit.select_all");
pub const DUPLICATE: CommandId = CommandId("edit.duplicate");
pub const DELETE_LINE: CommandId = CommandId("edit.delete_line");
pub const SELECT_NEXT: CommandId = CommandId("edit.select_next_occurrence");
pub const SELECT_ALL_OCCURRENCES: CommandId = CommandId("edit.select_all_occurrences");
pub const UNSELECT_OCCURRENCE: CommandId = CommandId("edit.unselect_occurrence");
pub const ADD_CURSOR_UP: CommandId = CommandId("edit.add_cursor_up");
pub const MOVE_STATEMENT_UP: CommandId = CommandId("code.move_statement_up");
pub const MOVE_STATEMENT_DOWN: CommandId = CommandId("code.move_statement_down");
pub const MOVE_LINE_UP: CommandId = CommandId("code.move_line_up");
pub const MOVE_LINE_DOWN: CommandId = CommandId("code.move_line_down");
pub const ADD_CURSOR_DOWN: CommandId = CommandId("edit.add_cursor_down");

pub const COMMENT: CommandId = workbench_model::COMMENT_LINE;
pub const FOLD: CommandId = CommandId("code.fold");
pub const UNFOLD: CommandId = CommandId("code.unfold");
pub const FOLD_ALL: CommandId = CommandId("code.fold_all");
pub const UNFOLD_ALL: CommandId = CommandId("code.unfold_all");

pub const FIND: CommandId = CommandId("find.open");
pub const REPLACE: CommandId = CommandId("find.replace");
pub const FIND_NEXT: CommandId = CommandId("find.next");
pub const FIND_PREV: CommandId = CommandId("find.prev");

pub const TOGGLE_DEBUG: CommandId = CommandId("view.debug_overlay");
/// The app's own rows in the tree's menu (the tree module brings its operations).
pub const TREE_COPY_MENU: CommandId = CommandId("tree.copy_path_menu");
pub const CLEAR_CACHES: CommandId = CommandId("app.clear_caches");
pub const GOTO_DEFINITION: CommandId = workbench_model::GOTO_DECLARATION;
pub const GOTO_LINE: CommandId = CommandId("navigate.line");
pub const GOTO_IMPLEMENTATION: CommandId = CommandId("navigate.implementation");
pub const GOTO_TYPE: CommandId = CommandId("navigate.type_declaration");
pub const TYPE_HIERARCHY: CommandId = CommandId("navigate.type_hierarchy");
pub const SHOW_ACTIONS: CommandId = CommandId("code.show_actions");
pub const EXTEND_SELECTION: CommandId = CommandId("edit.extend_selection");
pub const SHRINK_SELECTION: CommandId = CommandId("edit.shrink_selection");
pub const COMPLETE_CODE: CommandId = CommandId("code.complete");
pub const SEARCH_FILES: CommandId = CommandId("project.search_files");
/// Implemented by a module (indexing): see `workbench_model::IMPLEMENTED`.
pub const SEARCH_PROJECT: CommandId = workbench_model::SEARCH_PROJECT;
pub const LANGUAGE_LEVEL: CommandId = CommandId("project.language_level");
/// The terminal module's toggle (bound differently by some keymap presets).
pub const TOGGLE_PROJECT: CommandId = workbench_model::TOGGLE_PROJECT_PANEL;
pub const SWITCH_THEME: CommandId = CommandId("view.switch_theme");
/// Closes every notification shown (and those waiting).
pub const CLOSE_NOTIFICATIONS: CommandId = CommandId("notifications.close");
/// The project's last notifications, in a list.
pub const RECENT_NOTIFICATIONS: CommandId = CommandId("notifications.recent");
/// File › Open Recent › Clear Recent: the recent projects outside the user's groups and
/// this project's recent files forgotten.
pub const CLEAR_RECENT: CommandId = CommandId("file.clear_recent");
/// Help › Keyboard Shortcuts…: Settings on the Keymap page.
pub const KEYBOARD_SHORTCUTS: CommandId = CommandId("help.keyboard_shortcuts");
/// The log's folder shown in the file manager (made first if it isn't there yet).
pub const LOG_REVEAL_FOLDER: CommandId = CommandId("help.reveal_log_folder");
/// Another folder for the log, picked (the log's settings).
pub const LOG_CHOOSE_FOLDER: CommandId = CommandId("log.choose_folder");
/// The prefix of File › Open Recent's project rows (and the app icon's menu):
/// `project.open_recent:<path>`.
pub const OPEN_RECENT_PROJECT: &str = "project.open_recent:";
/// The prefix of File › Open Recent's file rows: `file.open_recent:<path>`.
pub const OPEN_RECENT_FILE: &str = "file.open_recent:";

const fn spec(
    id: CommandId,
    label: &'static str,
    menu: Option<MenuAt>,
    shortcuts: &'static [&'static str],
) -> CommandSpec {
    CommandSpec { id, label, menu, shortcuts }
}

/// A place in section `section` of the menu bar.
const fn at(section: &'static str, order: u16) -> Option<MenuAt> {
    Some(MenuAt::new(section, order))
}

pub const SPECS: &[CommandSpec] = &[
    spec(ABOUT, "About {app}", at(s::APP_ABOUT, 10), &[]),
    spec(OPEN_SETTINGS, "Settings…", at(s::APP_SETTINGS, 10), &["CmdOrCtrl+,"]),
    spec(QUIT, "Quit {app}", at(s::APP_QUIT, 10), &["CmdOrCtrl+Q"]),
    spec(NEW_FILE, "New File", at(s::FILE_NEW, 10), &["CmdOrCtrl+N"]),
    spec(NEW_WINDOW, "New Window", at(s::FILE_NEW, 20), &["CmdOrCtrl+Shift+N"]),
    spec(OPEN_FILE, "Open File…", at(s::FILE_OPEN, 10), &["CmdOrCtrl+O"]),
    spec(OPEN_PROJECT, "Open Project…", at(s::FILE_OPEN, 20), &["CmdOrCtrl+Shift+O"]),
    // After Open Recent; no chord of its own (none is free in every preset): the menus,
    // the project chip's list, the palette.
    spec(CLOSE_PROJECT, "Close Project", at(s::FILE_OPEN, 40), &[]),
    spec(CLEAR_RECENT, "Clear Recent", at(s::RECENT_CLEAR, 10), &[]),
    spec(STOP_REOPEN, "Stop Reopening the Session", None, &[]),
    spec(SAVE, "Save", at(s::FILE_SAVE, 10), &["CmdOrCtrl+S"]),
    spec(SAVE_AS, "Save As…", at(s::FILE_SAVE, 20), &["CmdOrCtrl+Shift+S"]),
    // No chord of its own (none is free in every preset): the status bar's padlock, the
    // tab's menu, the palette.
    spec(TOGGLE_READ_ONLY, "Toggle Read-only Mode", at(s::FILE_SAVE, 30), &[]),
    spec(CHOOSE_LANGUAGE, "Choose the File's Language…", None, &[]),
    spec(CHOOSE_ENCODING, "Choose the File's Encoding…", None, &[]),
    spec(CHOOSE_LINE_ENDING, "Choose the File's Line Separator…", None, &[]),
    spec(CHOOSE_INDENT, "Choose the File's Indentation…", None, &[]),
    spec(CLOSE_TAB, "Close Tab", at(s::FILE_CLOSE, 10), &["CmdOrCtrl+W"]),
    spec(UNDO, "Undo", at(s::EDIT_HISTORY, 10), &["CmdOrCtrl+Z"]),
    spec(REDO, "Redo", at(s::EDIT_HISTORY, 20), &["CmdOrCtrl+Shift+Z", "CmdOrCtrl+Y"]),
    spec(CUT, "Cut", at(s::EDIT_CLIPBOARD, 10), &["CmdOrCtrl+X"]),
    spec(COPY, "Copy", at(s::EDIT_CLIPBOARD, 20), &["CmdOrCtrl+C"]),
    spec(PASTE, "Paste", at(s::EDIT_CLIPBOARD, 30), &["CmdOrCtrl+V"]),
    spec(SELECT_ALL, "Select All", at(s::EDIT_SELECT, 10), &["CmdOrCtrl+A"]),
    spec(DUPLICATE, "Duplicate Line or Selection", at(s::EDIT_LINES, 10), &["CmdOrCtrl+D"]),
    spec(DELETE_LINE, "Delete Line", at(s::EDIT_LINES, 20), &["CmdOrCtrl+Backspace"]),
    // ⌃G on macOS, Alt+J on Windows / Linux (where Ctrl+G is Find Next).
    spec(
        SELECT_NEXT,
        "Add Selection for Next Occurrence",
        at(s::SELECTION_OCCURRENCES, 10),
        &["mac:Ctrl+G", "Alt+J"],
    ),
    spec(
        SELECT_ALL_OCCURRENCES,
        "Select All Occurrences",
        at(s::SELECTION_OCCURRENCES, 30),
        &["CmdOrCtrl+Shift+L", "Ctrl+Alt+Shift+J"],
    ),
    spec(
        UNSELECT_OCCURRENCE,
        "Unselect Occurrence",
        at(s::SELECTION_OCCURRENCES, 20),
        &["mac:Ctrl+Shift+G", "Alt+Shift+J"],
    ),
    spec(ADD_CURSOR_UP, "Add Caret Above", at(s::SELECTION_CARETS, 10), &["CmdOrCtrl+Alt+Up"]),
    spec(ADD_CURSOR_DOWN, "Add Caret Below", at(s::SELECTION_CARETS, 20), &["CmdOrCtrl+Alt+Down"]),
    // ⌥↑ / ⌥↓ on macOS.
    spec(EXTEND_SELECTION, "Extend Selection", at(s::SELECTION_EXTEND, 10), &["Alt+Up"]),
    spec(SHRINK_SELECTION, "Shrink Selection", at(s::SELECTION_EXTEND, 20), &["Alt+Down"]),
    spec(COMPLETE_CODE, "Complete Code", at(s::CODE_ASSIST, 10), &["Ctrl+Space"]),
    // ⌥⏎; ⌘. as the language servers' "Show Code Actions" had it.
    spec(
        SHOW_ACTIONS,
        "Show Context Actions",
        at(s::CODE_ASSIST, 20),
        &["Alt+Enter", "CmdOrCtrl+."],
    ),
    spec(COMMENT, "Comment with Line Comment", at(s::CODE_EDIT, 10), &["CmdOrCtrl+/"]),
    spec(MOVE_STATEMENT_UP, "Move Statement Up", at(s::CODE_EDIT, 20), &["CmdOrCtrl+Shift+Up"]),
    spec(
        MOVE_STATEMENT_DOWN,
        "Move Statement Down",
        at(s::CODE_EDIT, 30),
        &["CmdOrCtrl+Shift+Down"],
    ),
    spec(MOVE_LINE_UP, "Move Line Up", at(s::EDIT_LINES, 30), &["Alt+Shift+Up"]),
    spec(MOVE_LINE_DOWN, "Move Line Down", at(s::EDIT_LINES, 40), &["Alt+Shift+Down"]),
    spec(FOLD, "Collapse", at(s::FOLDING_ONE, 10), &["CmdOrCtrl+-"]),
    spec(UNFOLD, "Expand", at(s::FOLDING_ONE, 20), &["CmdOrCtrl+="]),
    // The keypad's − / + (the main row's are Zoom Out / In).
    spec(FOLD_ALL, "Collapse All", at(s::FOLDING_ALL, 10), &["CmdOrCtrl+Shift+NumpadSubtract"]),
    spec(UNFOLD_ALL, "Expand All", at(s::FOLDING_ALL, 20), &["CmdOrCtrl+Shift+NumpadAdd"]),
    spec(FIND, "Find…", at(s::FIND_FIND, 10), &["CmdOrCtrl+F"]),
    spec(REPLACE, "Replace…", at(s::FIND_FIND, 20), &["CmdOrCtrl+R"]),
    spec(FIND_NEXT, "Find Next", at(s::FIND_NEXT, 10), &["CmdOrCtrl+G", "F3"]),
    spec(FIND_PREV, "Find Previous", at(s::FIND_NEXT, 20), &["CmdOrCtrl+Shift+G", "Shift+F3"]),
    spec(NAVIGATE_BACK, "Back", at(s::NAVIGATE_HISTORY, 10), &["CmdOrCtrl+[", "Ctrl+Alt+Left"]),
    spec(
        NAVIGATE_FORWARD,
        "Forward",
        at(s::NAVIGATE_HISTORY, 20),
        &["CmdOrCtrl+]", "Ctrl+Alt+Right"],
    ),
    spec(CLEAR_CACHES, "Clear and Rebuild Caches", at(s::TOOLS_MAINTENANCE, 20), &[]),
    spec(TREE_COPY_MENU, "Copy Path / Reference…", None, &[]),
    spec(GOTO_DEFINITION, "Go to Declaration", at(s::NAVIGATE_DECLARATIONS, 10), &["CmdOrCtrl+B"]),
    spec(
        GOTO_IMPLEMENTATION,
        "Go to Implementation",
        at(s::NAVIGATE_DECLARATIONS, 20),
        &["CmdOrCtrl+Alt+B"],
    ),
    spec(
        GOTO_TYPE,
        "Go to Type Declaration",
        at(s::NAVIGATE_DECLARATIONS, 30),
        &["CmdOrCtrl+Shift+B"],
    ),
    spec(TYPE_HIERARCHY, "Type Hierarchy", at(s::NAVIGATE_HIERARCHY, 10), &["Ctrl+H"]),
    spec(GOTO_LINE, "Go to Line…", at(s::NAVIGATE_GOTO, 20), &["CmdOrCtrl+L"]),
    spec(SEARCH_FILES, "Go to File…", at(s::NAVIGATE_GOTO, 10), &["CmdOrCtrl+P"]),
    spec(SEARCH_PROJECT, "Search in Project…", at(s::FIND_PROJECT, 10), &["CmdOrCtrl+Shift+F"]),
    spec(LANGUAGE_LEVEL, "Language Level…", None, &[]),
    spec(REVEAL_IN_PROJECT, "Select in Project View", at(s::NAVIGATE_REVEAL, 10), &["Alt+F1"]),
    spec(TAB_CLOSE, "Close", None, &[]),
    spec(TAB_PIN, "Pin Tab", None, &[]),
    spec(TAB_RENAME, "Rename File…", None, &["Shift+F6"]),
    spec(TAB_NEW_WINDOW, "Open in New Window", None, &[]),
    spec(SPLIT_RIGHT, "Split Right", None, &[]),
    spec(SPLIT_DOWN, "Split Down", None, &[]),
    spec(SPLIT_MOVE_RIGHT, "Split and Move Right", None, &[]),
    spec(SPLIT_MOVE_DOWN, "Split and Move Down", None, &[]),
    spec(COPY_REFERENCE, "Copy Reference", None, &["CmdOrCtrl+Alt+Shift+C"]),
    spec(REOPEN_CLOSED_TAB, "Reopen Closed Tab", at(s::FILE_CLOSE, 20), &["CmdOrCtrl+Shift+T"]),
    spec(TAB_CLOSE_OTHERS, "Close Others", at(s::WINDOW_CLOSE, 10), &[]),
    spec(TAB_CLOSE_LEFT, "Close Tabs to the Left", None, &[]),
    spec(TAB_CLOSE_RIGHT, "Close Tabs to the Right", None, &[]),
    spec(TAB_CLOSE_SAVED, "Close Saved Tabs", at(s::WINDOW_CLOSE, 20), &[]),
    spec(TAB_CLOSE_ALL, "Close All Tabs", at(s::WINDOW_CLOSE, 30), &[]),
    spec(COPY_PATH, "Copy Path", None, &["CmdOrCtrl+Shift+C"]),
    spec(COPY_RELATIVE_PATH, "Copy Relative Path", None, &[]),
    spec(COPY_PATH_MENU, "Copy Path / Reference…", at(s::EDIT_CLIPBOARD, 40), &["CmdOrCtrl+Alt+C"]),
    spec(REVEAL_IN_FILE_MANAGER, "Reveal in File Manager", None, &[]),
    spec(NEXT_TAB, "Next Tab", at(s::WINDOW_TABS, 10), &["CmdOrCtrl+Shift+]", "Ctrl+Tab"]),
    spec(
        PREV_TAB,
        "Previous Tab",
        at(s::WINDOW_TABS, 20),
        &["CmdOrCtrl+Shift+[", "Ctrl+Shift+Tab"],
    ),
    spec(NEXT_GROUP, "Next Group", at(s::WINDOW_TABS, 30), &["CmdOrCtrl+Alt+Shift+PageDown"]),
    spec(PREV_GROUP, "Previous Group", at(s::WINDOW_TABS, 40), &["CmdOrCtrl+Alt+Shift+PageUp"]),
    spec(CLOSE_GROUP, "Close Group", None, &[]),
    spec(TOGGLE_PROJECT, "Project", None, &["CmdOrCtrl+1", "Alt+1"]),
    spec(ZOOM_IN, "Zoom In", at(s::VIEW_ZOOM, 10), &["CmdOrCtrl+Shift+="]),
    spec(ZOOM_OUT, "Zoom Out", at(s::VIEW_ZOOM, 20), &["CmdOrCtrl+Shift+-"]),
    spec(ZOOM_RESET, "Actual Size (100%)", at(s::VIEW_ZOOM, 30), &["CmdOrCtrl+Shift+0"]),
    // ⌃⌘F is macOS' own (elsewhere it would be Find's Ctrl+F).
    spec(FULL_SCREEN, "Full Screen", at(s::VIEW_SCREEN, 10), &["F11", "mac:Ctrl+CmdOrCtrl+F"]),
    // Hides every tool window at once.
    spec(
        EDITOR_FULL,
        "Editor Area: All the Window",
        at(s::VIEW_SCREEN, 20),
        &["CmdOrCtrl+Shift+F12"],
    ),
    spec(EDITOR_FULL_WIDTH, "Editor Area: All the Width", at(s::VIEW_SCREEN, 30), &[]),
    spec(EDITOR_FULL_HEIGHT, "Editor Area: All the Height", at(s::VIEW_SCREEN, 40), &[]),
    spec(WORD_WRAP, "Word Wrap", at(s::VIEW_ZOOM, 40), &["Alt+Z"]),
    // No chord of its own (none is free in every preset): the top bar's pin, the menus,
    // the palette.
    spec(KEEP_ON_TOP, "Keep Window on Top", at(s::WINDOW_LEVEL, 10), &[]),
    // ⌘⇧T is "Reopen Closed Tab"; with the terminal focused, it opens a terminal tab.
    spec(SWITCH_THEME, "Switch Theme", at(s::APPEARANCE_THEME, 10), &["CmdOrCtrl+Alt+T"]),
    spec(CLOSE_NOTIFICATIONS, "Close Notifications", at(s::VIEW_NOTIFICATIONS, 20), &[]),
    spec(RECENT_NOTIFICATIONS, "Show Recent Notifications", at(s::VIEW_NOTIFICATIONS, 10), &[]),
    spec(KEYBOARD_SHORTCUTS, "Keyboard Shortcuts…", at(s::HELP_KEYS, 10), &[]),
    spec(TOGGLE_DEBUG, "Diagnostic Overlay", at(s::HELP_TOOLS, 10), &["F12"]),
    spec(LOG_REVEAL_FOLDER, "Reveal Log Folder", at(s::HELP_TOOLS, 20), &[]),
    spec(LOG_CHOOSE_FOLDER, "Choose Log Folder…", None, &[]),
    // Not in a menu: it opens them (and Alt alone, on Windows and Linux).
    spec(MENU, "Menu", None, &["F10"]),
];

/// What the app's commands need of the window (`Requires`): without it their menu rows are
/// greyed out and their keys do nothing. The modules' handlers say theirs
/// (`CommandHandler::requires`, then `enabled`).
const REQUIRES: &[(CommandId, &[Requires])] = {
    use Requires as R;
    const TEXT: &[Requires] = &[R::TextFile];
    &[
        (SAVE, &[R::Tab]),
        (SAVE_AS, &[R::Tab]),
        (CLOSE_TAB, &[R::Tab]),
        (TOGGLE_READ_ONLY, &[R::Tab, R::Lockable]),
        (CHOOSE_LANGUAGE, TEXT),
        (CHOOSE_ENCODING, TEXT),
        (CHOOSE_LINE_ENDING, TEXT),
        (CHOOSE_INDENT, TEXT),
        (REOPEN_CLOSED_TAB, &[R::ClosedTab]),
        (COPY_PATH_MENU, &[R::FileOnDisk]),
        (FIND, &[R::Tab]),
        (REPLACE, TEXT),
        (FIND_NEXT, &[R::Tab]),
        (FIND_PREV, &[R::Tab]),
        (CLOSE_PROJECT, &[R::Project]),
        (SEARCH_PROJECT, &[R::Project]),
        (SEARCH_FILES, &[R::Project]),
        (REVEAL_IN_PROJECT, &[R::Project, R::FileOnDisk]),
        (NAVIGATE_BACK, &[R::BackHistory]),
        (NAVIGATE_FORWARD, &[R::ForwardHistory]),
        (GOTO_LINE, TEXT),
        (GOTO_DEFINITION, TEXT),
        (GOTO_IMPLEMENTATION, TEXT),
        (GOTO_TYPE, TEXT),
        (TYPE_HIERARCHY, TEXT),
        (COMPLETE_CODE, TEXT),
        (SHOW_ACTIONS, TEXT),
        (COMMENT, TEXT),
        (MOVE_STATEMENT_UP, TEXT),
        (MOVE_STATEMENT_DOWN, TEXT),
        (MOVE_LINE_UP, TEXT),
        (MOVE_LINE_DOWN, TEXT),
        (DUPLICATE, TEXT),
        (DELETE_LINE, TEXT),
        (FOLD, TEXT),
        (UNFOLD, TEXT),
        (FOLD_ALL, TEXT),
        (UNFOLD_ALL, TEXT),
        (SELECT_NEXT, TEXT),
        (SELECT_ALL_OCCURRENCES, TEXT),
        (UNSELECT_OCCURRENCE, TEXT),
        (ADD_CURSOR_UP, TEXT),
        (ADD_CURSOR_DOWN, TEXT),
        (EXTEND_SELECTION, TEXT),
        (SHRINK_SELECTION, TEXT),
        (NEXT_TAB, &[R::OtherTabs]),
        (PREV_TAB, &[R::OtherTabs]),
        (TAB_CLOSE_OTHERS, &[R::OtherTabs]),
        (TAB_CLOSE_SAVED, &[R::Tab]),
        (TAB_CLOSE_ALL, &[R::Tab]),
        (KEEP_ON_TOP, &[R::KeepOnTop]),
    ]
};

/// What app command `id` needs of the window (none: it always applies).
pub fn requires(id: CommandId) -> &'static [Requires] {
    REQUIRES.iter().find(|(c, _)| *c == id).map_or(&[], |(_, r)| r)
}

/// Why a command is greyed out, translated (`None`: no reason to tell).
pub fn disabled_hint(missing: Option<Requires>) -> Option<String> {
    missing.map(|r| i18n::t(r.hint_key()))
}

/// A text from the locales (`[command]`, `[menu]`), the app's name filled in; `fallback`
/// (English) if the key is missing.
fn localized(key: &str, fallback: &str) -> String {
    let text = i18n::t(key);
    kernel::brand::named(if text == key { fallback } else { &text })
}

/// A command's label in the current language (`command.<id>` in the locales).
pub fn label(id: CommandId) -> String {
    let fallback = spec_of(id).map_or(id.0, |s| s.label);
    localized(&format!("command.{}", id.0.replace('.', "_")), fallback)
}

/// Where a command is in the menus: its menu's title and its submenus' (`["Code",
/// "Folding"]`; a tool window's toggle: View › Tool Windows); empty if it's in none.
pub fn menu_path(id: CommandId) -> Vec<String> {
    let tree = MenuTree::new(System::current());
    match spec_of(id).and_then(|s| s.menu) {
        Some(at) => tree.path(at.section),
        None if tool_windows().iter().any(|t| t.toggle_command() == Some(id.0)) => {
            let row = SUBMENUS.iter().find(|s| s.id == submenus::TOOL_WINDOWS);
            row.map_or_else(Vec::new, |row| {
                let mut path = tree.path(row.at.section);
                path.push(localized(row.label_key, row.label));
                path
            })
        }
        None => Vec::new(),
    }
}

/// The prefix of the rows showing a tool window without a toggle of its own
/// (`tool-window:git`).
pub const TOOL_WINDOW_PREFIX: &str = "tool-window:";

/// The tool windows, as registered (set once when the modules load), for View › Tool
/// Windows.
static TOOL_WINDOWS: std::sync::RwLock<Vec<std::sync::Arc<dyn ui_workbench::ToolWindowProvider>>> =
    std::sync::RwLock::new(Vec::new());

/// Registers the tool windows (the modules' and the app's own).
pub fn set_tool_windows(tools: Vec<std::sync::Arc<dyn ui_workbench::ToolWindowProvider>>) {
    // As the commands: only added to, whichever modules load.
    if let Ok(mut all) = TOOL_WINDOWS.write() {
        merge_by(&mut all, tools, |t| t.id());
        all.sort_by_key(|t| (t.dock(), t.strip_end(), t.order()));
    }
}

fn tool_windows() -> Vec<std::sync::Arc<dyn ui_workbench::ToolWindowProvider>> {
    TOOL_WINDOWS.read().map(|t| t.clone()).unwrap_or_default()
}

/// The command of a tool window's row in View › Tool Windows: its toggle, else showing it.
fn tool_window_row(tool: &dyn ui_workbench::ToolWindowProvider) -> String {
    tool.toggle_command()
        .map_or_else(|| format!("{TOOL_WINDOW_PREFIX}{}", tool.id()), str::to_owned)
}

/// The icon of a tool window's row (`tool-window:<id>` or its toggle).
fn tool_window_icon(id: &str) -> Option<&'static str> {
    tool_windows().into_iter().find(|t| tool_window_row(t.as_ref()) == id).map(|t| t.icon())
}

/// [`menu_path`] as one text (`Code › Folding`; `None`: in no menu).
pub fn menu_place(id: CommandId) -> Option<String> {
    let path = menu_path(id);
    (!path.is_empty()).then(|| path.join(" › "))
}

/// The "reveal in the file manager" action, named after the system's file manager.
pub fn file_manager_label(manager: platform_api::FileManager) -> String {
    use platform_api::FileManager;
    match manager {
        FileManager::Finder => localized("command.reveal_finder", "Reveal in Finder"),
        FileManager::Explorer => localized("command.reveal_explorer", "Show in Explorer"),
        FileManager::Other => localized("command.reveal_folder", "Open Containing Folder"),
    }
}

/// The commands contributed by modules (their spec and context-menu icon), set once when
/// the modules load.
static CONTRIBUTED: std::sync::RwLock<Vec<(CommandSpec, Option<&'static str>)>> =
    std::sync::RwLock::new(Vec::new());

/// Registers the modules' commands (menus, shortcuts, labels).
pub fn set_contributed(commands: Vec<(CommandSpec, Option<&'static str>)>) {
    // A command's spec is its module's whichever modules load: only added to (see
    // `keymaps::set_places`).
    if let Ok(mut list) = CONTRIBUTED.write() {
        merge_by(&mut list, commands, |(spec, _)| spec.id);
    }
}

/// `new` into `all` by `key`: an entry with the same key replaced in place, the others
/// appended; nothing of `all` taken away. What modules register describes them, not the
/// selection that loaded them: registered again with fewer (another window's, a test's),
/// the process-wide lists stay whole.
pub fn merge_by<T, K: PartialEq>(all: &mut Vec<T>, new: Vec<T>, key: impl Fn(&T) -> K) {
    for item in new {
        let k = key(&item);
        match all.iter_mut().find(|old| key(old) == k) {
            Some(old) => *old = item,
            None => all.push(item),
        }
    }
}

/// The modules' commands.
pub fn contributed() -> Vec<(CommandSpec, Option<&'static str>)> {
    CONTRIBUTED.read().map(|l| l.clone()).unwrap_or_default()
}

/// A command's spec: the app's own or a module's.
pub fn spec_of(id: CommandId) -> Option<CommandSpec> {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .or_else(|| contributed().into_iter().map(|(s, _)| s).find(|s| s.id == id))
}

#[cfg(test)]
#[path = "../../tests/unit/commands.rs"]
mod tests;
