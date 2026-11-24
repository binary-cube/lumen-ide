//! The menu bar: its menus, their sections and submenus (the app's and the modules'), laid
//! out for the system it's on, as a tree of rows.

use super::*;

/// A top-level menu of the menu bar: its id (its sections name it), its title (`menu.<id>`
/// in the locales, `title` when missing; `{app}`: named after the app, see
/// `kernel::brand`), its icon where the app draws the menus (the menu drawer), the systems
/// it's on.
#[derive(Debug, Clone, Copy)]
pub(super) struct TopMenu {
    pub(super) id: &'static str,
    title: &'static str,
    pub(super) icon: &'static str,
    pub(super) platforms: Platforms,
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
pub(super) const MENUS: &[TopMenu] = &[
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
pub(super) const SUBMENUS: &[SubmenuSpec] = &[
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
pub(super) const NATIVE: &[(MenuAt, NativeItem, &str, &str)] = &[
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
pub(super) fn menu_title(menu: &TopMenu) -> String {
    if menu.title == "{app}" {
        kernel::brand::named(menu.title)
    } else {
        localized(&format!("menu.{}", menu.id), menu.title)
    }
}

/// What the menus are made of on one system: the sections, the submenus, the commands (the
/// app's, then the modules').
pub(super) struct MenuTree {
    system: System,
    pub(super) sections: Vec<MenuSectionSpec>,
    pub(super) submenus: Vec<SubmenuSpec>,
    pub(super) commands: Vec<CommandSpec>,
}

impl MenuTree {
    pub(super) fn new(system: System) -> Self {
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
    pub(super) fn path(&self, section: &str) -> Vec<String> {
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
