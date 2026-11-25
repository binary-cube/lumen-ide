//! A module's part of the settings window: a module declares its own settings
//! ([`settings_model::Setting`], in its crate) and contributes a [`SettingsContribution`]
//! at `activate` — which page its block goes on (an app page, a shared one, or its own:
//! [`SettingsPage`], in a [`SettingsGroup`]) and where on it, what it
//! shows (choices, sliders, switches, text fields, a status line with a button), what a
//! choice, a switch, a text or the button does, and
//! how its saved values reach its domain. The app only discovers the contributions and
//! the settings window draws their blocks, the same way for all; a module turned off
//! simply isn't there.

use std::path::Path;

use settings_model::SettingsService;

use crate::{OnYes, Question};

/// What a block shows, top to bottom, under its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsBlock {
    pub title: String,
    pub detail: String,
    pub rows: Vec<SettingsRow>,
}

/// A row of a block. Each with a label has a short description under it (`detail`: what
/// it does, in a few words; empty: none); a choice, a slider or a switch backed by a
/// setting has its `default` ("↺" shows while it differs: a click on it chooses the
/// default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsRow {
    /// A label and choices side by side (pills), one of them current; a click on one is
    /// [`SettingsContribution::choose`] with `key` and its value.
    Choices {
        key: &'static str,
        label: String,
        detail: String,
        choices: Vec<(i64, String)>,
        current: i64,
        default: Option<i64>,
    },
    /// The same as a segmented control (a few short choices in one track: where the
    /// notifications go); a click on a segment is [`SettingsContribution::choose`] too.
    Segmented {
        key: &'static str,
        label: String,
        detail: String,
        choices: Vec<(i64, String)>,
        current: i64,
        default: Option<i64>,
    },
    /// The same as a slider over its stops (for numbers in order: 1 · 2 · 4 · 8), the
    /// current stop's label said beside it; a click or a drag picks the nearest stop.
    Slider {
        key: &'static str,
        label: String,
        detail: String,
        stops: Vec<(i64, String)>,
        current: i64,
        default: Option<i64>,
    },
    /// The same as a list to choose from (a field showing the choice, its list opened by a
    /// click: many choices, or rows of them one under the other); a choice is
    /// [`SettingsContribution::choose`] with `key` and its value.
    Dropdown {
        key: &'static str,
        label: String,
        detail: String,
        /// An icon before its label (a module's), a name of the app's set.
        icon: Option<&'static str>,
        choices: Vec<(i64, String)>,
        current: i64,
        default: Option<i64>,
    },
    /// A card with a status line (what it holds now) and maybe a destructive button; a
    /// click on it is [`SettingsContribution::press`] with its action.
    Status { text: String, button: Option<(&'static str, String)> },
    /// A switch and what it turns on; a click is [`SettingsContribution::choose`] with `key`
    /// and 1 (on) or 0 (off).
    Toggle { key: &'static str, label: String, detail: String, on: bool, default: Option<bool> },
    /// A text field (a path, a command…), its value saved once written (Enter, or a click
    /// elsewhere): [`SettingsContribution::set_text`]; Esc gives up the change.
    Text { key: &'static str, label: String, detail: String, value: String, placeholder: String },
    /// A switch and what it turns on, and on its right what it's doing now (a badge: a
    /// model downloading, ready…); a click on the switch is [`SettingsContribution::choose`]
    /// with `key` and 1 (on) or 0 (off). A search that leads to its block highlights it.
    SwitchCard {
        key: &'static str,
        label: String,
        detail: String,
        on: bool,
        status: String,
        tone: StatusTone,
    },
    /// A secret's field (a licence key, a token): what's typed shows as dots and is never
    /// shown again — saved once written ([`SettingsContribution::set_text`]; an empty one
    /// removes it), the field saying only whether one is `set`.
    Secret { key: &'static str, label: String, detail: String, set: bool, placeholder: String },
    /// A card with a status line and buttons that don't destroy anything (Install, Use it);
    /// a click on one is [`SettingsContribution::press`] with its action.
    Actions { text: String, buttons: Vec<(&'static str, String)> },
    /// Lines of explanation (what it does, what it costs), muted, one under the other.
    Notes(Vec<String>),
    /// Cards side by side, each a picture of what choosing it gives (as the themes' on
    /// Appearance), its name and a tag under it; a click on one is
    /// [`SettingsContribution::choose`] with `key` and its value. The pictures are painted by
    /// the module's card preview named `preview` (in `ui-components`): the selected card —
    /// or the hovered one — live, the others still.
    Cards {
        key: &'static str,
        label: String,
        detail: String,
        cards: Vec<SettingsCard>,
        current: i64,
        default: Option<i64>,
        preview: &'static str,
    },
}

/// A card of a [`SettingsRow::Cards`] row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsCard {
    pub value: i64,
    pub label: String,
    /// A short tag beside its name (`3D`).
    pub tag: Option<String>,
}

impl SettingsRow {
    /// Its key, its label and its description, for a row a search can lead to.
    pub fn searchable(&self) -> Option<(&'static str, &str, &str)> {
        match self {
            Self::Choices { key, label, detail, .. }
            | Self::Segmented { key, label, detail, .. }
            | Self::Dropdown { key, label, detail, .. }
            | Self::Slider { key, label, detail, .. }
            | Self::Toggle { key, label, detail, .. }
            | Self::Text { key, label, detail, .. }
            | Self::SwitchCard { key, label, detail, .. }
            | Self::Secret { key, label, detail, .. }
            | Self::Cards { key, label, detail, .. } => Some((key, label, detail)),
            Self::Status { .. } | Self::Actions { .. } | Self::Notes(_) => None,
        }
    }
}

/// How a status badge reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusTone {
    #[default]
    Neutral,
    /// Working, in use.
    Accent,
    /// Something failed.
    Danger,
}

/// The groups of the settings' navigation, in its order (each shows a caption over its
/// pages).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SettingsGroup {
    /// How the app looks: themes, icons, notifications.
    Interface,
    /// Writing code: the editor, files, languages and their servers.
    Editing,
    /// The tools: terminal, databases, Task Manager, remote projects, search.
    Tools,
    /// The app itself: shortcuts, modules, its language.
    Application,
}

impl SettingsGroup {
    pub const ALL: [Self; 4] = [Self::Interface, Self::Editing, Self::Tools, Self::Application];

    /// Its name's key in the locales.
    pub fn label(self) -> &'static str {
        match self {
            Self::Interface => "settings.group_interface",
            Self::Editing => "settings.group_editing",
            Self::Tools => "settings.group_tools",
            Self::Application => "settings.group_application",
        }
    }

    /// Its icon (from the app's set), where only groups are shown (narrow tabs).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Interface => "palette",
            Self::Editing => "file-pen",
            Self::Tools => "terminal",
            Self::Application => "gear",
        }
    }
}

/// A page of the settings' navigation: its id (what blocks name), its name (a key in the
/// locales), its icon, its group and its place in the group (lower first; then by id). Its
/// owner describes it ([`SettingsContribution::page_info`]); the pages several owners share
/// are the constants below. A page with no blocks isn't shown (its module is off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsPage {
    pub id: &'static str,
    /// The name's key in the locales (`settings.terminal_section`).
    pub label: &'static str,
    /// The icon's name (from the app's set).
    pub icon: &'static str,
    pub group: SettingsGroup,
    pub order: u16,
}

impl SettingsPage {
    /// The place of a page shown last in its group, whatever else is added (Updates).
    pub const LAST: u16 = u16::MAX;

    /// Editing › Editor: the caret, what's shown in the code.
    pub const EDITOR: Self = Self {
        id: "editor",
        label: "settings.editor",
        icon: "file-pen",
        group: SettingsGroup::Editing,
        order: 10,
    };
    /// Editing › Files: saving, the final line break, the local history.
    pub const FILES: Self = Self {
        id: "files",
        label: "settings.files_section",
        icon: "file-text",
        group: SettingsGroup::Editing,
        order: 20,
    };
    /// Editing › Languages: one language chosen at the top, then its engine, its formatter
    /// and checker, its server ([`SettingsScope::language`]).
    pub const LANGUAGES: Self = Self {
        id: "languages",
        label: "settings.languages_section",
        icon: "braces",
        group: SettingsGroup::Editing,
        order: 30,
    };
    /// Editing › Language servers: what's downloaded, the servers' limits, servers added.
    pub const LANGUAGE_SERVERS: Self = Self {
        id: "language-servers",
        label: "settings.lsp_title",
        icon: "server",
        group: SettingsGroup::Editing,
        order: 40,
    };
    /// Tools › Databases: the data, the query consoles, the extractors, profiling.
    pub const DATABASE: Self = Self {
        id: "database",
        label: "settings.database_section",
        icon: "database",
        group: SettingsGroup::Tools,
        order: 20,
    };

    /// The order pages are listed in: by group, then their place, then their id.
    pub fn sort_key(&self) -> (SettingsGroup, u16, &'static str) {
        (self.group, self.order, self.id)
    }
}

/// A language of the Languages page's chooser: its id (`php`) and its name (`PHP`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLanguage {
    pub id: String,
    pub name: String,
}

/// The languages listed first, in this order (the others after them, by name).
const FIRST_LANGUAGES: [&str; 13] = [
    "php",
    "javascript",
    "typescript",
    "tsx",
    "vue",
    "html",
    "css",
    "scss",
    "less",
    "json",
    "yaml",
    "sql",
    "markdown",
];

/// The blocks' languages as the chooser lists them: each once (the first name given), the
/// usual ones first, the others by name — whatever order the blocks came in.
pub fn merge_languages(lists: impl IntoIterator<Item = SettingsLanguage>) -> Vec<SettingsLanguage> {
    let mut out: Vec<SettingsLanguage> = Vec::new();
    for language in lists {
        if !out.iter().any(|l| l.id == language.id) {
            out.push(language);
        }
    }
    let rank = |id: &str| FIRST_LANGUAGES.iter().position(|l| *l == id).unwrap_or(usize::MAX);
    out.sort_by(|a, b| {
        (rank(&a.id), a.name.to_lowercase(), &a.id).cmp(&(
            rank(&b.id),
            b.name.to_lowercase(),
            &b.id,
        ))
    });
    out
}

/// What a block is drawn for: the settings, the open project's own settings
/// (`<project dir>/project.toml`: what a module keeps per project — a formatter, a checker), and
/// its data folder (the project's own things a module keeps there). `None`: no project open.
#[derive(Clone, Copy)]
pub struct SettingsScope<'a> {
    pub settings: &'a SettingsService,
    pub project: Option<&'a SettingsService>,
    pub project_data: Option<&'a Path>,
    /// The language the Languages page shows ([`SettingsPage::LANGUAGES`]): one for all its
    /// blocks, chosen at its top (`None`: none to choose from).
    pub language: Option<&'a str>,
    /// Wakes the app from a background thread (a block's slow figure computed: then
    /// [`SettingsContribution::poll`] says so and the block is drawn again).
    pub wake: Option<&'a std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for SettingsScope<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsScope")
            .field("project", &self.project.is_some())
            .field("project_data", &self.project_data)
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

/// A module's block in the settings window.
pub trait SettingsContribution: std::fmt::Debug + Send + Sync {
    /// The block's id (unique; where a search result leads).
    fn id(&self) -> &'static str;

    /// The page it goes on: an app page's id (`appearance`, `icons`…), a shared one's
    /// ([`SettingsPage::LANGUAGES`]…) or its own (then [`Self::page_info`] describes it).
    fn page(&self) -> &'static str;

    /// The page it goes on, described (its own, or a shared one's constant): where it shows
    /// in the navigation. `None`: an app page (or one another block describes).
    fn page_info(&self) -> Option<SettingsPage> {
        None
    }

    /// Its place on its page (lower first; then by id): never the modules' order.
    fn order(&self) -> u16 {
        100
    }

    /// It's for those who look for it: under the page's foldable "Advanced", at its end.
    fn advanced(&self) -> bool {
        false
    }

    /// The languages it has something for, on the Languages page (its chooser lists every
    /// block's, once): the block then shows [`SettingsScope::language`]'s.
    fn languages(&self, scope: SettingsScope<'_>) -> Vec<SettingsLanguage> {
        let _ = scope;
        Vec::new()
    }

    /// The one language it's about (SQL's formatting): shown only while it's the one
    /// chosen, and in the chooser.
    fn language(&self) -> Option<SettingsLanguage> {
        None
    }

    /// Hands the saved values to the module's domain: at startup and after each change.
    fn apply(&self, settings: &SettingsService) {
        let _ = settings;
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock;

    /// Something its block shows was computed in the background since the last call (the
    /// block is drawn again). Slow figures (what a folder holds) are computed so, never in
    /// [`Self::block`].
    fn poll(&self) -> bool {
        false
    }

    /// A choice was clicked: saves it — in the settings, or the project's (then
    /// [`Self::apply`] runs).
    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64);

    /// A text field was written: saves it — in the settings, or the project's (then
    /// [`Self::apply`] runs).
    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let _ = (scope, key, value);
    }

    /// A button's command (by its action), run by the app as from the palette — the
    /// settings stay open under what it opens (a preview); `None`: [`Self::press`] has it.
    fn command(&self, action: &'static str) -> Option<command_model::CommandId> {
        let _ = action;
        None
    }

    /// The status card's button was clicked: what to ask first and what runs on "yes"
    /// (`None`: nothing to do).
    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        let _ = (scope, action);
        None
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings_page.rs"]
mod tests;
