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
use ui_kit::TextInput;

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
    pub filter: TextInput,
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
            filter: TextInput::new(""),
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

    /// The selected source's values into the fields.
    fn load(&mut self) {
        let Some(entry) = self.current().cloned() else {
            self.specs.clear();
            self.options.clear();
            return;
        };
        let spec = self.spec();
        self.name = TextInput::new(&entry.source.name);
        self.comment = TextInput::new(&entry.source.comment);
        self.specs = spec
            .fields
            .iter()
            .filter(|f| !f.is_secret())
            .map(|f| (f.id.clone(), TextInput::new(&fields::value(&entry.source, f))))
            .collect();
        self.startup = TextInput::new(
            entry.source.target.options.get(Target::STARTUP).map_or("", String::as_str),
        );
        self.options = entry
            .source
            .target
            .options
            .iter()
            .filter(|(k, _)| !own_option(&spec, k))
            .map(|(k, v)| (TextInput::new(k), TextInput::new(v)))
            .collect();
        self.revealed = false;
        self.url_error = None;
        self.write_url();
    }

    /// The URL written from the source as it is now.
    fn write_url(&mut self) {
        let spec = self.spec();
        let url = self.current().and_then(|e| fields::to_url(&spec, &e.source)).unwrap_or_default();
        self.url = TextInput::new(&url);
    }

    /// Every field there is now, with its text, in one order that [`Self::inputs_mut`]
    /// keeps too: the filter, the source's own, its driver's, its options, its password
    /// (a source selected), the folder's name (a folder selected).
    pub fn inputs(&self) -> Vec<(Field, &TextInput)> {
        let mut out = vec![
            (Field::Filter, &self.filter),
            (Field::Name, &self.name),
            (Field::Comment, &self.comment),
            (Field::Url, &self.url),
            (Field::Startup, &self.startup),
        ];
        out.extend(self.specs.iter().map(|(id, input)| (Field::Spec(id.clone()), input)));
        for (i, (key, value)) in self.options.iter().enumerate() {
            out.extend([(Field::OptionKey(i), key), (Field::OptionValue(i), value)]);
        }
        if let Some(entry) = self.current() {
            out.push((Field::Password, &entry.password));
        }
        if self.selected_folder.is_some() {
            out.push((Field::FolderName, &self.folder_name));
        }
        out
    }

    /// [`Self::inputs`], to be edited (the same fields, in the same order).
    pub fn inputs_mut(&mut self) -> Vec<(Field, &mut TextInput)> {
        let Self {
            filter,
            name,
            comment,
            url,
            startup,
            specs,
            options,
            entries,
            selected,
            selected_folder,
            folder_name,
            ..
        } = self;
        let mut out = vec![
            (Field::Filter, filter),
            (Field::Name, name),
            (Field::Comment, comment),
            (Field::Url, url),
            (Field::Startup, startup),
        ];
        out.extend(specs.iter_mut().map(|(id, input)| (Field::Spec(id.clone()), input)));
        for (i, (key, value)) in options.iter_mut().enumerate() {
            out.extend([(Field::OptionKey(i), key), (Field::OptionValue(i), value)]);
        }
        if let Some(entry) = selected.and_then(|i| entries.get_mut(i)) {
            out.push((Field::Password, &mut entry.password));
        }
        if selected_folder.is_some() {
            out.push((Field::FolderName, folder_name));
        }
        out
    }

    pub fn input(&self, field: &Field) -> Option<&TextInput> {
        Some(match field {
            Field::Filter => &self.filter,
            Field::Name => &self.name,
            Field::Comment => &self.comment,
            Field::Url => &self.url,
            Field::Startup => &self.startup,
            Field::Spec(id) => &self.specs.iter().find(|(i, _)| i == id)?.1,
            Field::Password => &self.current()?.password,
            Field::OptionKey(i) => &self.options.get(*i)?.0,
            Field::OptionValue(i) => &self.options.get(*i)?.1,
            Field::FolderName => {
                self.selected_folder?;
                &self.folder_name
            }
        })
    }

    pub fn input_mut(&mut self, field: &Field) -> Option<&mut TextInput> {
        Some(match field {
            Field::Filter => &mut self.filter,
            Field::Name => &mut self.name,
            Field::Comment => &mut self.comment,
            Field::Url => &mut self.url,
            Field::Startup => &mut self.startup,
            Field::Spec(id) => &mut self.specs.iter_mut().find(|(i, _)| i == id)?.1,
            Field::Password => &mut self.current_mut()?.password,
            Field::OptionKey(i) => &mut self.options.get_mut(*i)?.0,
            Field::OptionValue(i) => &mut self.options.get_mut(*i)?.1,
            Field::FolderName => {
                self.selected_folder?;
                &mut self.folder_name
            }
        })
    }

    /// `field` was edited: its text goes into the source (the URL into every field, and
    /// every field into the URL).
    pub fn edited(&mut self, field: &Field) {
        match field {
            Field::Filter => {
                let visible = self.visible();
                if self.selected.is_none_or(|s| !visible.contains(&s)) {
                    self.select(visible.first().copied());
                }
                return;
            }
            Field::Password => {
                if let Some(entry) = self.current_mut() {
                    entry.password_typed = true;
                }
                return;
            }
            Field::FolderName => {
                self.store_folder_name();
                return;
            }
            Field::Url => {
                let spec = self.spec();
                let text = self.url.text().to_owned();
                let Some(entry) = self.current_mut() else { return };
                let mut source = entry.source.clone();
                match fields::from_url(&spec, &text, &mut source) {
                    Ok(secret) => {
                        entry.source = source;
                        // A password in the URL goes to its field, never the source.
                        if let Some(secret) = secret {
                            entry.password = TextInput::new(secret.expose());
                            entry.password.set_masked(true);
                            entry.password_typed = true;
                        }
                        let url = std::mem::replace(&mut self.url, TextInput::new(""));
                        self.load();
                        self.url = url;
                        // The password never stays in the URL's text.
                        if self.current().is_some_and(|e| e.password_typed)
                            && text.contains('@')
                            && let Some(e) = self.current()
                            && let Some(clean) = fields::to_url(&spec, &e.source)
                            && clean.len() < text.len()
                        {
                            self.url = TextInput::new(&clean);
                        }
                        self.url_error = None;
                    }
                    Err(e) => self.url_error = Some(e),
                }
                return;
            }
            _ => {}
        }
        self.store();
        self.write_url();
    }

    /// The fields' texts into the selected source.
    fn store(&mut self) {
        let spec = self.spec();
        let name = self.name.text().trim().to_owned();
        let comment = self.comment.text().trim().to_owned();
        let startup = self.startup.text().trim().to_owned();
        let specs: Vec<(String, String)> =
            self.specs.iter().map(|(id, input)| (id.clone(), input.text().to_owned())).collect();
        let options: Vec<(String, String)> = self
            .options
            .iter()
            .map(|(k, v)| (k.text().trim().to_owned(), v.text().trim().to_owned()))
            .collect();
        let Some(entry) = self.current_mut() else { return };
        let source = &mut entry.source;
        source.name = name;
        source.comment = comment;
        for (id, text) in specs {
            if let Some(field) = spec.field(&id) {
                fields::set_value(source, field, &text);
            }
        }
        // The advanced options: as listed (an empty name is none; a secret's never kept).
        source.target.options.retain(|k, _| own_option(&spec, k));
        for (k, v) in options {
            if !k.is_empty() && !database::source::is_secret_option(&k) && !own_option(&spec, &k) {
                source.target.options.insert(k, v);
            }
        }
        if startup.is_empty() {
            source.target.options.remove(Target::STARTUP);
        } else {
            source.target.options.insert(Target::STARTUP.to_owned(), startup);
        }
    }

    /// A choice field's value chosen (an empty one: its default).
    pub fn choose(&mut self, id: &str, value: &str) {
        if let Some((_, input)) = self.specs.iter_mut().find(|(i, _)| i == id) {
            input.set_text(value);
        }
        self.store();
        self.write_url();
    }

    /// A switch field flipped (unset: its default first).
    pub fn flip(&mut self, id: &str) {
        let spec = self.spec();
        let Some(field) = spec.field(id) else { return };
        let now = self.specs.iter().find(|(i, _)| i == id).map(|(_, t)| t.text().to_owned());
        let now = now.filter(|t| !t.is_empty()).unwrap_or_else(|| field.default.clone());
        self.choose(id, if now == "true" { "false" } else { "true" });
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        if let Some(entry) = self.current_mut() {
            entry.source.read_only = read_only;
        }
    }

    /// The selected source marked as production (its own mark; a folder's stays).
    pub fn set_production(&mut self, production: bool) {
        if let Some(entry) = self.current_mut() {
            entry.source.production = production;
        }
    }

    /// Entry `index` is production: marked so, or in a folder that is.
    pub fn production_of(&self, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else { return false };
        entry.source.production || self.inherited_production(index).is_some()
    }

    /// The folder entry `index` takes production from (the nearest one up marked so).
    pub fn inherited_production(&self, index: usize) -> Option<&SourceFolder> {
        let source = &self.entries.get(index)?.source;
        let folder = self.folder_of(source)?;
        folders::production_folder(&self.folders, folder)
    }

    /// Folder `id` marked as production, or not.
    pub fn set_folder_production(&mut self, id: FolderId, production: bool) {
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.production = production;
        }
    }

    /// Folder `id` is production (marked so, or inside one that is).
    pub fn folder_production(&self, id: FolderId) -> bool {
        folders::folder_production(&self.folders, id)
    }

    pub fn set_policy(&mut self, policy: SecretPolicy) {
        if let Some(entry) = self.current_mut() {
            entry.source.secrets = policy;
        }
    }

    /// The selected source's schema `name` shown or not (all shown: all but it, then).
    pub fn toggle_schema(&mut self, name: &str, all: &[String]) {
        if let Some(entry) = self.current_mut() {
            toggle_shown(&mut entry.source.schemas, name, all);
        }
    }

    /// The selected source's database `name` shown or not (all shown: all but it, then).
    pub fn toggle_database(&mut self, name: &str, all: &[String]) {
        if let Some(entry) = self.current_mut() {
            toggle_shown(&mut entry.source.databases, name, all);
        }
    }

    /// Every database shown again.
    pub fn show_all_databases(&mut self) {
        if let Some(entry) = self.current_mut() {
            entry.source.databases.clear();
        }
    }

    /// The databases known for the selected source: those the last test found, and those
    /// it shows.
    pub fn databases(&self) -> Vec<String> {
        let Some(entry) = self.current() else { return Vec::new() };
        let mut all: Vec<String> = self
            .databases_found
            .iter()
            .find(|(id, _)| *id == entry.source.id)
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        for d in &entry.source.databases {
            if !all.contains(d) {
                all.push(d.clone());
            }
        }
        all
    }

    /// A database is shown in the tree.
    pub fn shows_database(&self, name: &str) -> bool {
        self.current().is_some_and(|e| {
            e.source.databases.is_empty() || e.source.databases.iter().any(|s| s == name)
        })
    }

    /// Every schema shown again.
    pub fn show_all_schemas(&mut self) {
        if let Some(entry) = self.current_mut() {
            entry.source.schemas.clear();
        }
    }

    /// The schemas known for the selected source: those the last test found, and those it
    /// shows.
    pub fn schemas(&self) -> Vec<String> {
        let Some(entry) = self.current() else { return Vec::new() };
        let mut all: Vec<String> = self
            .schemas_found
            .iter()
            .find(|(id, _)| *id == entry.source.id)
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        for s in &entry.source.schemas {
            if !all.contains(s) {
                all.push(s.clone());
            }
        }
        all
    }

    /// A schema is shown in the tree.
    pub fn shows_schema(&self, name: &str) -> bool {
        self.current().is_some_and(|e| {
            e.source.schemas.is_empty() || e.source.schemas.iter().any(|s| s == name)
        })
    }

    /// The names taken (a new source's is made unique among them).
    fn names(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.source.name.clone()).collect()
    }

    /// A new source of `driver` (the project's), selected; its id.
    pub fn add(&mut self, driver: &str) -> Option<DataSourceId> {
        let info = self.driver(driver)?.clone();
        let names = self.names();
        let name = database::unique_name(&info.name, names.iter().map(String::as_str));
        let mut source = DataSource::new(name, driver, Scope::Project);
        if info.file_based {
            source.secrets = SecretPolicy::Never;
        }
        let id = source.id;
        self.entries.push(Entry::new(source, None));
        self.sort();
        let at = self.entries.iter().position(|e| e.source.id == id);
        self.filter.set_text("");
        self.select(at);
        self.tab = Tab::General;
        Some(id)
    }

    /// The selected source copied (a new identity, its name unique), selected.
    pub fn duplicate(&mut self) {
        let Some(entry) = self.current().cloned() else { return };
        let names = self.names();
        let mut source = entry.source.clone();
        source.id = DataSourceId::new();
        source.name = database::unique_name(&entry.source.name, names.iter().map(String::as_str));
        let mut copy = Entry::new(source, None);
        if entry.password_typed {
            copy.password = entry.password;
            copy.password_typed = true;
        } else {
            copy.copied_from = entry.original;
        }
        let id = copy.source.id;
        let after = self.selected.unwrap_or(self.entries.len()) + 1;
        self.entries.insert(after.min(self.entries.len()), copy);
        self.sort();
        let at = self.entries.iter().position(|e| e.source.id == id);
        self.select(at);
    }

    /// The selected source removed (the catalog's on Apply).
    pub fn remove(&mut self) {
        let Some(at) = self.selected else { return };
        let entry = self.entries.remove(at);
        if let Some(original) = entry.original {
            self.removed.push(original);
        }
        let visible = self.visible();
        let next = visible.iter().copied().find(|i| *i >= at).or_else(|| visible.last().copied());
        self.select(next);
    }

    /// The selected source moved to the other list (the project's ↔ every project's), at
    /// its top (its folder is the other list's no more).
    pub fn move_scope(&mut self) {
        let Some(entry) = self.current_mut() else { return };
        entry.source.scope = match entry.source.scope {
            Scope::Project => Scope::Global,
            Scope::Global => Scope::Project,
        };
        entry.source.folder = None;
        self.sort();
    }

    /// The selected source is in the project's list.
    pub fn scope(&self) -> Option<Scope> {
        self.current().map(|e| e.source.scope)
    }

    /// The problems keeping Apply back.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        for (i, entry) in self.entries.iter().enumerate().filter(|(_, e)| e.changed()) {
            if entry.source.name.trim().is_empty() {
                out.push(Problem::NoName(i));
            }
            if let Some(driver) = self.driver(&entry.source.driver) {
                if self.selected == Some(i) {
                    // The selected one's fields as typed (a port that isn't one never
                    // reaches the source).
                    for (id, input) in &self.specs {
                        if let Some(field) = driver.spec.field(id)
                            && let Err(error) = fields::validate(field, input.text())
                        {
                            out.push(Problem::Field(i, id.clone(), error));
                        }
                    }
                } else {
                    for (field, error) in driver.spec.problems(&entry.source) {
                        out.push(Problem::Field(i, field, error));
                    }
                }
            }
        }
        // The selected one's options as typed (one named like a secret is never kept).
        if let Some(at) = self.selected {
            for (k, _) in &self.options {
                if database::source::is_secret_option(k.text().trim()) {
                    out.push(Problem::SecretOption(at, k.text().trim().to_owned()));
                }
            }
        }
        out
    }

    /// The selected source's field `id`'s problem, if it has one.
    pub fn field_problem(&self, id: &str) -> Option<fields::FieldError> {
        let at = self.selected?;
        self.problems().into_iter().find_map(|p| match p {
            Problem::Field(i, f, e) if i == at && f == id => Some(e),
            _ => None,
        })
    }

    /// What Apply makes of the changes.
    pub fn plan(&self) -> Plan {
        let mut plan = Plan::default();
        let mut typed = Vec::new();
        let mut copies = Vec::new();
        for entry in &self.entries {
            let secret = entry.password_typed.then(|| Secret::new(entry.password.text()));
            match &entry.original {
                None => {
                    plan.add.push(entry.source.clone());
                    match (&secret, &entry.copied_from) {
                        (Some(s), _) => typed.push(SecretOp::Set(entry.source.clone(), s.clone())),
                        (None, Some(from)) => copies
                            .push(SecretOp::Copy { from: from.clone(), to: entry.source.clone() }),
                        _ => {}
                    }
                }
                Some(original) => {
                    if *original != entry.source {
                        plan.update.push(entry.source.clone());
                        if original.secrets != entry.source.secrets
                            || original.scope != entry.source.scope
                        {
                            plan.secrets.push(SecretOp::Move {
                                from: original.clone(),
                                to: entry.source.clone(),
                            });
                        }
                    }
                    if let Some(s) = secret {
                        typed.push(SecretOp::Set(entry.source.clone(), s));
                    }
                }
            }
        }
        plan.folders = self.changed_folders();
        plan.secrets.extend(typed);
        plan.secrets.extend(copies);
        for removed in &self.removed {
            plan.remove.push(removed.clone());
            plan.secrets.push(SecretOp::Remove(removed.clone()));
        }
        plan
    }

    /// Applied: what the catalog has now is the edited lists (nothing left to apply).
    pub fn applied(&mut self) {
        self.removed.clear();
        self.original_folders = self.folders.clone();
        for entry in &mut self.entries {
            entry.original = Some(entry.source.clone());
            entry.password = TextInput::new("");
            entry.password.set_masked(true);
            entry.password_typed = false;
            entry.copied_from = None;
        }
    }

    /// The selected source's field `id` as the driver describes it.
    pub fn field(&self, id: &str) -> Option<FieldSpec> {
        self.spec().field(id).cloned()
    }

    /// The fields of the selected source shown in `tab` (its password, too, in General).
    pub fn fields_in(&self, tab: FieldTab) -> Vec<FieldSpec> {
        self.spec().fields_in(tab).cloned().collect()
    }

    /// The text of the selected source's field `id` (empty: its default meant).
    pub fn text_of(&self, id: &str) -> String {
        self.specs
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, t)| t.text().to_owned())
            .unwrap_or_default()
    }

    /// A choice field's current value (its default when none).
    pub fn choice_of(&self, field: &FieldSpec) -> String {
        let text = self.text_of(&field.id);
        if text.is_empty() { field.default.clone() } else { text }
    }

    /// The fields that take typed text, in order (for ⇥).
    pub fn texts(&self) -> Vec<Field> {
        if self.selected_folder.is_some() {
            return vec![Field::FolderName];
        }
        let mut out = vec![Field::Name, Field::Comment];
        let Some(entry) = self.current() else { return vec![Field::Filter] };
        let spec = self.spec();
        let typed = |f: &FieldSpec| !matches!(f.kind, FieldKind::Choice(_) | FieldKind::Flag);
        match self.tab {
            Tab::General => {
                for f in spec.fields_in(FieldTab::General).filter(|f| typed(f)) {
                    out.push(if f.is_secret() {
                        Field::Password
                    } else {
                        Field::Spec(f.id.clone())
                    });
                }
                if spec.url.is_some() {
                    out.push(Field::Url);
                }
            }
            Tab::Options => {
                out.extend(
                    spec.fields_in(FieldTab::Options)
                        .filter(|f| typed(f))
                        .map(|f| Field::Spec(f.id.clone())),
                );
                if !self.driver(&entry.source.driver).is_some_and(|d| d.file_based) {
                    out.push(Field::Startup);
                }
            }
            Tab::Ssl => {
                out.extend(
                    spec.fields_in(FieldTab::Ssl)
                        .filter(|f| typed(f))
                        .map(|f| Field::Spec(f.id.clone())),
                );
            }
            Tab::Schemas => {}
            Tab::Advanced => {
                for i in 0..self.options.len() {
                    out.push(Field::OptionKey(i));
                    out.push(Field::OptionValue(i));
                }
            }
        }
        out
    }

    /// An advanced option row added (empty), its name's field returned.
    pub fn add_option(&mut self) -> Field {
        self.options.push((TextInput::new(""), TextInput::new("")));
        Field::OptionKey(self.options.len() - 1)
    }

    pub fn remove_option(&mut self, i: usize) {
        if i < self.options.len() {
            self.options.remove(i);
            self.store();
            self.write_url();
        }
    }

    /// Where the selected source's file field points (a driver's `File` field: its id).
    pub fn file_fields(&self) -> Vec<String> {
        self.spec()
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::File { .. }))
            .map(|f| f.id.clone())
            .collect()
    }

    /// A file picked for field `id`.
    pub fn picked(&mut self, id: &str, path: &str) {
        let field = Field::Spec(id.to_owned());
        if let Some(input) = self.input_mut(&field) {
            input.set_text(path);
        }
        self.edited(&field);
    }

    /// The selected source's password binding's field (a server's).
    pub fn has_password(&self) -> bool {
        self.spec().fields.iter().any(|f| f.binding == Binding::Password)
    }
}

// ─── Folders ───

/// A row of the list on the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRow {
    /// A list's title (the project's, every project's).
    Section(Scope),
    /// A list with nothing in it (or nothing the filter keeps).
    Empty(Scope),
    Folder {
        id: FolderId,
        depth: u16,
        open: bool,
        count: usize,
    },
    /// Entry `index`, `depth` folders in.
    Source {
        index: usize,
        depth: u16,
    },
}

/// Where something dragged (or moved from a menu) goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A list's top.
    Top(Scope),
    Folder(FolderId),
}

/// What's selected in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Source(DataSourceId),
    Folder(FolderId),
}

impl Editor {
    /// The list's rows: by list, each folder (by name) before the sources at its level, a
    /// closed folder's insides left out. With a filter: the sources it keeps with the
    /// folders on their way, and every folder named like it with all it holds.
    pub fn list_rows(&self) -> Vec<ListRow> {
        let typed = self.filter.text().trim().to_lowercase();
        let filtering = !typed.is_empty();
        let mut out = Vec::new();
        for scope in [Scope::Project, Scope::Global] {
            out.push(ListRow::Section(scope));
            let before = out.len();
            self.level_rows((scope, None, 0), &typed, filtering, false, &mut out);
            if out.len() == before {
                out.push(ListRow::Empty(scope));
            }
        }
        out
    }

    fn level_rows(
        &self,
        (scope, parent, depth): (Scope, Option<FolderId>, u16),
        typed: &str,
        filtering: bool,
        all: bool,
        out: &mut Vec<ListRow>,
    ) {
        for f in folders::children(&self.folders, scope, parent) {
            let named = all || (filtering && f.name.to_lowercase().contains(typed));
            let open = filtering || !self.collapsed.contains(&f.id);
            let count = self.count_in(f.id);
            let at = out.len();
            out.push(ListRow::Folder { id: f.id, depth, open, count });
            if open {
                self.level_rows((scope, Some(f.id), depth + 1), typed, filtering, named, out);
            }
            // Filtering: a folder neither named like it nor holding a match goes.
            if filtering && !named && out.len() == at + 1 {
                out.truncate(at);
            }
        }
        for (index, e) in self.entries.iter().enumerate() {
            if e.source.scope != scope || self.folder_of(&e.source) != parent {
                continue;
            }
            if filtering && !all && !e.source.name.to_lowercase().contains(typed) {
                continue;
            }
            out.push(ListRow::Source { index, depth });
        }
    }

    /// The folder `source` is shown in (none known of its list: the top).
    fn folder_of(&self, source: &DataSource) -> Option<FolderId> {
        source.folder.filter(|id| {
            folders::folder(&self.folders, *id).is_some_and(|f| f.scope == source.scope)
        })
    }

    /// The sources in folder `id` and those inside it.
    pub fn count_in(&self, id: FolderId) -> usize {
        let mut inside = folders::descendants(&self.folders, id);
        inside.push(id);
        self.entries.iter().filter(|e| e.source.folder.is_some_and(|f| inside.contains(&f))).count()
    }

    pub fn folder(&self, id: FolderId) -> Option<&SourceFolder> {
        folders::folder(&self.folders, id)
    }

    /// The selected folder, if a folder is selected.
    pub fn current_folder(&self) -> Option<&SourceFolder> {
        self.folder(self.selected_folder?)
    }

    /// Folder `id` opened or closed in the list.
    pub fn toggle_folder(&mut self, id: FolderId) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }

    /// Folder `id` selected (its name and color shown on the right).
    pub fn select_folder(&mut self, id: FolderId) {
        if self.folder(id).is_none() {
            return;
        }
        self.select(None);
        self.selected_folder = Some(id);
        self.folder_name =
            TextInput::new(&self.folder(id).map(|f| f.name.clone()).unwrap_or_default());
    }

    /// Where a new folder goes: inside the selected folder, beside the selected source
    /// (in its folder), else at the project's top.
    pub fn new_folder_place(&self) -> Place {
        if let Some(f) = self.current_folder() {
            return Place::Folder(f.id);
        }
        match self.current() {
            Some(e) => match self.folder_of(&e.source) {
                Some(id) => Place::Folder(id),
                None => Place::Top(e.source.scope),
            },
            None => Place::Top(Scope::Project),
        }
    }

    /// A new folder at `place` (its name unique beside it), selected; its id.
    pub fn add_folder(&mut self, place: Place, name: &str) -> FolderId {
        let (scope, parent) = match place {
            Place::Top(scope) => (scope, None),
            Place::Folder(id) => match self.folder(id) {
                Some(f) => (f.scope, Some(id)),
                None => (Scope::Project, None),
            },
        };
        let taken: Vec<String> = folders::children(&self.folders, scope, parent)
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let name = database::unique_name(name, taken.iter().map(String::as_str));
        let mut folder = SourceFolder::new(name, scope);
        folder.parent = parent;
        let id = folder.id;
        self.folders.push(folder);
        if let Some(parent) = parent {
            self.collapsed.remove(&parent);
        }
        self.select_folder(id);
        // Its name selected: what's typed replaces it.
        self.folder_name.select_all();
        id
    }

    /// The selected folder's name as typed.
    fn store_folder_name(&mut self) {
        let name = self.folder_name.text().trim().to_owned();
        let Some(id) = self.selected_folder else { return };
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.name = name;
        }
    }

    pub fn set_folder_color(&mut self, id: FolderId, color: Option<SourceColor>) {
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.color = color;
        }
    }

    /// The selected source's own color (`None`: its folder's).
    pub fn set_color(&mut self, color: Option<SourceColor>) {
        if let Some(entry) = self.current_mut() {
            entry.source.color = color;
        }
    }

    /// The color entry `index` is shown in (its own, else its folder's).
    pub fn color_of(&self, index: usize) -> Option<SourceColor> {
        let source = &self.entries.get(index)?.source;
        source.color.or_else(|| {
            self.folder_of(source).and_then(|f| folders::folder_color(&self.folders, f))
        })
    }

    /// Folder `id`'s color as shown (its own, else its parent's).
    pub fn folder_color(&self, id: FolderId) -> Option<SourceColor> {
        folders::folder_color(&self.folders, id)
    }

    /// Source `id` moved to `place` (another list's: it goes to that list too).
    pub fn move_source(&mut self, id: DataSourceId, place: Place) -> bool {
        let (scope, folder) = match place {
            Place::Top(scope) => (scope, None),
            Place::Folder(f) => match self.folder(f) {
                Some(folder) => (folder.scope, Some(f)),
                None => return false,
            },
        };
        let Some(entry) = self.entries.iter_mut().find(|e| e.source.id == id) else { return false };
        if entry.source.scope == scope && entry.source.folder == folder {
            return false;
        }
        entry.source.folder = folder;
        entry.source.scope = scope;
        if let Some(folder) = folder {
            self.collapsed.remove(&folder);
        }
        self.sort();
        true
    }

    /// Folder `id` moved to `place` (its own list only: never into itself nor one inside).
    pub fn move_folder(&mut self, id: FolderId, place: Place) -> bool {
        let Some(scope) = self.folder(id).map(|f| f.scope) else { return false };
        let parent = match place {
            Place::Top(s) if s == scope => None,
            Place::Top(_) => return false,
            Place::Folder(p) => Some(p),
        };
        let moved = folders::move_folder(&mut self.folders, id, parent);
        if moved && let Some(p) = parent {
            self.collapsed.remove(&p);
        }
        moved
    }

    /// The places `item` can be moved to: each list's top, then every folder it may go in.
    pub fn places_for(&self, item: Item) -> Vec<Place> {
        let mut out = Vec::new();
        let scopes: Vec<Scope> = match item {
            Item::Source(_) => vec![Scope::Project, Scope::Global],
            Item::Folder(id) => self.folder(id).map(|f| vec![f.scope]).unwrap_or_default(),
        };
        for scope in scopes {
            out.push(Place::Top(scope));
            let mut stack: Vec<FolderId> =
                folders::children(&self.folders, scope, None).iter().rev().map(|f| f.id).collect();
            while let Some(f) = stack.pop() {
                let fine = match item {
                    Item::Source(_) => true,
                    Item::Folder(id) => folders::can_move_folder(&self.folders, id, Some(f)),
                };
                if fine {
                    out.push(Place::Folder(f));
                }
                for c in folders::children(&self.folders, scope, Some(f)).iter().rev() {
                    stack.push(c.id);
                }
            }
        }
        out
    }

    /// Folder `id` taken away: what it held moves up (no source goes with it).
    pub fn delete_folder(&mut self, id: FolderId) {
        let mut sources: Vec<DataSource> = self.entries.iter().map(|e| e.source.clone()).collect();
        if !folders::delete_folder(&mut self.folders, &mut sources, id) {
            return;
        }
        for (entry, source) in self.entries.iter_mut().zip(sources) {
            entry.source.folder = source.folder;
        }
        if self.selected_folder == Some(id) {
            self.selected_folder = None;
        }
        self.collapsed.remove(&id);
    }

    /// The folders changed here, by list (Apply sets them).
    fn changed_folders(&self) -> Vec<(Scope, Vec<SourceFolder>)> {
        [Scope::Project, Scope::Global]
            .into_iter()
            .filter_map(|scope| {
                let now: Vec<SourceFolder> =
                    self.folders.iter().filter(|f| f.scope == scope).cloned().collect();
                let was: Vec<SourceFolder> =
                    self.original_folders.iter().filter(|f| f.scope == scope).cloned().collect();
                (now != was).then_some((scope, now))
            })
            .collect()
    }
}

/// `name` shown or not among `shown` (none: all of `all`): all shown, all but it then; the
/// last hidden one shown again, all again (none listed).
fn toggle_shown(shown: &mut Vec<String>, name: &str, all: &[String]) {
    if shown.is_empty() {
        *shown = all.iter().filter(|s| *s != name).cloned().collect();
    } else if let Some(at) = shown.iter().position(|s| s == name) {
        shown.remove(at);
    } else {
        shown.push(name.to_owned());
        if all.iter().all(|s| shown.contains(s)) {
            shown.clear();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/sources/form.rs"]
mod tests;
