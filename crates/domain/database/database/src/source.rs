//! Data sources: what the user configured (a server, a SQLite file, a CSV), where it's kept
//! (the project's list or every project's), and how its password is kept. A source never
//! holds a secret: those are in a [`kernel::SecretStore`], by [`DataSource::secret_id`].

use std::fmt;
use std::path::{Path, PathBuf};

use kernel::Rgba;

use crate::Target;
use crate::folders::{FolderId, SourceColor, SourceList};
use crate::relations::RelationNotes;

/// A data source's identity: a UUID v4, never its name (two may share one; a name changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DataSourceId(u128);

impl DataSourceId {
    /// A new identity (UUID v4).
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }

    pub const fn from_u128(id: u128) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u128 {
        self.0
    }

    /// The identity of database `name` of this source's server (a session of its own,
    /// opened on that database): the same pair always gives the same one
    /// ([`kernel::id::derived_uuid`]), never this source's own.
    pub fn of_database(self, name: &str) -> Self {
        Self(kernel::id::derived_uuid(self.0, name))
    }

    /// From its saved text; `None` if it isn't a UUID.
    pub fn parse(text: &str) -> Option<Self> {
        kernel::id::parse_uuid(text).map(Self)
    }
}

impl Default for DataSourceId {
    /// A new identity: a source made without saying which is another source.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DataSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&kernel::id::uuid_text(self.0))
    }
}

/// Where a source is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    /// In the user's settings: every project sees it.
    Global,
    /// In the project's data folder: it goes with the project.
    #[default]
    Project,
}

/// How a source's password is kept (never in its configuration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SecretPolicy {
    /// Never kept: asked at each connection (or none needed: a file).
    Never,
    /// Kept in memory until the project closes.
    Session,
    /// In the project's vault, encrypted, its key in the system's keychain.
    #[default]
    ProjectVault,
    /// In the system's keychain.
    Keychain,
}

/// A label on a source (`prod`, `local`…), with its color in the explorer.
#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub name: String,
    pub color: Rgba,
}

/// A configured source: which driver opens it, where it goes, and how carefully.
#[derive(Debug, Clone, PartialEq)]
pub struct DataSource {
    pub id: DataSourceId,
    pub name: String,
    /// The driver's id ([`crate::DriverFactory::id`]): `postgres`, `sqlite`, `csv`…
    pub driver: String,
    pub scope: Scope,
    pub target: Target,
    /// Nothing written through it: no edits, no schema changes (the UI offers none).
    pub read_only: bool,
    pub tags: Vec<Tag>,
    /// A file-based source's file: relative to the project's root when inside it (the
    /// project can move), absolute otherwise.
    pub file: Option<PathBuf>,
    pub secrets: SecretPolicy,
    /// What the user wrote about it.
    pub comment: String,
    /// The schemas (or databases) shown in its tree; empty: all of them.
    pub schemas: Vec<String>,
    /// For a server with several databases (PostgreSQL's), those shown in its tree; empty:
    /// all it may connect to. Its own database is only the one it opens first.
    pub databases: Vec<String>,
    /// The relations between its tables the user decided on (keys accepted as virtual
    /// ones, those set aside) and which its diagram shows.
    pub relations: RelationNotes,
    /// The folder it's in (one of its list's); `None`: at the list's top.
    pub folder: Option<FolderId>,
    /// Its own color; `None`: its folder's ([`crate::folders::effective_color`]).
    pub color: Option<SourceColor>,
    /// Marked as production by the user (or by a folder it's in:
    /// [`crate::folders::effective_production`]): a console's writes are confirmed and its
    /// transactions are manual by default. Never guessed from a name or a color.
    pub production: bool,
}

impl DataSource {
    /// A new source of `driver`, with a new identity and the defaults.
    pub fn new(name: impl Into<String>, driver: impl Into<String>, scope: Scope) -> Self {
        Self {
            id: DataSourceId::new(),
            name: name.into(),
            driver: driver.into(),
            scope,
            target: Target::default(),
            read_only: false,
            tags: Vec::new(),
            file: None,
            secrets: SecretPolicy::default(),
            comment: String::new(),
            schemas: Vec::new(),
            databases: Vec::new(),
            relations: RelationNotes::default(),
            folder: None,
            color: None,
            production: false,
        }
    }

    /// The id its password is kept under in a [`kernel::SecretStore`] (shared with other
    /// contexts' secrets, hence the prefix).
    pub fn secret_id(&self) -> String {
        format!("data-source/{}", self.id)
    }

    /// Its file, absolute (a relative one is under `project_root`).
    pub fn file_in(&self, project_root: &Path) -> Option<PathBuf> {
        let file = self.file.as_ref()?;
        Some(if file.is_absolute() { file.clone() } else { project_root.join(file) })
    }
}

impl DataSource {
    /// `other` connects as it does: they differ, if at all, only in where they're shown
    /// (their folder, their color) or how carefully (production) — a session open on it
    /// stays open.
    pub fn same_connection(&self, other: &Self) -> bool {
        let shown = |s: &Self| Self { folder: None, color: None, production: false, ..s.clone() };
        shown(self) == shown(other)
    }

    /// It says where to go: a file-based source its file, a server's its host (or, for an
    /// engine that needs none, its database). A source added from the `+` menu before its
    /// settings are filled in isn't: nothing connects until it is.
    pub fn is_configured(&self, file_based: bool) -> bool {
        if file_based {
            return self.file.as_ref().is_some_and(|f| !f.as_os_str().is_empty());
        }
        !self.target.host.trim().is_empty() || !self.target.database.trim().is_empty()
    }
}

/// The drivers used last: those of `sources` (oldest first, as their lists keep them), the
/// newest first, each once, at most `limit` — the `+` menu's "recent" ones.
pub fn recent_drivers<'a>(
    sources: impl IntoIterator<Item = &'a DataSource>,
    limit: usize,
) -> Vec<String> {
    let all: Vec<&DataSource> = sources.into_iter().collect();
    let mut out: Vec<String> = Vec::new();
    for source in all.iter().rev() {
        if out.len() == limit {
            break;
        }
        if !out.contains(&source.driver) {
            out.push(source.driver.clone());
        }
    }
    out
}

/// `base`, or `base (2)`, `base (3)`… — the first no name of `taken` already is (a new
/// source's name: two may share one, but the list reads better when they don't).
pub fn unique_name<'a>(base: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let free = |name: &str| !taken.clone().into_iter().any(|t| t == name);
    if free(base) {
        return base.to_owned();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|n| free(n)).unwrap_or_else(|| base.to_owned())
}

/// The option names never written with a source (a password typed as an option): their
/// value is a secret, kept by [`SecretPolicy`] only.
pub fn is_secret_option(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["password", "passwd", "pwd", "secret", "token", "passphrase", "api_key", "apikey"]
        .iter()
        .any(|s| name == *s || name.contains(s))
}

/// `path` as a source keeps it: relative to `project_root` when inside it.
pub fn stored_path(path: &Path, project_root: &Path) -> PathBuf {
    path.strip_prefix(project_root).map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

/// The sources' port: the project's list (`<project dir>/data-sources.toml`) and the
/// global one (in the user's config folder), each saved whole with its folders — never
/// with a secret.
pub trait DataSourceStore: Send + Sync {
    /// The sources and folders of `scope`, as saved (none yet: empty). An error when its
    /// file can't be read as one (it's then never written over: nothing in it is lost).
    fn load(&self, scope: Scope) -> Result<SourceList, String>;
    /// `list` written as `scope`'s whole list (atomically: a half-written file never
    /// replaces a good one), without any secret.
    fn save(&self, scope: Scope, list: &SourceList) -> Result<(), String>;
}

/// A store keeping nothing (a file outside any project, the tests): every list empty.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoStore;

impl DataSourceStore for NoStore {
    fn load(&self, _scope: Scope) -> Result<SourceList, String> {
        Ok(SourceList::default())
    }

    fn save(&self, _scope: Scope, _list: &SourceList) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/source.rs"]
mod tests;
