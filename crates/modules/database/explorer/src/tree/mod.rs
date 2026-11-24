//! The "Database" window's tree, as a model (no drawing, no I/O): the sources, then what
//! each holds as it's read — source › schema › tables / views › table › columns /
//! indexes / foreign keys — with the counts beside the nodes. A server with several
//! databases (PostgreSQL's) has them between: source › database › schema › …, each
//! database read through a session of its own ([`TreeDatabase::id`]: the source's own for
//! the database it opens, another identity for each other one), all those the user may
//! connect to listed (or those the source shows), the source's own marked. A node's children are read
//! only when it's opened: [`SourceTree::expand`] says what to ask for ([`Need`]), the
//! answers come back through [`SourceTree::set_tables`] / [`SourceTree::set_structure`];
//! until then a node shows a loading row, after a failure an error row.
//!
//! Its rows ([`SourceTree::rows`]) are what's open, or — with a filter — every node read so
//! far that matches it, with the nodes on the way to it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use database::folders::{FolderId, SourceColor, SourceFolder};
use database::{DataSourceId, Scope, Structure, TableInfo, TableKind, TableRef};

mod reading;
mod rows;
mod table_rows;

pub use rows::nearest;

/// A source as the tree shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSource {
    pub id: DataSourceId,
    pub name: String,
    /// The driver's id, and its name (shown beside it).
    pub driver: String,
    pub driver_name: String,
    pub icon: &'static str,
    /// The tone its icon is drawn in (its engine's).
    pub tone: database::IconTone,
    /// Where it's listed; `None`: a file shown by itself, not configured (only while open).
    pub scope: Option<Scope>,
    /// A file-based source's file (absolute).
    pub file: Option<PathBuf>,
    /// It says where to go (nothing is connected until it does).
    pub configured: bool,
    pub read_only: bool,
    /// The structure changes its driver makes (none for a read-only source).
    pub schema_changes: database::SchemaChanges,
    /// The schemas shown (none: all).
    pub schemas: Vec<String>,
    /// Its server has several databases, shown under it (source › database › schema).
    pub databases: bool,
    /// The databases shown (none: all).
    pub shown_databases: Vec<String>,
    /// A server's name and version, once connected (`PostgreSQL 16.4`).
    pub server: Option<String>,
    /// The folder it's in (of its list's folders); `None`: at its list's top.
    pub folder: Option<FolderId>,
    /// The color it's shown in (its own, else its folder's).
    pub color: Option<SourceColor>,
    /// Production (marked so, or its folder is).
    pub production: bool,
    /// A console of it has a transaction open (`TransactionChanged`).
    pub transaction: bool,
    /// Its session: open, closed, or couldn't open (`ConnectionChanged`).
    pub link: Link,
}

/// A group under a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Tables,
    Views,
}

/// A part of a table's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Columns,
    Indexes,
    Keys,
}

/// A node, by what it is (stable while the tree is read again: what's open and selected
/// stays so).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeKey {
    /// The lists' own notice (a list that can't be read): not a source's.
    Notice(Scope),
    /// A folder of sources.
    Folder(FolderId),
    Source(DataSourceId),
    /// A database of a source's server, by its own identity ([`TreeDatabase::id`]: the
    /// source's for the one it opens).
    Database(DataSourceId),
    Schema(DataSourceId, String),
    Group(DataSourceId, Option<String>, Group),
    Table(DataSourceId, TableRef),
    Part(DataSourceId, TableRef, Part),
    Column(DataSourceId, TableRef, String),
    Index(DataSourceId, TableRef, String),
    Key(DataSourceId, TableRef, usize),
    /// A loading, error or hint row under `parent`.
    Status(Box<NodeKey>),
}

impl NodeKey {
    /// The source it's under.
    pub fn source(&self) -> Option<DataSourceId> {
        match self {
            Self::Notice(_) | Self::Folder(_) => None,
            Self::Source(id)
            | Self::Database(id)
            | Self::Schema(id, _)
            | Self::Group(id, ..)
            | Self::Table(id, _)
            | Self::Part(id, ..)
            | Self::Column(id, ..)
            | Self::Index(id, ..)
            | Self::Key(id, ..) => Some(*id),
            Self::Status(parent) => parent.source(),
        }
    }

    /// The table it's in (or is).
    pub fn table(&self) -> Option<&TableRef> {
        match self {
            Self::Table(_, t)
            | Self::Part(_, t, _)
            | Self::Column(_, t, _)
            | Self::Index(_, t, _)
            | Self::Key(_, t, _) => Some(t),
            Self::Status(parent) => parent.table(),
            _ => None,
        }
    }
}

/// What a row is (its icon, its menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    Folder,
    Source,
    Database,
    Schema,
    Group(Group),
    Table(TableKind),
    Part(Part),
    Column {
        key: bool,
        foreign: bool,
    },
    Index {
        unique: bool,
    },
    Key,
    Loading,
    Error,
    /// A note (a source not configured yet, nothing in a list).
    Hint,
}

/// A row of the tree as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: NodeKey,
    pub depth: u16,
    pub kind: RowKind,
    pub label: String,
    /// After the label, dimmed (a column's type, a driver's name, a table's rows).
    pub detail: String,
    /// The badge at the end (tables in a schema, columns of a table).
    pub count: Option<usize>,
    pub expandable: bool,
    pub expanded: bool,
    /// A folder's or a source's color (its mark in the row).
    pub color: Option<SourceColor>,
    /// What the row says of how carefully it's used (its badges after the name).
    pub marks: RowMarks,
}

/// A source's or a folder's badges: production, a console's transaction open on it, its
/// session's state (a source's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowMarks {
    pub production: bool,
    pub transaction: bool,
    pub link: Link,
}

/// A source's session, as its icon's mark shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Link {
    /// Not connected (no mark).
    #[default]
    Closed,
    Open,
    /// The last try to open it failed.
    Failed,
}

/// Something read, or being read.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Pending,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// What the tree needs read (the window asks the source's worker).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Need {
    /// A server's databases (source `id`'s).
    Databases(DataSourceId),
    Tables(DataSourceId),
    Structure(DataSourceId, TableRef),
}

/// A database of a source's server, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDatabase {
    pub name: String,
    /// What reads it: the source's own identity for the database it opens, another for
    /// each other one ([`database::DataSourceId::of_database`]).
    pub id: DataSourceId,
    /// The one the source opens (its own).
    pub default: bool,
}

/// What's known of a source (or of a database of its server).
#[derive(Debug, Default)]
struct Known {
    tables: Load<Vec<TableInfo>>,
    structures: HashMap<TableRef, Load<Structure>>,
}

/// The texts the rows show that aren't data (translated by the window).
#[derive(Debug, Clone)]
pub struct Words {
    pub loading: String,
    /// Beside the database a source opens.
    pub default_database: String,
    pub no_databases: String,
    pub not_configured: String,
    pub no_tables: String,
    pub tables: String,
    pub views: String,
    pub columns: String,
    pub indexes: String,
    pub keys: String,
    pub unique: String,
    pub global: String,
    pub read_only: String,
    pub open_file: String,
    /// `{count}` rows (an estimate).
    pub rows: fn(u64) -> String,
}

/// The tree (see the module's doc).
#[derive(Debug, Default)]
pub struct SourceTree {
    sources: Vec<TreeSource>,
    known: HashMap<DataSourceId, Known>,
    expanded: HashSet<NodeKey>,
    /// The lists that can't be read: why (their notice at the top).
    notices: Vec<(Scope, String)>,
    /// The lists' folders (the sources are shown in them).
    folders: Vec<SourceFolder>,
    /// The folders closed (open unless closed: what's in them is seen at once).
    closed: HashSet<FolderId>,
    /// Each server's databases, as read (by the source's identity).
    databases: HashMap<DataSourceId, Load<Vec<TreeDatabase>>>,
    /// The databases' own sources (another database than its source's: its identity, the
    /// source with its name), by their identity.
    subs: Vec<TreeSource>,
}

impl SourceTree {
    pub fn sources(&self) -> &[TreeSource] {
        &self.sources
    }

    /// Source `id`, or a database of a source's server as its own source (its identity,
    /// its name with the database's).
    pub fn source(&self, id: DataSourceId) -> Option<&TreeSource> {
        self.sources.iter().find(|s| s.id == id).or_else(|| self.subs.iter().find(|s| s.id == id))
    }

    /// The source whose server database `id` is (itself for a source).
    pub fn base_of(&self, id: DataSourceId) -> DataSourceId {
        if self.sources.iter().any(|s| s.id == id) {
            return id;
        }
        self.databases
            .iter()
            .find(|(_, load)| load.ready().is_some_and(|l| l.iter().any(|d| d.id == id)))
            .map_or(id, |(base, _)| *base)
    }

    /// Source `id`'s databases, as read.
    pub fn databases(&self, id: DataSourceId) -> Option<&[TreeDatabase]> {
        self.databases.get(&id)?.ready().map(Vec::as_slice)
    }

    /// The identities of source `id` and of its server's databases.
    fn family(&self, id: DataSourceId) -> Vec<DataSourceId> {
        let mut ids = vec![id];
        if let Some(list) = self.databases(id) {
            ids.extend(list.iter().map(|d| d.id).filter(|d| *d != id));
        }
        ids
    }

    /// The databases' own sources, made again from their sources (their names, flags).
    fn make_subs(&mut self) {
        let mut subs = Vec::new();
        for source in &self.sources {
            let Some(list) = self.databases.get(&source.id).and_then(Load::ready) else {
                continue;
            };
            for d in list.iter().filter(|d| d.id != source.id) {
                subs.push(TreeSource {
                    id: d.id,
                    name: crate::catalog::database_name(&source.name, &d.name),
                    databases: false,
                    ..source.clone()
                });
            }
        }
        self.subs = subs;
    }

    /// Source `id`'s databases, as read (or why not): each with the identity it's read by.
    pub fn set_databases(
        &mut self,
        id: DataSourceId,
        databases: Result<Vec<TreeDatabase>, String>,
    ) {
        let load = match databases {
            Ok(mut list) => {
                list.sort_by_key(|d| d.name.to_lowercase());
                // What's known of those no longer there goes.
                let gone: Vec<DataSourceId> = self
                    .databases(id)
                    .map(|old| {
                        old.iter()
                            .filter(|o| o.id != id && !list.iter().any(|d| d.id == o.id))
                            .map(|o| o.id)
                            .collect()
                    })
                    .unwrap_or_default();
                for d in gone {
                    self.known.remove(&d);
                }
                Load::Ready(list)
            }
            Err(why) => Load::Failed(why),
        };
        self.databases.insert(id, load);
        self.make_subs();
    }

    /// The sources shown now: what's known of those still there kept (open, read), of the
    /// others forgotten.
    pub fn set_sources(&mut self, sources: Vec<TreeSource>) {
        let mut ids: HashSet<DataSourceId> = sources.iter().map(|s| s.id).collect();
        self.databases.retain(|id, _| ids.contains(id));
        // A database's own identity stays while its source does.
        for list in self.databases.values().filter_map(Load::ready) {
            ids.extend(list.iter().map(|d| d.id));
        }
        self.known.retain(|id, _| ids.contains(id));
        self.expanded.retain(|k| k.source().is_none_or(|id| ids.contains(&id)));
        // A source whose settings changed (another file, configured now) is read again.
        // (Its server's version, known once connected, isn't a setting.)
        // Nor where it's shown (its folder, its color).
        let bare = |s: &TreeSource| TreeSource {
            server: None,
            folder: None,
            color: None,
            production: false,
            transaction: false,
            link: Link::Closed,
            ..s.clone()
        };
        for source in &sources {
            if self.sources.iter().any(|s| s.id == source.id && bare(s) != bare(source)) {
                for id in self.family(source.id) {
                    self.known.remove(&id);
                }
                self.databases.remove(&source.id);
            }
        }
        self.sources = sources;
        self.make_subs();
    }

    /// Source `id`'s server, as its session said once connected.
    pub fn set_server(&mut self, id: DataSourceId, server: Option<String>) {
        if let Some(source) = self.sources.iter_mut().find(|s| s.id == id) {
            source.server = server;
        }
    }

    /// The lists' folders (both lists').
    pub fn set_folders(&mut self, folders: Vec<SourceFolder>) {
        self.closed.retain(|id| folders.iter().any(|f| f.id == *id));
        self.folders = folders;
    }

    pub fn folders(&self) -> &[SourceFolder] {
        &self.folders
    }

    /// The notices of the lists that can't be read.
    pub fn set_notices(&mut self, notices: Vec<(Scope, String)>) {
        self.notices = notices;
    }

    pub fn is_expanded(&self, key: &NodeKey) -> bool {
        match key {
            NodeKey::Folder(id) => !self.closed.contains(id),
            _ => self.expanded.contains(key),
        }
    }

    /// The tables of source `id`, as read.
    pub fn tables(&self, id: DataSourceId) -> Option<&[TableInfo]> {
        self.known.get(&id)?.tables.ready().map(Vec::as_slice)
    }

    /// Table `table`'s structure, as read.
    pub fn structure(&self, id: DataSourceId, table: &TableRef) -> Option<&Structure> {
        self.known.get(&id)?.structures.get(table)?.ready()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/tree.rs"]
mod tests;
