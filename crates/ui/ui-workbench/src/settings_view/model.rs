//! What the settings window is given and what it reports: the model drawn (themes, icons,
//! modules, frameworks, the blocks modules contribute) and the actions a click gives.

use super::*;

/// A theme's colors for the preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePreview {
    pub window: Rgba,
    pub editor: Rgba,
    pub text: Rgba,
    pub accent: Rgba,
    pub keyword: Rgba,
    pub string: Rgba,
}

impl ThemePreview {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            window: p.window_background,
            editor: p.editor_background,
            text: p.code_text,
            accent: p.accent,
            keyword: p.terminal_ansi[3],
            string: p.terminal_ansi[2],
        }
    }
}

/// A theme in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeOption {
    pub id: String,
    pub name: String,
    pub dark: bool,
    pub preview: ThemePreview,
}

/// What the settings display (the current state, from the app).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsModel {
    pub themes: Vec<ThemeOption>,
    pub current_theme: String,
    pub follow_system: bool,
    /// The interface languages: (code, name in its own language).
    pub languages: Vec<(String, String)>,
    pub current_language: String,
    /// The icons' look chosen, and every one to choose from (our styles, the packs).
    pub icon_look: icons::IconLook,
    pub icon_looks: Vec<IconChoice>,
    /// The file icons chosen (`None`: ours, by language), and the sets to choose from.
    pub file_icon_set: Option<u8>,
    pub file_icon_sets: Vec<FileIconChoice>,
    /// The files the preview shows, as the languages say (name, our icon, language), in
    /// their order.
    pub icon_samples: Vec<(String, &'static str, String)>,
    /// Keymap presets: (id, name), the active one, and its shortcuts (command, keys as
    /// the system shows them).
    pub keymaps: Vec<(String, String)>,
    pub current_keymap: String,
    /// Every command, its keys (Settings → Shortcuts).
    pub bindings: std::sync::Arc<Vec<crate::shortcuts_editor::BindingRow>>,
    /// Every module (but the frameworks'): on / off, loaded at startup, in memory.
    pub modules: Vec<ModuleRow>,
    /// The modules' data is read at startup (else on first use, let go when unused).
    pub modules_at_startup: bool,
    /// The open project's name (`None`: no project, no framework choices).
    pub project: Option<String>,
    pub frameworks: Vec<FrameworkRow>,
    /// The modules' blocks (their own settings), in the order they're shown.
    pub blocks: Vec<ContributedBlock>,
    /// The Languages page's chooser: (id, name) of each language its blocks have something
    /// for, and the one shown.
    pub code_languages: Vec<(String, String)>,
    pub code_language: Option<String>,
}

/// A module's block in the settings (from its `SettingsContribution`): where it goes, the
/// words a search finds it by, and what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedBlock {
    pub id: &'static str,
    /// The page it's on.
    pub page: &'static str,
    /// The page it's on, described (its own, a shared one), if it describes it.
    pub page_info: Option<SettingsPage>,
    /// Its place on the page (lower first, then by id), and whether it's under the page's
    /// "Advanced".
    pub order: u16,
    pub advanced: bool,
    /// The one language it's about (shown on the Languages page only while it's chosen).
    pub language: Option<String>,
    /// Words a search finds it by, besides its title and detail (every language's).
    pub keywords: String,
    pub block: workbench_model::SettingsBlock,
}

/// An icon look to choose: itself, its name and what it is (a pack's author and license).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconChoice {
    pub look: icons::IconLook,
    pub name: String,
    pub detail: String,
}

/// A file icon set to choose (`None`: ours), its name and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIconChoice {
    pub set: Option<u8>,
    pub name: String,
    pub detail: String,
}

/// A module in the settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleRow {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Its icon (a name of the app's set).
    pub icon: &'static str,
    /// Who made it, its version, when it last changed (`YYYY-MM-DD`; empty: unknown).
    pub author: String,
    pub version: String,
    pub updated: String,
    /// On (from the next start).
    pub enabled: bool,
    /// Working now.
    pub active: bool,
    /// Its data read at startup.
    pub preload: bool,
    /// Its data is in memory now.
    pub loaded: bool,
    /// The modules on that need it (it can't be turned off while they are).
    pub required_by: Vec<String>,
    /// Hidden (from the next start): it runs, but shows nothing of its own.
    pub hidden: bool,
    /// Running hidden now.
    pub hidden_now: bool,
}

/// A framework in the settings: whether the project uses it and why.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameworkRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub on: bool,
    /// Where it was detected from (e.g. `composer.json`).
    pub detected: Option<String>,
    /// Chosen in the project's settings (overriding the detection).
    pub chosen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    SelectTheme(String),
    ToggleFollowSystem,
    SelectLanguage(String),
    SelectKeymap(String),
    /// Commands' shortcuts changed (each one's whole list; `None`: the preset's).
    Shortcuts(crate::shortcuts_editor::ShortcutChanges),
    SelectIcons(icons::IconLook),
    /// The file icons: a set's (its place), or ours (`None`).
    SelectFileIcons(Option<u8>),
    /// A settings search result (opening it is done by the component).
    OpenResult,
    /// The theme configurator.
    OpenThemeStudio,
    /// A framework turned on / off for the open project.
    ToggleFramework(String),
    /// A module turned on / off (from the next start).
    ToggleModule(String),
    /// A module hidden / shown (from the next start).
    ToggleModuleHidden(String),
    /// A module's data read at startup, or not.
    TogglePreload(String),
    /// The modules' data read at startup (`true`) or on first use.
    SetModulesAtStartup(bool),
    /// A module block's choice: its row's key and the value chosen.
    Choose {
        block: &'static str,
        key: &'static str,
        value: i64,
    },
    /// Another language chosen on the Languages page.
    SelectCodeLanguage(String),
    /// A row's "↺": its default chosen (as [`Self::Choose`]).
    Reset {
        block: &'static str,
        key: &'static str,
        value: i64,
    },
    /// A page's "Advanced" opened or closed (done by the component).
    ToggleAdvanced(&'static str),
    /// A module block's button (its action).
    Press {
        block: &'static str,
        action: &'static str,
    },
    /// A module block's text field, written: its row's key and the text.
    SetText {
        block: &'static str,
        key: &'static str,
        value: String,
    },
    Close,
}
