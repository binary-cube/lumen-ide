//! The data sources' modal as a model (pure: no drawing, no thread): the sources being
//! edited — copies, the catalog untouched until Apply — the one selected, its fields as
//! its driver describes them ([`database::ConnectionSpec`]), its URL kept in step both
//! ways, its password typed (never read back from where it's kept), and what Apply makes of
//! it all ([`Plan`]: the catalog's changes and the passwords' moves). The sources are
//! listed in their folders ([`ListRow`]): folders made, renamed, colored, moved and taken
//! away here too, applied with the rest.

use std::collections::HashSet;

use database::fields::{self, Binding, FieldKind, FieldSpec, FieldTab, UrlError};
use database::folders::{self, FolderId, SourceColor, SourceFolder};
use database::{ConnectionSpec, DataSource, DataSourceId, Scope, SecretPolicy, Target};
use kernel::Secret;
use ui_components::search_field::SearchField;
use ui_kit::TextInput;

mod changes;
mod choices;
mod in_folders;
mod inputs;

pub use in_folders::{Item, ListRow, Place};

/// What the modal knows of a driver.
#[derive(Debug, Clone)]
pub struct DriverInfo {
    pub id: String,
    pub name: String,
    pub icon: &'static str,
    /// Its icon's tone (its engine's).
    pub tone: database::IconTone,
    pub spec: ConnectionSpec,
    pub file_based: bool,
    /// Its tables are grouped in schemas (the Schemas tab lists them).
    pub schemas: bool,
    /// Its server has several databases (the Schemas tab lists them too).
    pub databases: bool,
    /// A program installed apart, speaking the drivers' protocol (marked so in the lists).
    pub external: bool,
}

/// The tabs on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    General,
    Options,
    Ssl,
    Schemas,
    Advanced,
}

impl Tab {
    pub const ALL: [Self; 5] =
        [Self::General, Self::Options, Self::Ssl, Self::Schemas, Self::Advanced];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::General => "datasources.tab_general",
            Self::Options => "datasources.tab_options",
            Self::Ssl => "datasources.tab_ssl",
            Self::Schemas => "datasources.tab_schemas",
            Self::Advanced => "datasources.tab_advanced",
        }
    }
}

/// A text field of the modal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Field {
    Filter,
    Name,
    Comment,
    Url,
    /// A driver's field (by its id).
    Spec(String),
    Password,
    Startup,
    OptionKey(usize),
    OptionValue(usize),
    /// The selected folder's name.
    FolderName,
}

/// The password policies offered, in order.
pub const POLICIES: [SecretPolicy; 4] = [
    SecretPolicy::ProjectVault,
    SecretPolicy::Keychain,
    SecretPolicy::Session,
    SecretPolicy::Never,
];

/// A policy's label's key.
pub fn policy_key(policy: SecretPolicy) -> &'static str {
    match policy {
        SecretPolicy::ProjectVault => "datasources.policy_project",
        SecretPolicy::Keychain => "datasources.policy_keychain",
        SecretPolicy::Session => "datasources.policy_session",
        SecretPolicy::Never => "datasources.policy_never",
    }
}

/// A source being edited.
#[derive(Debug, Clone)]
pub struct Entry {
    pub source: DataSource,
    /// As the catalog has it (`None`: added here).
    pub original: Option<DataSource>,
    /// The password typed (masked), when it was.
    pub password: TextInput,
    pub password_typed: bool,
    /// Made by duplicating this one: its password is copied from it.
    pub copied_from: Option<DataSource>,
}

impl Entry {
    fn new(source: DataSource, original: Option<DataSource>) -> Self {
        let mut password = TextInput::new("");
        password.set_masked(true);
        Self { source, original, password, password_typed: false, copied_from: None }
    }

    /// It differs from what the catalog has.
    pub fn changed(&self) -> bool {
        self.original.as_ref() != Some(&self.source) || self.password_typed
    }
}

/// What happens to a password on Apply.
#[derive(Debug, Clone, PartialEq)]
pub enum SecretOp {
    /// Kept for the source as its policy says (empty: removed).
    Set(DataSource, Secret),
    /// The source's settings changed (its policy, its scope): its password follows.
    Move { from: DataSource, to: DataSource },
    /// Copied from a source to its duplicate.
    Copy { from: DataSource, to: DataSource },
    /// The source is gone: so is its password.
    Remove(DataSource),
}

/// What Apply does.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub add: Vec<DataSource>,
    pub update: Vec<DataSource>,
    pub remove: Vec<DataSource>,
    /// The lists whose folders changed: their folders as they are now.
    pub folders: Vec<(Scope, Vec<SourceFolder>)>,
    /// In order: moves first, then what's typed, then copies, then removals.
    pub secrets: Vec<SecretOp>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty()
            && self.folders.is_empty()
            && self.update.is_empty()
            && self.remove.is_empty()
            && self.secrets.is_empty()
    }
}

/// Why Apply can't be made yet: the source (its place) and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    NoName(usize),
    Field(usize, String, fields::FieldError),
    /// An advanced option named like a secret (it belongs in the password field).
    SecretOption(usize, String),
}

impl Problem {
    pub fn entry(&self) -> usize {
        match self {
            Self::NoName(i) | Self::Field(i, ..) | Self::SecretOption(i, _) => *i,
        }
    }
}

/// The modal's state (see the module's doc).
#[derive(Debug)]
pub struct Editor {
    pub drivers: Vec<DriverInfo>,
    pub entries: Vec<Entry>,
    /// Removed here (those the catalog had).
    pub removed: Vec<DataSource>,
    pub selected: Option<usize>,
    /// The list's filter (the app's search field: ×, ⌘A; its focus the modal's).
    pub filter: SearchField,
    pub tab: Tab,
    /// The selected source's fields.
    pub name: TextInput,
    pub comment: TextInput,
    pub url: TextInput,
    pub url_error: Option<UrlError>,
    pub specs: Vec<(String, TextInput)>,
    pub startup: TextInput,
    /// Its own (advanced) options: name and value.
    pub options: Vec<(TextInput, TextInput)>,
    /// The password shown as typed (the eye).
    pub revealed: bool,
    /// The schemas the last test found, by source.
    pub schemas_found: Vec<(DataSourceId, Vec<String>)>,
    /// The databases the last test found (a server with several), by source.
    pub databases_found: Vec<(DataSourceId, Vec<String>)>,
    /// Both lists' folders, as edited (and as the catalog has them).
    pub folders: Vec<SourceFolder>,
    original_folders: Vec<SourceFolder>,
    /// The folders closed in the list.
    pub collapsed: HashSet<FolderId>,
    /// The folder selected (then no source is).
    pub selected_folder: Option<FolderId>,
    pub folder_name: TextInput,
}

/// The options the modal edits outside the Advanced tab.
fn own_option(spec: &ConnectionSpec, name: &str) -> bool {
    spec.bound_options().contains(&name) || name == Target::READ_ONLY || name == Target::STARTUP
}

impl Editor {
    /// The modal over `project` and `global` (the catalog's lists, in their order).
    pub fn new(drivers: Vec<DriverInfo>, project: &[DataSource], global: &[DataSource]) -> Self {
        let entries =
            project.iter().chain(global).map(|s| Entry::new(s.clone(), Some(s.clone()))).collect();
        let mut editor = Self {
            drivers,
            entries,
            removed: Vec::new(),
            selected: None,
            filter: SearchField::new(),
            tab: Tab::General,
            name: TextInput::new(""),
            comment: TextInput::new(""),
            url: TextInput::new(""),
            url_error: None,
            specs: Vec::new(),
            startup: TextInput::new(""),
            options: Vec::new(),
            revealed: false,
            schemas_found: Vec::new(),
            databases_found: Vec::new(),
            folders: Vec::new(),
            original_folders: Vec::new(),
            collapsed: HashSet::new(),
            selected_folder: None,
            folder_name: TextInput::new(""),
        };
        editor.sort();
        let first = editor.visible().first().copied();
        editor.select(first);
        editor
    }

    /// The lists' folders (as the catalog has them).
    pub fn with_folders(mut self, folders: Vec<SourceFolder>) -> Self {
        self.original_folders = folders.clone();
        self.folders = folders;
        let first = self.visible().first().copied();
        self.select(first);
        self
    }

    /// The project's first, then the global ones (each as listed).
    fn sort(&mut self) {
        let selected = self.selected.map(|i| self.entries[i].source.id);
        self.entries.sort_by_key(|e| e.source.scope == Scope::Global);
        self.selected = selected.and_then(|id| self.entries.iter().position(|e| e.source.id == id));
    }

    pub fn driver(&self, id: &str) -> Option<&DriverInfo> {
        self.drivers.iter().find(|d| d.id == id)
    }

    /// The selected source.
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.selected?)
    }

    fn current_mut(&mut self) -> Option<&mut Entry> {
        self.entries.get_mut(self.selected?)
    }

    /// The selected source's driver's fields (none known: none).
    pub fn spec(&self) -> ConnectionSpec {
        self.current()
            .and_then(|e| self.driver(&e.source.driver))
            .map(|d| d.spec.clone())
            .unwrap_or_default()
    }

    /// The entries shown (the filter keeps them, their folders are open), in the list's
    /// order.
    pub fn visible(&self) -> Vec<usize> {
        self.list_rows()
            .into_iter()
            .filter_map(|r| match r {
                ListRow::Source { index, .. } => Some(index),
                _ => None,
            })
            .collect()
    }

    /// Anything to apply.
    pub fn dirty(&self) -> bool {
        !self.removed.is_empty()
            || self.entries.iter().any(Entry::changed)
            || self.folders != self.original_folders
    }

    /// Entry `at` selected (`None`: none): its fields shown.
    pub fn select(&mut self, at: Option<usize>) {
        self.selected = at.filter(|i| *i < self.entries.len());
        self.selected_folder = None;
        self.load();
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/sources/form.rs"]
mod tests;
