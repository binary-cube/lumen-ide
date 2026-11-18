//! Menus contributed by modules: a submenu in the editor's / the tree's right-click menu
//! (`Git ›`, `Local History ›`) and, optionally, a button in the left tool strip opening it;
//! and the rows of those menus themselves ([`ContextMenuItem`]: Rename, Cut, Delete…), each
//! module's and the app's own, arranged by section. Rows are commands (labels, icons and
//! shortcuts from their contributions); a command that isn't enabled where the menu opens
//! is left out.

use std::sync::Arc;

use command_model::CommandId;

use crate::{ActionContext, WorkbenchHost};

/// Where a contributed menu (or one of its rows) appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuPlace {
    /// The editor's right-click menu (acting on the active file).
    Editor,
    /// The project tree's right-click menu on a file (acting on that file).
    TreeFile,
    /// The project tree's right-click menu on a folder.
    TreeFolder,
    /// A button at the bottom of the left tool strip, opening the menu.
    ToolStrip,
    /// A tab's right-click menu (acting on its file).
    Tab,
}

/// Every place a contributed submenu can be in.
pub const EVERYWHERE: &[MenuPlace] =
    &[MenuPlace::Editor, MenuPlace::TreeFile, MenuPlace::TreeFolder, MenuPlace::ToolStrip];

/// A row of a contributed menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRow {
    /// A command, shown in these places.
    Command(CommandId, &'static [MenuPlace]),
    Separator,
}

/// A module's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuContribution {
    /// Its id (the tool strip button's too).
    pub id: &'static str,
    /// Translated with `t(label_key)`, the spec's fallback being `label`.
    pub label_key: &'static str,
    pub icon: &'static str,
    pub places: &'static [MenuPlace],
    pub rows: Vec<MenuRow>,
}

impl MenuContribution {
    /// The rows shown in `place`, given which commands are enabled there: separators only
    /// between shown commands (none leading, trailing or doubled).
    pub fn rows_in(&self, place: MenuPlace, enabled: &dyn Fn(CommandId) -> bool) -> Vec<MenuRow> {
        if !self.places.contains(&place) {
            return Vec::new();
        }
        let mut out: Vec<MenuRow> = Vec::new();
        let mut pending_separator = false;
        for row in &self.rows {
            match *row {
                MenuRow::Separator => pending_separator = !out.is_empty(),
                MenuRow::Command(id, places) if places.contains(&place) && enabled(id) => {
                    if pending_separator {
                        out.push(MenuRow::Separator);
                        pending_separator = false;
                    }
                    out.push(*row);
                }
                MenuRow::Command(..) => {}
            }
        }
        out
    }
}

/// The sections of a context menu, in this order, a separator between them; some show as a
/// submenu gathering every contribution to them ("New ›", "Compare with ›").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MenuSection {
    /// What the file's own tool does with it (a query console's Execute, Explain Plan):
    /// first.
    Run,
    New,
    Refactor,
    Navigate,
    /// What changes the code in place (Comment).
    Code,
    Clipboard,
    Path,
    Compare,
    /// How the entry counts for the index ("Mark As").
    Marks,
    /// What can't be taken back lightly (Delete): last.
    Danger,
}

impl MenuSection {
    /// The submenu the section shows as: its label's key and icon (`None`: inline).
    pub fn submenu(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::New => Some(("menu.new", "plus")),
            Self::Compare => Some(("menu.compare_with", "compare")),
            Self::Marks => Some(("menu.mark_as", "folder")),
            _ => None,
        }
    }
}

/// Where a context menu's row applies, besides its command being enabled: the menu's file
/// (the editor's, or the entry right-clicked) as the app sees it. The app knows no language:
/// a module names its own, or a feature its intelligence declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuWhen {
    /// Wherever its command is enabled.
    Always,
    /// The file is in language `id` (`"sql"`), as [`command_model::When::EditorLanguage`].
    Language(&'static str),
    /// The file's language offers this (its intelligence says so; a file without any
    /// offers nothing).
    Provides(code_intelligence::LanguageFeature),
    /// While this other command is enabled (a console's rows: while the file is a console).
    With(CommandId),
}

/// A row a module (or the app) puts among a context menu's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextMenuItem {
    pub command: CommandId,
    pub places: &'static [MenuPlace],
    pub section: MenuSection,
    /// Its place in the section (lower first; equal: as contributed).
    pub order: u16,
    /// Its label's key there (`None`: the command's label).
    pub label_key: Option<&'static str>,
    /// The shortcut shown there, as specs write it (`CmdOrCtrl+X`), when it isn't the
    /// command's own (the editor's clipboard keys acting on the tree's entries).
    pub shortcut: Option<&'static str>,
    /// Where it applies besides its command being enabled.
    pub when: MenuWhen,
}

impl ContextMenuItem {
    pub const fn new(
        command: CommandId,
        places: &'static [MenuPlace],
        section: MenuSection,
        order: u16,
    ) -> Self {
        Self {
            command,
            places,
            section,
            order,
            label_key: None,
            shortcut: None,
            when: MenuWhen::Always,
        }
    }

    /// Shown only where `when` holds.
    pub const fn when(self, when: MenuWhen) -> Self {
        Self { when, ..self }
    }

    /// Labeled `t(key)` there.
    pub const fn label(self, key: &'static str) -> Self {
        Self { label_key: Some(key), ..self }
    }

    /// Showing `shortcut` there.
    pub const fn shortcut(self, shortcut: &'static str) -> Self {
        Self { shortcut: Some(shortcut), ..self }
    }
}

/// Rows a module lists in a context menu's section, made when the menu opens from what's
/// open then (a tab's "Compare with ›": the window's other tabs), before the section's
/// items. A row's menu id is `<id>:<argument>`; a click runs [`MenuList::run`] with it.
#[derive(Clone)]
pub struct ListedMenuItems {
    pub id: &'static str,
    /// Its rows' icon.
    pub icon: Option<&'static str>,
    pub places: &'static [MenuPlace],
    pub section: MenuSection,
    pub list: Arc<dyn MenuList>,
}

impl std::fmt::Debug for ListedMenuItems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListedMenuItems").field("id", &self.id).finish_non_exhaustive()
    }
}

impl ListedMenuItems {
    /// The menu id of the row with `argument`.
    pub fn row_id(&self, argument: &str) -> String {
        format!("{}:{argument}", self.id)
    }

    /// The argument of a row's menu id, if the row is one of these.
    pub fn argument<'a>(&self, menu_id: &'a str) -> Option<&'a str> {
        menu_id.strip_prefix(self.id)?.strip_prefix(':')
    }
}

/// A listed row: its label and what it runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRow {
    pub label: String,
    pub argument: String,
}

/// What a module lists in a menu, and what a row does.
pub trait MenuList: Send + Sync {
    /// The rows of a menu opened on `context` (its place, what it acts on).
    fn rows(&self, host: &dyn WorkbenchHost, context: &ActionContext) -> Vec<ListedRow>;
    /// A row was clicked. `true` if something opened (the layout changes).
    fn run(&self, host: &mut dyn WorkbenchHost, argument: &str) -> bool;
}

/// A row of an arranged context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arranged<'a> {
    Item(&'a ContextMenuItem),
    /// A section shown as a submenu, with its items.
    Submenu(MenuSection, Vec<&'a ContextMenuItem>),
    Separator,
}

/// The items for `place` that are enabled there, by section and order, a separator
/// between the sections (none leading or trailing).
pub fn arrange<'a>(
    items: &'a [ContextMenuItem],
    place: MenuPlace,
    enabled: &dyn Fn(&ContextMenuItem) -> bool,
) -> Vec<Arranged<'a>> {
    let mut shown: Vec<&ContextMenuItem> =
        items.iter().filter(|i| i.places.contains(&place) && enabled(i)).collect();
    // Stable: equal orders stay as contributed.
    shown.sort_by_key(|i| (i.section, i.order));
    let mut out = Vec::new();
    for section in shown.chunk_by(|a, b| a.section == b.section) {
        if !out.is_empty() {
            out.push(Arranged::Separator);
        }
        let kind = section[0].section;
        if kind.submenu().is_some() {
            out.push(Arranged::Submenu(kind, section.to_vec()));
        } else {
            out.extend(section.iter().map(|i| Arranged::Item(i)));
        }
    }
    out
}

/// The menu bar's top-level menus (their ids): a module contributing sections of its own
/// names its menu by these (`command_model::MenuSectionSpec::menu`).
pub mod bar {
    /// The application menu (macOS only), named after the app.
    pub const APP: &str = "app";
    pub const FILE: &str = "file";
    pub const EDIT: &str = "edit";
    pub const VIEW: &str = "view";
    pub const FIND: &str = "find";
    pub const NAVIGATE: &str = "navigate";
    pub const CODE: &str = "code";
    pub const REFACTOR: &str = "refactor";
    pub const GIT: &str = "git";
    pub const TOOLS: &str = "tools";
    pub const WINDOW: &str = "window";
    pub const HELP: &str = "help";
}

/// The app's submenus (their ids): a module may put its sections in one.
pub mod submenus {
    /// Edit › Selection: extending, occurrences, carets.
    pub const SELECTION: &str = "edit.selection";
    /// View › Tool Windows: made from the registered tool windows.
    pub const TOOL_WINDOWS: &str = "view.tool_windows";
    /// View › Appearance: the theme.
    pub const APPEARANCE: &str = "view.appearance";
    /// Code › Folding.
    pub const FOLDING: &str = "code.folding";
    /// File › Open Recent: the recent projects and files, then Clear Recent.
    pub const OPEN_RECENT: &str = "file.open_recent";
}

/// The menu bar's sections the app declares (`command_model::MenuAt::section`): a module
/// puts its command in the menus by naming one of these and its place there, never a
/// position. A section a module brings is its own (Git's).
pub mod sections {
    pub const APP_ABOUT: &str = "app.about";
    pub const APP_SETTINGS: &str = "app.settings";
    pub const APP_SERVICES: &str = "app.services";
    pub const APP_HIDE: &str = "app.hide";
    pub const APP_QUIT: &str = "app.quit";
    pub const FILE_NEW: &str = "file.new";
    pub const FILE_OPEN: &str = "file.open";
    pub const RECENT_PROJECTS: &str = "file.open_recent.projects";
    pub const RECENT_FILES: &str = "file.open_recent.files";
    pub const RECENT_CLEAR: &str = "file.open_recent.clear";
    pub const FILE_SAVE: &str = "file.save";
    pub const FILE_CLOSE: &str = "file.close";
    pub const FILE_HISTORY: &str = "file.history";
    pub const EDIT_HISTORY: &str = "edit.history";
    pub const EDIT_CLIPBOARD: &str = "edit.clipboard";
    pub const EDIT_SELECT: &str = "edit.select";
    pub const EDIT_LINES: &str = "edit.lines";
    pub const SELECTION_EXTEND: &str = "edit.selection.extend";
    pub const SELECTION_OCCURRENCES: &str = "edit.selection.occurrences";
    pub const SELECTION_CARETS: &str = "edit.selection.carets";
    pub const VIEW_WINDOWS: &str = "view.windows";
    pub const VIEW_APPEARANCE: &str = "view.appearance";
    pub const APPEARANCE_THEME: &str = "view.appearance.theme";
    pub const VIEW_ZOOM: &str = "view.zoom";
    pub const VIEW_SCREEN: &str = "view.screen";
    pub const VIEW_NOTIFICATIONS: &str = "view.notifications";
    pub const FIND_FIND: &str = "find.find";
    pub const FIND_NEXT: &str = "find.next";
    pub const FIND_PROJECT: &str = "find.project";
    pub const NAVIGATE_HISTORY: &str = "navigate.history";
    pub const NAVIGATE_GOTO: &str = "navigate.goto";
    pub const NAVIGATE_DECLARATIONS: &str = "navigate.declarations";
    pub const NAVIGATE_HIERARCHY: &str = "navigate.hierarchy";
    pub const NAVIGATE_REVEAL: &str = "navigate.reveal";
    pub const CODE_ASSIST: &str = "code.assist";
    pub const CODE_EDIT: &str = "code.edit";
    pub const CODE_FOLDING: &str = "code.folding";
    pub const FOLDING_ONE: &str = "code.folding.one";
    pub const FOLDING_ALL: &str = "code.folding.all";
    pub const CODE_FORMAT: &str = "code.format";
    pub const CODE_CHECK: &str = "code.check";
    pub const REFACTOR_CHANGE: &str = "refactor.change";
    pub const REFACTOR_UNDO: &str = "refactor.undo";
    pub const TOOLS_DATA: &str = "tools.data";
    pub const TOOLS_LANGUAGES: &str = "tools.languages";
    pub const TOOLS_MAINTENANCE: &str = "tools.maintenance";
    pub const WINDOW_ARRANGE: &str = "window.arrange";
    pub const WINDOW_TABS: &str = "window.tabs";
    pub const WINDOW_CLOSE: &str = "window.close";
    pub const WINDOW_FRONT: &str = "window.front";
    pub const HELP_KEYS: &str = "help.keys";
    pub const HELP_TOOLS: &str = "help.tools";
    pub const HELP_ABOUT: &str = "help.about";
}

#[cfg(test)]
#[path = "../tests/unit/menus.rs"]
mod tests;
