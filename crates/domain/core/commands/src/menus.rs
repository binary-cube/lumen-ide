//! The menu bar's model: where a command sits (a section of a menu and its place there),
//! the sections themselves and the submenus that open more of them. Separators come between
//! the sections that show, never declared; the order is the one declared, not the order the
//! modules were registered in.

/// Where a command sits in the menu bar: a section, and its place inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuAt {
    /// The section's id ([`MenuSectionSpec::id`]).
    pub section: &'static str,
    /// Its place among the section's rows (smaller first).
    pub order: u16,
}

impl MenuAt {
    pub const fn new(section: &'static str, order: u16) -> Self {
        Self { section, order }
    }
}

/// The systems something in the menus is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Platforms {
    #[default]
    All,
    MacOs,
    /// Windows and Linux.
    Others,
}

impl Platforms {
    /// On macOS (`mac`), or on the others.
    pub const fn includes(self, mac: bool) -> bool {
        match self {
            Self::All => true,
            Self::MacOs => mac,
            Self::Others => !mac,
        }
    }
}

/// A group of rows in a menu: a top-level menu's or a submenu's (`menu`, its id). The same
/// section can be declared once per system, in another menu on each (Settings: the
/// application menu on macOS, File elsewhere); its rows follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuSectionSpec {
    pub id: &'static str,
    pub menu: &'static str,
    /// Its place in that menu (smaller first).
    pub order: u16,
    pub platforms: Platforms,
}

impl MenuSectionSpec {
    pub const fn new(id: &'static str, menu: &'static str, order: u16) -> Self {
        Self { id, menu, order, platforms: Platforms::All }
    }

    /// The same, only on `platforms`.
    pub const fn on(self, platforms: Platforms) -> Self {
        Self { platforms, ..self }
    }
}

/// A row opening more rows: the sections whose menu is its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubmenuSpec {
    pub id: &'static str,
    /// Where its row is.
    pub at: MenuAt,
    /// Its title's translation key; `label` (English) when it's missing.
    pub label_key: &'static str,
    pub label: &'static str,
    /// Its icon (a name from the app's set), where the app draws its menus.
    pub icon: Option<&'static str>,
}

/// What a command needs to make sense: without it, its menu row is greyed out and its keys
/// do nothing (a module may ask more of its own state, its handler's `enabled`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Requires {
    /// A project open.
    Project,
    /// The project in a version-controlled repository (Git).
    Repository,
    /// A tab open (any: a file, a view).
    Tab,
    /// The active tab shows a text file's editor.
    TextFile,
    /// The active tab's file is on disk (saved once: a path).
    FileOnDisk,
    /// The active tab's language is this one (its id: `sql`).
    Language(&'static str),
    /// A tab closed, to open again.
    ClosedTab,
    /// A tab visited before (Back) / after (Forward) the active one, still open.
    BackHistory,
    ForwardHistory,
    /// Another tab than the active one.
    OtherTabs,
    /// The active tab can be locked (read-only): a text's, or a view that says so.
    Lockable,
}

impl Requires {
    /// Why a command is greyed out without it: its hint's translation key.
    pub fn hint_key(self) -> &'static str {
        match self {
            Self::Project => "menu.needs_project",
            Self::Repository => "menu.needs_repository",
            Self::Tab => "menu.needs_tab",
            Self::OtherTabs => "menu.needs_other_tabs",
            Self::TextFile => "menu.needs_text_file",
            Self::Language(_) => "menu.needs_language",
            Self::FileOnDisk => "menu.needs_saved_file",
            Self::ClosedTab => "menu.needs_closed_tab",
            Self::BackHistory | Self::ForwardHistory => "menu.needs_history",
            Self::Lockable => "menu.needs_lockable",
        }
    }
}

/// The window's state the commands' requirements are checked against: made from it when
/// something they look at may have changed (never per frame), compared to the last one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuContext {
    pub project: bool,
    pub repository: bool,
    /// How many tabs are open.
    pub tabs: usize,
    pub text_file: bool,
    pub file_on_disk: bool,
    /// The active tab's language (its id).
    pub language: Option<String>,
    /// The active file changed since it was opened or saved (a module's state may follow it:
    /// Git's Revert Change).
    pub modified: bool,
    pub closed_tab: bool,
    pub back: bool,
    pub forward: bool,
    /// The active tab can be locked (read-only).
    pub lockable: bool,
}

impl MenuContext {
    /// `requires` holds here.
    pub fn satisfies(&self, requires: Requires) -> bool {
        match requires {
            Requires::Project => self.project,
            Requires::Repository => self.project && self.repository,
            Requires::Tab => self.tabs > 0,
            Requires::TextFile => self.text_file,
            Requires::FileOnDisk => self.file_on_disk,
            Requires::Language(id) => self.text_file && self.language.as_deref() == Some(id),
            Requires::ClosedTab => self.closed_tab,
            Requires::BackHistory => self.back,
            Requires::ForwardHistory => self.forward,
            Requires::OtherTabs => self.tabs > 1,
            Requires::Lockable => self.lockable,
        }
    }

    /// The first of `requires` that doesn't hold (`None`: they all do).
    pub fn missing(&self, requires: &[Requires]) -> Option<Requires> {
        requires.iter().copied().find(|r| !self.satisfies(*r))
    }
}

#[cfg(test)]
#[path = "../tests/unit/menus.rs"]
mod tests;
