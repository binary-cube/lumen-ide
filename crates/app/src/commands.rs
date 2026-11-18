//! The app commands: a single list from which the menu and the keymap are built.

use command_model::{
    CommandId, CommandSpec, MenuAt, MenuSectionSpec, Platforms, Requires, SubmenuSpec,
};
use platform_api::{Menu, MenuItem, NativeItem};
use workbench_model::menus::{bar, sections as s, submenus};

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
    // ⌘⇧T is "Reopen Closed Tab"; with the terminal focused, it opens a terminal tab.
    spec(SWITCH_THEME, "Switch Theme", at(s::APPEARANCE_THEME, 10), &["CmdOrCtrl+Alt+T"]),
    spec(CLOSE_NOTIFICATIONS, "Close Notifications", at(s::VIEW_NOTIFICATIONS, 20), &[]),
    spec(RECENT_NOTIFICATIONS, "Show Recent Notifications", at(s::VIEW_NOTIFICATIONS, 10), &[]),
    spec(KEYBOARD_SHORTCUTS, "Keyboard Shortcuts…", at(s::HELP_KEYS, 10), &[]),
    spec(TOGGLE_DEBUG, "Diagnostic Overlay", at(s::HELP_TOOLS, 10), &["F12"]),
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

/// A top-level menu of the menu bar: its id (its sections name it), its title (`menu.<id>`
/// in the locales, `title` when missing; `{app}`: named after the app, see
/// `kernel::brand`), its icon where the app draws the menus (the menu drawer), the systems
/// it's on.
#[derive(Debug, Clone, Copy)]
struct TopMenu {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    platforms: Platforms,
    /// The app's window list (macOS adds the open windows to it and to the Dock's menu).
    windows: bool,
    /// The Help menu (macOS puts its menus' search in it).
    help: bool,
}

const fn top(id: &'static str, title: &'static str, icon: &'static str) -> TopMenu {
    TopMenu { id, title, icon, platforms: Platforms::All, windows: false, help: false }
}

/// The menu bar, in order. A menu shows only with rows in it (Git,
/// its module on); the application menu only on macOS (elsewhere its rows are in File and
/// Help).
const MENUS: &[TopMenu] = &[
    TopMenu { platforms: Platforms::MacOs, ..top(bar::APP, "{app}", "info") },
    top(bar::FILE, "File", "folder"),
    top(bar::EDIT, "Edit", "file-pen"),
    top(bar::VIEW, "View", "eye"),
    top(bar::FIND, "Find", "search"),
    top(bar::NAVIGATE, "Navigate", "compass"),
    top(bar::CODE, "Code", "braces"),
    top(bar::REFACTOR, "Refactor", "wand"),
    top(bar::GIT, "Git", "git-branch"),
    top(bar::TOOLS, "Tools", "wrench"),
    TopMenu { windows: true, ..top(bar::WINDOW, "Window", "app-window") },
    TopMenu { help: true, ..top(bar::HELP, "Help", "circle-help") },
];

/// A section of the app's, everywhere.
const fn section(id: &'static str, menu: &'static str, order: u16) -> MenuSectionSpec {
    MenuSectionSpec::new(id, menu, order)
}

/// The app's sections of the menus, each in its menu, in order (a separator between those
/// shown). Modules put their commands in them by id (`workbench_model::menus::sections`)
/// and may bring sections of their own (`MenuSectionSpec` contributions). About, Settings
/// and Quit are the application menu's on macOS (its HIG); elsewhere Help's and File's.
const SECTIONS: &[MenuSectionSpec] = &[
    section(s::APP_ABOUT, bar::APP, 10).on(Platforms::MacOs),
    section(s::APP_SETTINGS, bar::APP, 20).on(Platforms::MacOs),
    section(s::APP_SERVICES, bar::APP, 30).on(Platforms::MacOs),
    section(s::APP_HIDE, bar::APP, 40).on(Platforms::MacOs),
    section(s::APP_QUIT, bar::APP, 50).on(Platforms::MacOs),
    section(s::FILE_NEW, bar::FILE, 10),
    section(s::FILE_OPEN, bar::FILE, 20),
    section(s::RECENT_PROJECTS, submenus::OPEN_RECENT, 10),
    section(s::RECENT_FILES, submenus::OPEN_RECENT, 20),
    section(s::RECENT_CLEAR, submenus::OPEN_RECENT, 30),
    section(s::FILE_SAVE, bar::FILE, 30),
    section(s::FILE_CLOSE, bar::FILE, 40),
    section(s::FILE_HISTORY, bar::FILE, 50),
    section(s::APP_SETTINGS, bar::FILE, 60).on(Platforms::Others),
    section(s::APP_QUIT, bar::FILE, 70).on(Platforms::Others),
    section(s::EDIT_HISTORY, bar::EDIT, 10),
    section(s::EDIT_CLIPBOARD, bar::EDIT, 20),
    section(s::EDIT_SELECT, bar::EDIT, 30),
    section(s::EDIT_LINES, bar::EDIT, 40),
    section(s::SELECTION_EXTEND, submenus::SELECTION, 10),
    section(s::SELECTION_OCCURRENCES, submenus::SELECTION, 20),
    section(s::SELECTION_CARETS, submenus::SELECTION, 30),
    section(s::VIEW_WINDOWS, bar::VIEW, 10),
    section(s::VIEW_APPEARANCE, bar::VIEW, 20),
    section(s::APPEARANCE_THEME, submenus::APPEARANCE, 10),
    section(s::VIEW_ZOOM, bar::VIEW, 30),
    section(s::VIEW_SCREEN, bar::VIEW, 40),
    section(s::VIEW_NOTIFICATIONS, bar::VIEW, 50),
    section(s::FIND_FIND, bar::FIND, 10),
    section(s::FIND_NEXT, bar::FIND, 20),
    section(s::FIND_PROJECT, bar::FIND, 30),
    section(s::NAVIGATE_HISTORY, bar::NAVIGATE, 10),
    section(s::NAVIGATE_GOTO, bar::NAVIGATE, 20),
    section(s::NAVIGATE_DECLARATIONS, bar::NAVIGATE, 30),
    section(s::NAVIGATE_HIERARCHY, bar::NAVIGATE, 40),
    section(s::NAVIGATE_REVEAL, bar::NAVIGATE, 50),
    section(s::CODE_ASSIST, bar::CODE, 10),
    section(s::CODE_EDIT, bar::CODE, 20),
    section(s::CODE_FOLDING, bar::CODE, 30),
    section(s::FOLDING_ONE, submenus::FOLDING, 10),
    section(s::FOLDING_ALL, submenus::FOLDING, 20),
    section(s::CODE_FORMAT, bar::CODE, 40),
    section(s::CODE_CHECK, bar::CODE, 50),
    section(s::REFACTOR_CHANGE, bar::REFACTOR, 10),
    section(s::REFACTOR_UNDO, bar::REFACTOR, 20),
    section(s::TOOLS_DATA, bar::TOOLS, 10),
    section(s::TOOLS_LANGUAGES, bar::TOOLS, 20),
    section(s::TOOLS_MAINTENANCE, bar::TOOLS, 30),
    section(s::WINDOW_ARRANGE, bar::WINDOW, 10).on(Platforms::MacOs),
    section(s::WINDOW_TABS, bar::WINDOW, 20),
    section(s::WINDOW_CLOSE, bar::WINDOW, 30),
    section(s::WINDOW_FRONT, bar::WINDOW, 40).on(Platforms::MacOs),
    section(s::HELP_KEYS, bar::HELP, 5),
    section(s::HELP_TOOLS, bar::HELP, 10),
    section(s::APP_ABOUT, bar::HELP, 20).on(Platforms::Others),
];

/// The app's submenus: rows opening sections of their own (Tool Windows: made from the
/// registered tool windows).
const SUBMENUS: &[SubmenuSpec] = &[
    SubmenuSpec {
        id: submenus::OPEN_RECENT,
        at: MenuAt::new(s::FILE_OPEN, 30),
        label_key: "menu.open_recent",
        label: "Open Recent",
        icon: Some("clock"),
    },
    SubmenuSpec {
        id: submenus::SELECTION,
        at: MenuAt::new(s::EDIT_SELECT, 20),
        label_key: "menu.selection",
        label: "Selection",
        icon: Some("text-cursor"),
    },
    SubmenuSpec {
        id: submenus::TOOL_WINDOWS,
        at: MenuAt::new(s::VIEW_WINDOWS, 10),
        label_key: "menu.tool_windows",
        label: "Tool Windows",
        icon: Some("panel-right"),
    },
    SubmenuSpec {
        id: submenus::APPEARANCE,
        at: MenuAt::new(s::VIEW_APPEARANCE, 10),
        label_key: "menu.appearance",
        label: "Appearance",
        icon: Some("palette"),
    },
    SubmenuSpec {
        id: submenus::FOLDING,
        at: MenuAt::new(s::CODE_FOLDING, 10),
        label_key: "menu.folding",
        label: "Folding",
        icon: Some("chevrons-collapse"),
    },
];

/// The system's own rows (macOS): where each is, its label's key and English fallback.
const NATIVE: &[(MenuAt, NativeItem, &str, &str)] = &[
    (MenuAt::new(s::APP_SERVICES, 10), NativeItem::Services, "menu.native_services", "Services"),
    (MenuAt::new(s::APP_HIDE, 10), NativeItem::Hide, "menu.native_hide", "Hide {app}"),
    (
        MenuAt::new(s::APP_HIDE, 20),
        NativeItem::HideOthers,
        "menu.native_hide_others",
        "Hide Others",
    ),
    (MenuAt::new(s::APP_HIDE, 30), NativeItem::ShowAll, "menu.native_show_all", "Show All"),
    (MenuAt::new(s::WINDOW_ARRANGE, 10), NativeItem::Minimize, "menu.native_minimize", "Minimize"),
    (MenuAt::new(s::WINDOW_ARRANGE, 20), NativeItem::Zoom, "menu.native_zoom", "Zoom"),
    (
        MenuAt::new(s::WINDOW_FRONT, 10),
        NativeItem::BringAllToFront,
        "menu.native_bring_all_to_front",
        "Bring All to Front",
    ),
];

/// The sections and submenus contributed by modules (set once when the modules load).
static CONTRIBUTED_MENUS: std::sync::RwLock<(Vec<MenuSectionSpec>, Vec<SubmenuSpec>)> =
    std::sync::RwLock::new((Vec::new(), Vec::new()));

/// Registers the modules' sections and submenus of the menu bar.
pub fn set_contributed_menus(sections: Vec<MenuSectionSpec>, submenus: Vec<SubmenuSpec>) {
    // As the commands: only added to, whichever modules load.
    if let Ok(mut menus) = CONTRIBUTED_MENUS.write() {
        merge_by(&mut menus.0, sections, |s| (s.id, s.platforms));
        merge_by(&mut menus.1, submenus, |s| s.id);
    }
}

/// The systems the menus are made for (a label or two differ between Windows and Linux).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    MacOs,
    Windows,
    Linux,
}

impl System {
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::MacOs, Self::Windows, Self::Linux];

    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub fn mac(self) -> bool {
        self == Self::MacOs
    }
}

/// The menu bar on this system.
pub fn menus() -> Vec<Menu> {
    menus_on(System::current())
}

/// The menu bar on `system`: every top-level menu with rows, each made of its sections.
pub fn menus_on(system: System) -> Vec<Menu> {
    let tree = MenuTree::new(system);
    MENUS
        .iter()
        .filter(|m| m.platforms.includes(system.mac()))
        .map(|m| Menu {
            title: menu_title(m),
            items: tree.items(m.id),
            windows: m.windows,
            icon: Some(m.icon),
            help: m.help,
        })
        // A module's menu (e.g. Git) only when its module is on.
        .filter(|m| !m.items.is_empty())
        .collect()
}

/// A top-level menu's title, translated.
fn menu_title(menu: &TopMenu) -> String {
    if menu.title == "{app}" {
        kernel::brand::named(menu.title)
    } else {
        localized(&format!("menu.{}", menu.id), menu.title)
    }
}

/// What the menus are made of on one system: the sections, the submenus, the commands (the
/// app's, then the modules').
struct MenuTree {
    system: System,
    sections: Vec<MenuSectionSpec>,
    submenus: Vec<SubmenuSpec>,
    commands: Vec<CommandSpec>,
}

impl MenuTree {
    fn new(system: System) -> Self {
        let (mut sections, mut submenus) = (SECTIONS.to_vec(), SUBMENUS.to_vec());
        if let Ok(contributed) = CONTRIBUTED_MENUS.read() {
            sections.extend_from_slice(&contributed.0);
            submenus.extend_from_slice(&contributed.1);
        }
        sections.retain(|s| s.platforms.includes(system.mac()));
        let modules = contributed().into_iter().map(|(spec, _)| spec);
        let commands = SPECS.iter().cloned().chain(modules).collect();
        Self { system, sections, submenus, commands }
    }

    /// The rows of menu (or submenu) `menu`: its sections in order, a separator between
    /// those with rows; in each, its commands, submenus and the system's rows by their
    /// order (equal: the app's first, then as declared).
    fn items(&self, menu: &str) -> Vec<MenuItem> {
        if menu == submenus::TOOL_WINDOWS {
            return self.tool_windows();
        }
        let nested = !MENUS.iter().any(|m| m.id == menu);
        let mut sections: Vec<&MenuSectionSpec> =
            self.sections.iter().filter(|s| s.menu == menu).collect();
        sections.sort_by_key(|s| s.order);
        let mut out = Vec::new();
        for section in sections {
            let mut rows: Vec<(u16, Vec<MenuItem>)> = Vec::new();
            for spec in &self.commands {
                if let Some(at) = spec.menu.filter(|at| at.section == section.id) {
                    let shortcuts = crate::keymaps::shortcuts_on(spec.id, self.system.mac());
                    let shortcut = shortcuts.first().map(String::as_str);
                    let label = self.row_label(spec.id, nested);
                    rows.push((at.order, vec![MenuItem::command(spec.id.0, &label, shortcut)]));
                }
            }
            for (order, row) in recent_rows(section.id).into_iter().enumerate() {
                rows.push((order as u16, vec![row]));
            }
            for submenu in self.submenus.iter().filter(|s| s.at.section == section.id) {
                rows.push((submenu.at.order, self.submenu(submenu)));
            }
            if self.system.mac() {
                for (at, item, key, fallback) in NATIVE.iter().filter(|n| n.0.section == section.id)
                {
                    let label = localized(key, fallback);
                    rows.push((at.order, vec![MenuItem::Native { item: *item, label }]));
                }
            }
            rows.sort_by_key(|(order, _)| *order);
            let rows: Vec<MenuItem> = rows.into_iter().flat_map(|(_, items)| items).collect();
            if rows.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(MenuItem::Separator);
            }
            out.extend(rows);
        }
        out
    }

    /// A command's row label: in a submenu its short one if it has one (`menu_label.<id>`:
    /// "Compare With › Last Commit"); Quit is "Exit" on Windows.
    fn row_label(&self, id: CommandId, nested: bool) -> String {
        if id == QUIT && self.system == System::Windows {
            return localized("command.app_exit", "Exit");
        }
        let key = format!("menu_label.{}", id.0.replace('.', "_"));
        match i18n::t(&key) {
            short if nested && short != key => kernel::brand::named(&short),
            _ => label(id),
        }
    }

    /// View › Tool Windows: every registered tool window (but the top bar's, and one whose
    /// command has a place of its own: the canvas, in Navigate), by dock in the strips'
    /// order, a separator between docks and between a strip's top and bottom groups; each
    /// runs its toggle (or shows the window).
    fn tool_windows(&self) -> Vec<MenuItem> {
        let placed =
            |command: &str| self.commands.iter().any(|c| c.id.0 == command && c.menu.is_some());
        let mut out = Vec::new();
        let mut dock = None;
        for tool in tool_windows() {
            if tool.dock() == ui_workbench::Dock::Top || tool.toggle_command().is_some_and(placed) {
                continue;
            }
            let group = (tool.dock(), tool.strip_end());
            if dock.is_some_and(|d| d != group) {
                out.push(MenuItem::Separator);
            }
            dock = Some(group);
            let shortcuts = tool.toggle_command().map_or_else(Vec::new, |c| {
                crate::keymaps::shortcuts_on(CommandId(c), self.system.mac())
            });
            let id = tool_window_row(tool.as_ref());
            out.push(MenuItem::command(&id, &tool.title(), shortcuts.first().map(String::as_str)));
        }
        out
    }

    /// A submenu's row: none when it has no rows, its only row in its place when it has one.
    fn submenu(&self, submenu: &SubmenuSpec) -> Vec<MenuItem> {
        let items = self.items(submenu.id);
        let rows = items.iter().filter(|i| **i != MenuItem::Separator).count();
        // Open Recent stays a submenu (Clear Recent alone, greyed out, when it's empty).
        let rows = if submenu.id == submenus::OPEN_RECENT { rows.max(2) } else { rows };
        match rows {
            0 => Vec::new(),
            1 => items.into_iter().filter(|i| *i != MenuItem::Separator).collect(),
            _ => vec![MenuItem::Submenu {
                title: localized(submenu.label_key, submenu.label),
                icon: submenu.icon,
                items,
            }],
        }
    }

    /// The titles from the top-level menu down to section `section`'s menu.
    fn path(&self, section: &str) -> Vec<String> {
        let Some(menu) = self.sections.iter().find(|s| s.id == section).map(|s| s.menu) else {
            return Vec::new();
        };
        if let Some(top) = MENUS.iter().find(|m| m.id == menu) {
            return vec![menu_title(top)];
        }
        match self.submenus.iter().find(|s| s.id == menu) {
            Some(submenu) => {
                let mut path = self.path(submenu.at.section);
                path.push(localized(submenu.label_key, submenu.label));
                path
            }
            None => Vec::new(),
        }
    }
}

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
fn recent_rows(section: &str) -> Vec<MenuItem> {
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

/// An action of a bottom window tab's menu (any window's: the terminal's shells, Git's
/// views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTabAction {
    Rename,
    TogglePin,
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseAll,
    SplitRight,
    SplitDown,
    SplitMoveRight,
    SplitMoveDown,
}

impl ToolTabAction {
    const ALL: [(Self, &'static str); 11] = [
        (Self::Rename, "rename"),
        (Self::TogglePin, "pin"),
        (Self::Close, "close"),
        (Self::CloseOthers, "close-others"),
        (Self::CloseLeft, "close-left"),
        (Self::CloseRight, "close-right"),
        (Self::CloseAll, "close-all"),
        (Self::SplitRight, "split-right"),
        (Self::SplitDown, "split-down"),
        (Self::SplitMoveRight, "split-move-right"),
        (Self::SplitMoveDown, "split-move-down"),
    ];

    fn name(self) -> &'static str {
        Self::ALL.iter().find(|(a, _)| *a == self).map_or("", |(_, n)| n)
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Rename => "file-pen",
            Self::TogglePin => "pin",
            Self::Close | Self::CloseLeft | Self::CloseRight | Self::CloseAll => "x",
            Self::CloseOthers => "x-square",
            Self::SplitRight => "view-split",
            Self::SplitMoveRight => "view-split-move",
            Self::SplitDown => "view-split-down",
            Self::SplitMoveDown => "view-split-move-down",
        }
    }
}

/// A bottom window's tab, as its menu sees it: its place among the `count` of its group,
/// pinned, closable (a fixed one isn't).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolTab {
    pub index: usize,
    pub count: usize,
    pub pinned: bool,
    pub closable: bool,
}

/// The context menu of a bottom window's tab: rename, pin, the closes (on its group; a
/// fixed tab has no Close of its own), the splits — another view in a new group when the
/// window makes them (`makes_views`: the terminal's shells), or this one moved there when
/// its group has others.
pub fn tool_tab_menu(tab: ToolTab, makes_views: bool) -> Vec<MenuItem> {
    use ToolTabAction as A;
    let ToolTab { index, count, pinned, closable } = tab;
    let item = |action: A, label: &str| {
        MenuItem::command(&format!("{TOOL_TAB_PREFIX}{}:{index}", action.name()), label, None)
    };
    let pin = if pinned {
        localized("command.unpin_tab", "Unpin Tab")
    } else {
        localized("command.tab_ctx_pin", "Pin Tab")
    };
    let mut items = vec![
        item(A::Rename, &localized("command.rename", "Rename…")),
        item(A::TogglePin, &pin),
        MenuItem::Separator,
    ];
    if closable {
        items.push(item(A::Close, &label(TAB_CLOSE)));
    }
    if count > 1 {
        items.push(item(
            A::CloseOthers,
            &localized("command.close_other_views", "Close Other Tabs"),
        ));
        if index > 0 {
            items.push(item(A::CloseLeft, &label(TAB_CLOSE_LEFT)));
        }
        if index + 1 < count {
            items.push(item(A::CloseRight, &label(TAB_CLOSE_RIGHT)));
        }
        items.push(item(A::CloseAll, &label(TAB_CLOSE_ALL)));
    }
    let mut splits = Vec::new();
    for (split, moved) in [(A::SplitRight, A::SplitMoveRight), (A::SplitDown, A::SplitMoveDown)] {
        let (id, moved_id) = match split {
            A::SplitRight => (SPLIT_RIGHT, SPLIT_MOVE_RIGHT),
            _ => (SPLIT_DOWN, SPLIT_MOVE_DOWN),
        };
        if makes_views {
            splits.push(item(split, &label(id)));
        }
        if count > 1 {
            splits.push(item(moved, &label(moved_id)));
        }
    }
    if !splits.is_empty() {
        items.push(MenuItem::Separator);
        items.extend(splits);
    }
    items
}

/// `tool-tab:<action>:<index>` → (action, index).
pub fn parse_tool_tab(id: &str) -> Option<(ToolTabAction, usize)> {
    let (name, index) = id.strip_prefix(TOOL_TAB_PREFIX)?.split_once(':')?;
    let action = ToolTabAction::ALL.iter().find(|(_, n)| *n == name)?.0;
    Some((action, index.parse().ok()?))
}

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

/// The rows modules list in menus: their ids and icons.
static LISTED: std::sync::RwLock<Vec<(&'static str, Option<&'static str>)>> =
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

/// The command and its argument (the tab index) from an `id@arg` identifier.
pub fn parse_with_arg(id: &str) -> Option<(CommandId, u64)> {
    let (id, arg) = id.split_once('@')?;
    Some((by_id(id)?, arg.parse().ok()?))
}

/// Finds the command by the identifier received from the native menu.
pub fn by_id(id: &str) -> Option<CommandId> {
    SPECS
        .iter()
        .find(|s| s.id.0 == id)
        .map(|s| s.id)
        .or_else(|| contributed().into_iter().map(|(s, _)| s.id).find(|c| c.0 == id))
}

/// Every command with its keys, for Settings → Shortcuts: its label, its menu (or where it
/// works), its shortcuts (as written and as shown), whether the user changed them; in menu
/// order, then the views' actions.
pub fn keymap_bindings() -> std::sync::Arc<Vec<ui_workbench::BindingRow>> {
    use command_model::When;
    let rows = crate::keymaps::bindings().into_iter().map(|b| {
        let (place, place_name) = match b.when {
            When::Everywhere => (String::new(), None),
            When::Editor => ("editor".to_owned(), Some(i18n::t("shortcuts.place_editor"))),
            When::EditorLanguage(language) => (
                format!("editor-{language}"),
                Some(i18n::tr!(
                    "shortcuts.place_editor_language",
                    language = language.to_uppercase()
                )),
            ),
            When::View(view) => {
                let key = format!("shortcuts.view_{}", view.replace('.', "_"));
                let name = i18n::t(&key);
                (view.to_owned(), Some(if name == key { view.to_owned() } else { name }))
            }
        };
        ui_workbench::BindingRow {
            command: b.command.0.to_owned(),
            label: label(b.command).trim_end_matches('…').to_owned(),
            group: place_name.or_else(|| menu_place(b.command)).unwrap_or_default(),
            place,
            shown: b.shortcuts.iter().map(|s| display_shortcut(s)).collect(),
            shortcuts: b.shortcuts,
            changed: b.origin == command_model::bindings::Origin::User,
        }
    });
    // In the menus' order (a module's command where its menu puts it: Theme Studio before
    // Quit), then the rest as they were.
    fn commands(items: Vec<MenuItem>, out: &mut Vec<String>) {
        for item in items {
            match item {
                MenuItem::Command { id, .. } => out.push(id),
                MenuItem::Submenu { items, .. } => commands(items, out),
                MenuItem::Separator | MenuItem::Native { .. } => {}
            }
        }
    }
    let mut order = Vec::new();
    for menu in menus() {
        commands(menu.items, &mut order);
    }
    let mut rows: Vec<ui_workbench::BindingRow> = rows.collect();
    rows.sort_by_key(|r| order.iter().position(|id| *id == r.command).unwrap_or(usize::MAX));
    std::sync::Arc::new(rows)
}

/// The menus as the menu drawer shows them (the native menu bar's, in its order): each
/// with its icon, its rows with theirs and their shortcuts as the system shows them.
/// `hidden`: the rows left out (what makes sense only elsewhere — another language's file);
/// the separators and submenus left empty go with them.
pub fn drawer_menus(hidden: &dyn Fn(&str) -> bool) -> Vec<ui_workbench::DrawerMenu> {
    menus()
        .into_iter()
        .map(|menu| ui_workbench::DrawerMenu {
            title: menu.title,
            icon: menu.icon,
            entries: ui_workbench::tidy_menu(without(
                one_level_of_submenus(menu_entries(menu.items)),
                hidden,
            )),
        })
        .filter(|menu| !menu.entries.is_empty())
        .collect()
}

/// `entries` without the rows `hidden` says (in submenus too).
fn without(
    entries: Vec<ui_workbench::MenuEntry>,
    hidden: &dyn Fn(&str) -> bool,
) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::MenuEntry;
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item { ref id, .. } if hidden(id) => None,
            MenuEntry::Submenu { label, icon, entries } => {
                Some(MenuEntry::Submenu { label, icon, entries: without(entries, hidden) })
            }
            entry => Some(entry),
        })
        .collect()
}

/// The menus' submenus as the drawer shows them (its third panel): their own submenus'
/// rows in their place.
fn one_level_of_submenus(entries: Vec<ui_workbench::MenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::MenuEntry;
    entries
        .into_iter()
        .map(|entry| match entry {
            MenuEntry::Submenu { label, icon, entries } => {
                MenuEntry::Submenu { label, icon, entries: inline_submenus(entries) }
            }
            entry => entry,
        })
        .collect()
}

/// The submenus' rows in their place, between separators.
fn inline_submenus(entries: Vec<ui_workbench::MenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::MenuEntry;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            MenuEntry::Submenu { entries, .. } => {
                out.push(MenuEntry::Separator);
                out.extend(inline_submenus(entries));
                out.push(MenuEntry::Separator);
            }
            entry => out.push(entry),
        }
    }
    out
}

/// Menu items as our context menu's rows (icons and shortcuts as the system shows them);
/// the system's own rows are left out.
pub fn menu_entries(items: Vec<MenuItem>) -> Vec<ui_workbench::MenuEntry> {
    items
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Command { id, label, shortcut, .. } => {
                let icon = menu_icon(&id);
                let shortcut = shortcut.as_deref().map(display_shortcut);
                Some(ui_workbench::MenuEntry::Item {
                    id,
                    label,
                    shortcut,
                    icon,
                    on: None,
                    look: Default::default(),
                })
            }
            MenuItem::Separator => Some(ui_workbench::MenuEntry::Separator),
            MenuItem::Submenu { title, icon, items } => Some(ui_workbench::MenuEntry::Submenu {
                label: title,
                icon,
                entries: menu_entries(items),
            }),
            MenuItem::Native { .. } => None,
        })
        .collect()
}

/// The command's first shortcut, as the system shows it: `⌘⇧F` on macOS, `Ctrl+Shift+F`
/// elsewhere (for tooltips).
pub fn shortcut_label(id: CommandId) -> Option<String> {
    crate::keymaps::shortcuts(id).first().map(|s| display_shortcut(s))
}

/// A shortcut as written in the specs (`CmdOrCtrl+Shift+C`), as the system shows it.
pub fn display_shortcut(shortcut: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let parts: Vec<String> = shortcut
        .split('+')
        .map(|part| match (part, mac) {
            ("CmdOrCtrl", true) | ("Cmd", true) => "⌘".into(),
            ("CmdOrCtrl", false) | ("Ctrl", false) => "Ctrl".into(),
            ("Ctrl", true) => "⌃".into(),
            ("Shift", true) => "⇧".into(),
            ("Alt", true) => "⌥".into(),
            ("Left", _) => "←".into(),
            ("Right", _) => "→".into(),
            ("Up", _) => "↑".into(),
            ("Down", _) => "↓".into(),
            (key, _) if key.eq_ignore_ascii_case("NumpadAdd") => "NumPad+".into(),
            (key, _) if key.eq_ignore_ascii_case("NumpadSubtract") => "NumPad-".into(),
            (other, _) => other.to_owned(),
        })
        .collect();
    if mac { parts.concat() } else { parts.join("+") }
}

#[cfg(test)]
#[path = "../tests/unit/commands.rs"]
mod tests;
